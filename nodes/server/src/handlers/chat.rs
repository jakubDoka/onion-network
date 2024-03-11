use {
    super::MissingTopic,
    chat_spec::{
        advance_nonce, retain_messages_in_vec, rpcs, unpack_messages_ref, BlockNumber, ChatError,
        ChatEvent, ChatName, Cursor, Identity, Member, Message, Proof, ReplVec, REPLICATION_FACTOR,
    },
    component_utils::{encode_len, Codec, Reminder},
    dht::{try_peer_id_to_ed, U256},
    libp2p::{futures::StreamExt, PeerId},
    std::{
        collections::{HashMap, HashSet},
        ops::DerefMut,
    },
    tokio::sync::OwnedRwLockWriteGuard,
};

const MAX_MESSAGE_SIZE: usize = 1024 - 40;
const MESSAGE_FETCH_LIMIT: usize = 20;
const BLOCK_SIZE: usize = if cfg!(test) { 1024 * 4 } else { 1024 * 64 };
const MAX_UNFINALIZED_BLOCKS: usize = REPLICATION_FACTOR.get() / 2 + 1;
const UNFINALIZED_BUFFER_CAP: usize = BLOCK_SIZE * MAX_UNFINALIZED_BLOCKS;
const BLOCK_HISTORY: usize = 8;

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub async fn create(
    missing_topic: MissingTopic,
    (name, identity): (ChatName, Identity),
) -> Result<()> {
    crate::ensure!(
        let MissingTopic::Chat { mut lock, .. } = missing_topic,
        ChatError::AlreadyExists
    );

    *lock = Chat::new(identity, name);

    Ok(())
}

pub async fn add_member(
    cx: crate::Context,
    (proof, identity): (Proof<ChatName>, Identity),
) -> Result<()> {
    let chat = cx.chats.get(&proof.context).ok_or(ChatError::NotFound)?.clone();
    let mut chat = chat.write().await;

    crate::ensure!(proof.verify(), ChatError::InvalidProof);

    let sender_id = crypto::hash::from_raw(&proof.pk);
    let sender = chat.members.get_mut(&sender_id).ok_or(ChatError::NotMember)?;

    crate::ensure!(
        advance_nonce(&mut sender.action, proof.nonce),
        ChatError::InvalidChatAction(sender.action)
    );

    crate::ensure!(
        chat.members.try_insert(identity, Member::default()).is_ok(),
        ChatError::AlreadyMember
    );

    Ok(())
}

pub async fn send_message(
    cx: crate::Context,
    group: super::FullReplGroup,
    (proof, Reminder(msg)): (Proof<ChatName>, Reminder<'_>),
) -> Result<()> {
    crate::ensure!(proof.verify(), ChatError::InvalidProof);

    let chat = cx.chats.get(&proof.context).ok_or(ChatError::NotFound)?.clone();

    {
        let mut chat_guard = chat.write().await;
        let chat = chat_guard.deref_mut();

        let identity = crypto::hash::from_raw(&proof.pk);
        let sender = chat.members.get_mut(&identity).ok_or(ChatError::NotMember)?;
        let nonce = sender.action;

        crate::ensure!(
            advance_nonce(&mut sender.action, proof.nonce),
            ChatError::InvalidChatAction(sender.action)
        );

        crate::ensure!(msg.len() <= MAX_MESSAGE_SIZE, ChatError::MessageTooLarge);

        let message = Message { identity, nonce, content: Reminder(msg) };
        let encoded_len = message.encoded_len();

        if chat.buffer.len() + encoded_len + 2 > UNFINALIZED_BUFFER_CAP {
            return Err(ChatError::MessageOverload);
        }

        _ = message.encode(&mut chat.buffer);
        chat.buffer.extend(encode_len(encoded_len));

        if chat.buffer.len() % BLOCK_SIZE > BLOCK_SIZE / 2
            && (chat.buffer.len() - encoded_len - 2) % BLOCK_SIZE < BLOCK_SIZE / 2
        {
            chat.resolve_finalization(cx, group, proof.context).await;
        }
    }

    cx.push_chat_event(proof.context, ChatEvent::Message(proof, Reminder(msg))).await;

    Ok(())
}

pub async fn handle_message_block(
    cx: crate::Context,
    origin: super::Origin,
    group: super::FullReplGroup,
    name: ChatName,
    (number, Reminder(block)): (BlockNumber, Reminder<'_>),
) -> Result<()> {
    use {chat_spec::InvalidBlockReason::*, ChatError::*};

    let chat = cx.chats.get(&name).ok_or(NotFound)?.clone();
    let compressed_origin =
        dht::try_peer_id_to_ed(origin).map(U256::from).expect("its in the table");

    let mut chat = chat.write().await;
    let chat = chat.deref_mut();

    'a: {
        if chat.is_recovering() {
            break 'a;
        }

        let finalizer = chat.select_finalizer(group.clone());
        if finalizer == compressed_origin {
            break 'a;
        }

        log::warn!(
            "not selected to finalize block: us: {} them: {} expected: {} block_len: {}",
            cx.local_peer_id,
            origin,
            decompress_peer_id(finalizer),
            chat.buffer.len()
        );
        return Err(InvalidBlock(NotExpected));
    }

    let proposed = unpack_messages_ref(block).collect::<HashSet<_>>();
    let message_count =
        unpack_messages_ref(&chat.buffer).filter(|msg| proposed.contains(msg)).count();

    if message_count == proposed.len() {
        chat.naturally_created = true;
        retain_messages_in_vec(&mut chat.buffer, |msg| !proposed.contains(msg));
        chat.finalized[(number % BLOCK_HISTORY as u64) as usize] =
            Block { hash: crypto::hash::from_slice(block), data: block.to_vec() };
        chat.number += 1;
        return Ok(());
    }

    if chat.is_recovering() {
        chat.number = number + 1;
        retain_messages_in_vec(&mut chat.buffer, |msg| !proposed.contains(msg));
        return Ok(());
    }

    log::warn!(
        "extra messages in block: message_count: {:?} proposed: {:?} us: {} them: {}",
        message_count,
        proposed.len(),
        cx.local_peer_id,
        origin,
    );

    Err(InvalidBlock(ExtraMessages))
}

pub async fn fetch_chat_data(cx: crate::Context, name: ChatName, _: ()) -> Result<MinimalChatData> {
    let chat = cx.chats.get(&name).ok_or(ChatError::NotFound)?.clone();
    let chat = chat.read().await;
    Ok(MinimalChatData { number: chat.number, members: chat.members.clone() })
}

pub async fn recover(
    cx: crate::Context,
    name: ChatName,
    mut chat: OwnedRwLockWriteGuard<Chat>,
) -> Result<()> {
    let mut repl_chat_data = cx
        .repl_rpc(name, rpcs::FETCH_CHAT_DATA, name)
        .await
        .collect::<ReplVec<_>>()
        .await
        .into_iter()
        .filter_map(|(peer, resp)| {
            resp.as_deref()
                .ok()
                .and_then(|mut r| <_>::decode(&mut r))
                .and_then(Result::<MinimalChatData>::ok)
                .map(|resp| (peer, resp))
        })
        .collect::<ReplVec<_>>();

    if repl_chat_data.len() <= REPLICATION_FACTOR.get() / 2 {
        log::warn!("not enough data to recover chat: {:?}", name);
        cx.chats.remove(&name);
        return Err(ChatError::NotFound);
    }

    let chat_data = reconstruct_chat(&mut repl_chat_data);
    chat.number = chat_data.number;
    chat.members = chat_data.members;

    Ok(())
}

fn reconstruct_chat(block_data: &mut ReplVec<(PeerId, MinimalChatData)>) -> MinimalChatData {
    fn retain_outliers<T>(
        block_data: &mut ReplVec<T>,
        tolerance: impl Fn(usize) -> usize,
        field: impl Fn(&T) -> usize,
    ) {
        if block_data.len() <= REPLICATION_FACTOR.get() {
            return;
        }

        block_data.sort_unstable_by_key(|data| field(data));
        let median = (field(&block_data[block_data.len() / 2 - 1])
            + field(&block_data[block_data.len() / 2]))
            / 2;
        let tolerance = tolerance(median);
        block_data.retain(|data| field(data).abs_diff(median) <= tolerance);
    }

    fn retain_members(
        block_data: &mut ReplVec<(PeerId, MinimalChatData)>,
    ) -> HashMap<Identity, Member> {
        let mut member_count_map = HashMap::<Identity, ReplVec<Member>>::new();
        for (id, other) in block_data.iter().flat_map(|(_, data)| &data.members) {
            member_count_map.entry(*id).or_default().push(*other);
        }
        member_count_map.values_mut().for_each(|v| retain_outliers(v, |_| 10, |m| m.action as _));
        member_count_map.retain(|_, v| v.len() >= REPLICATION_FACTOR.get() / 2);

        member_count_map.into_iter().map(|(id, mut v)| (id, Member::combine(&mut v))).collect()
    }

    retain_outliers(block_data, |_| 0, |(_, data)| data.number as _);
    retain_outliers(block_data, |m| m.clamp(1, 3), |(_, data)| data.members.len());

    let members = retain_members(block_data);
    let number = block_data.first().map(|(_, data)| data.number).unwrap_or(0);
    MinimalChatData { number, members }
}

pub async fn fetch_messages(
    cx: crate::Context,
    (chat, mut cursor): (ChatName, Cursor),
) -> Result<(Cursor, Vec<u8>)> {
    let chat = cx.chats.get(&chat).ok_or(ChatError::NotFound)?.clone();
    let chat = chat.read().await;

    if cursor == Cursor::INIT {
        cursor.block = chat.number + 1;
        cursor.offset = chat.buffer.len();
    }

    let bail = Ok((Cursor::INIT, vec![]));

    if cursor.offset == 0 {
        if cursor.block == 0 {
            return bail;
        }
        cursor.block += 1;
    }

    let block = match chat.number < cursor.block {
        true => &chat.buffer,
        false => &chat.finalized[cursor.block as usize % BLOCK_HISTORY].data,
    };

    if cursor.offset == 0 {
        cursor.offset = block.len();
    }

    let Some(slice) = block.get(..cursor.offset) else { return bail };
    let len = unpack_messages_ref(slice)
        .take(MESSAGE_FETCH_LIMIT)
        .map(|msg| msg.len() + 2)
        .sum::<usize>();
    cursor.offset -= len;

    Ok((cursor, slice[cursor.offset..].to_vec()))
}

#[derive(Default)]
pub struct Block {
    pub hash: crypto::Hash,
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct Chat {
    pub members: HashMap<Identity, Member>,
    pub finalized: [Block; BLOCK_HISTORY],
    pub number: BlockNumber,
    pub buffer: Vec<u8>,
    pub naturally_created: bool,
}

impl Chat {
    pub fn new(id: Identity, _: ChatName) -> Self {
        Self {
            members: [(id, Member::default())].into(),
            naturally_created: true,
            ..Default::default()
        }
    }

    pub fn is_recovering(&self) -> bool {
        !self.naturally_created
    }

    pub async fn resolve_finalization(
        &mut self,
        cx: crate::Context,
        group: super::FullReplGroup,
        name: ChatName,
    ) {
        if self.buffer.len() < BLOCK_SIZE {
            return;
        }

        let selected = self.select_finalizer(group.clone());
        if selected != try_peer_id_to_ed(cx.local_peer_id).map(U256::from).unwrap() {
            return;
        }

        let actual_block_size = {
            let trashold = self.buffer.len() / BLOCK_SIZE * BLOCK_SIZE;
            let mut res = self.buffer.len();
            for msg in unpack_messages_ref(&self.buffer) {
                res -= msg.len() + 2;
                if res <= trashold {
                    break;
                }
            }
            res
        };

        cx.repl_rpc_no_resp(
            name,
            rpcs::SEND_BLOCK,
            (self.number, Reminder(&self.buffer[..actual_block_size])),
        )
        .await;

        self.finalized[(self.number % BLOCK_HISTORY as u64) as usize] = Block {
            hash: crypto::hash::from_slice(&self.buffer[..actual_block_size]),
            data: self.buffer.drain(..actual_block_size).collect(),
        };
        self.number += 1;
    }

    pub fn select_finalizer(&self, members: super::FullReplGroup) -> U256 {
        let Some(last_block) = (self.number > 0)
            .then(|| &self.finalized[((self.number - 1) % BLOCK_HISTORY as u64) as usize])
        else {
            return select_finalizer(members, crypto::Hash::default(), 0);
        };
        select_finalizer(members, last_block.hash, self.buffer.len() - BLOCK_SIZE)
    }
}

pub fn select_finalizer(
    mut members: super::FullReplGroup,
    selector: crypto::Hash,
    block_len: usize,
) -> U256 {
    let selector = U256::from(selector);
    let round_count = block_len / BLOCK_SIZE;

    assert!(round_count < MAX_UNFINALIZED_BLOCKS);
    for _ in 0..round_count {
        let (index, _) =
            members.iter().enumerate().max_by_key(|&(_, &member)| member ^ selector).unwrap();
        members.swap_remove(index);
    }

    members.into_iter().max_by_key(|&member| member ^ selector).unwrap()
}

pub fn decompress_peer_id(compressed: U256) -> PeerId {
    let bytes: [u8; 32] = compressed.into();
    let pk = libp2p::identity::ed25519::PublicKey::try_from_bytes(&bytes).unwrap();
    let key = libp2p::identity::PublicKey::from(pk);
    libp2p::PeerId::from(key)
}

#[derive(component_utils::Codec)]
pub struct MinimalChatData {
    pub number: BlockNumber,
    pub members: HashMap<Identity, Member>,
}
