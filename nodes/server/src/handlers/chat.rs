use {
    super::MissingTopic,
    chat_spec::{
        advance_nonce, retain_messages, rpcs, unpack_messages_ref, BlockNumber, ChatError,
        ChatEvent, ChatName, Cursor, Identity, Member, Message, Proof, ReplVec, REPLICATION_FACTOR,
    },
    component_utils::{arrayvec::ArrayVec, encode_len, Buffer, Codec, Reminder},
    core::panic,
    dht::U256,
    libp2p::{
        futures::{FutureExt, StreamExt},
        PeerId,
    },
    rand_core::block,
    std::{
        collections::{HashMap, HashSet},
        ops::DerefMut,
    },
    tokio::sync::OwnedRwLockWriteGuard,
};

const MAX_MESSAGE_SIZE: usize = 1024;
const MESSAGE_FETCH_LIMIT: usize = 20;
const BLOCK_SIZE: usize = if cfg!(test) { 1024 * 4 } else { 1024 * 128 };
const FINALIZATION_TRIGGER_SIZE: usize = BLOCK_SIZE + BLOCK_SIZE / 2;
const MAX_UNFINALIZED_BLOCKS: usize = 5;
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

        {
            let encoded = message.to_bytes();
            let hash = crypto::hash::from_slice(&encoded);
            assert!(unpack_messages_ref(
                &chat.finalized[(chat.number - 1) as usize % BLOCK_HISTORY]
            )
            .all(|m| crypto::hash::from_slice(m) != hash));
        }

        if chat.buffer.len() + encoded_len + 4 > UNFINALIZED_BUFFER_CAP {
            return Err(ChatError::MessageOverload);
        }

        let should_mark_bound =
            (chat.buffer.len() - chat.block_bounds.last().cloned().unwrap_or(0) + encoded_len + 4)
                >= BLOCK_SIZE;
        if should_mark_bound {
            chat.block_bounds.push(chat.buffer.len());
        }

        let should_fianlize = chat.buffer.len() / FINALIZATION_TRIGGER_SIZE
            < (encoded_len + chat.buffer.len()) / FINALIZATION_TRIGGER_SIZE;

        _ = message.encode(&mut chat.buffer);
        chat.buffer.extend(encode_len(encoded_len));

        if should_fianlize {
            chat.resolve_finalization(cx, &group, proof.context).await;
        }
    }

    cx.push_chat_event(proof.context, ChatEvent::Message(proof, Reminder(msg))).await;

    Ok(())
}

pub fn select_finalizer(members: &[U256], selector: crypto::Hash) -> Option<PeerId> {
    if selector == crypto::Hash::default() {
        return None;
    }

    let selector = U256::from(selector);
    let closest =
        members.iter().min_by_key(|&&id| selector.abs_diff(id)).expect("its always at least us");

    let bytes: [u8; 32] = (*closest).into();
    let pk = libp2p::identity::ed25519::PublicKey::try_from_bytes(&bytes).unwrap();
    let key = libp2p::identity::PublicKey::from(pk);
    Some(libp2p::PeerId::from(key))
}

pub async fn handle_message_block(
    cx: crate::Context,
    origin: super::Origin,
    group: super::FullReplGroup,
    name: ChatName,
    Reminder(block): Reminder<'_>,
) -> Result<()> {
    use {chat_spec::InvalidBlockReason::*, ChatError::*};

    let chat = cx.chats.get(&name).ok_or(NotFound)?.clone();

    let mut chat = chat.write().await;
    let chat = chat.deref_mut();

    if select_finalizer(&group, chat.selector).is_some_and(|id| id != origin) {
        log::warn!("not selected to finalize block: us: {:?} them: {:?}", cx.local_peer_id, origin);
        return Err(InvalidBlock(NotExpected));
    }

    let proposed = unpack_messages_ref(block).map(crypto::hash::from_slice).collect::<HashSet<_>>();
    let message_count = unpack_messages_ref(&chat.buffer)
        .filter(|msg| proposed.contains(&crypto::hash::from_slice(msg)))
        .count();

    if message_count == proposed.len() {
        let kept_len = retain_messages(&mut chat.buffer, |msg| {
            !proposed.contains(&crypto::hash::from_slice(msg))
        })
        .len();

        assert_eq!(chat.buffer.len() - kept_len, block.len());
        assert_eq!(block.len(), chat.block_bounds[0], "{:?}", chat.block_bounds);

        chat.buffer[..block.len()].copy_from_slice(block);
        chat.finalize_block();
        chat.resolve_finalization(cx, &group, name).await;
        return Ok(());
    }

    if chat.selector != crypto::Hash::default() {
        log::warn!(
            "extra messages in block: message_count: {:?} proposed: {:?} us: {:?} them: {:?}",
            message_count,
            proposed.len(),
            cx.local_peer_id,
            origin,
        );

        chat.resolve_finalization(cx, &group, name).await;
        return Err(InvalidBlock(ExtraMessages));
    }

    let retained_len =
        retain_messages(&mut chat.buffer, |msg| !proposed.contains(&crypto::hash::from_slice(msg)))
            .len();
    chat.buffer.drain(..chat.buffer.len() - retained_len);

    log::warn!("outdated block: us: {:?} them: {:?}", cx.local_peer_id, origin);
    chat.number += 1;
    if chat.block_bounds.is_empty() {
        return Ok(());
    }

    chat.block_bounds.clear();

    let mut block_length = chat.buffer.len();
    while block_length > BLOCK_SIZE {
        let mut happened = false;
        for msg in unpack_messages_ref(&chat.buffer) {
            block_length -= msg.len() + 2;
            if block_length < BLOCK_SIZE {
                happened = true;
                break;
            }
        }

        assert!(happened);

        let bound = chat.block_bounds.last().cloned().unwrap_or(0) + block_length;
        chat.block_bounds.push(bound);
        block_length = chat.buffer.len() - bound;
    }

    Ok(())
}

pub async fn fetch_chat_data(cx: crate::Context, name: ChatName, _: ()) -> Result<MinimalChatData> {
    let chat = cx.chats.get(&name).ok_or(ChatError::NotFound)?.clone();
    let chat = chat.read().await;
    Ok(MinimalChatData { number: chat.number, members: chat.members.clone() })
}

pub async fn recover(
    cx: crate::Context,
    name: ChatName,
    group: super::FullReplGroup,
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
        true => &chat.buffer[..chat.buffer.len()],
        false => &chat.finalized[cursor.block as usize % BLOCK_HISTORY],
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

pub struct Chat {
    members: HashMap<Identity, Member>,
    finalized: [[u8; BLOCK_SIZE]; BLOCK_HISTORY],
    number: BlockNumber,
    buffer: ArrayVec<u8, UNFINALIZED_BUFFER_CAP>,
    block_bounds: ArrayVec<usize, MAX_UNFINALIZED_BLOCKS>,
    selector: crypto::Hash,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            members: Default::default(),
            finalized: [[0; BLOCK_SIZE]; BLOCK_HISTORY],
            block_bounds: Default::default(),
            number: 0,
            buffer: Default::default(),
            selector: Default::default(),
        }
    }
}

impl Chat {
    pub fn new(id: Identity, name: ChatName) -> Self {
        Self {
            members: [(id, Member::default())].into(),
            selector: crypto::hash::from_slice(name.as_bytes()),
            ..Default::default()
        }
    }

    pub fn number(&self) -> BlockNumber {
        self.number
    }

    pub async fn resolve_finalization(
        &mut self,
        cx: crate::Context,
        group: &[U256],
        name: ChatName,
    ) {
        while self.buffer.len() >= FINALIZATION_TRIGGER_SIZE {
            assert!(!self.block_bounds.is_empty());
            let Some(selected) = select_finalizer(group, self.selector) else {
                break;
            };

            if selected != cx.local_peer_id {
                cx.init_finalization_timer(selected, name, self.number).await;
                break;
            }

            let block = self.finalize_block();
            cx.repl_rpc_no_resp(name, rpcs::SEND_BLOCK, Reminder(block)).await;
        }
    }

    fn finalize_block(&mut self) -> &[u8] {
        log::error!("finalizing block: {:?}", self.number);
        let block_len =
            if self.buffer.len() > BLOCK_SIZE { self.block_bounds[0] } else { self.buffer.len() };
        let padding = BLOCK_SIZE - block_len - 2;
        let dest = &mut self.finalized[self.number as usize % BLOCK_HISTORY];
        self.selector = crypto::hash::from_slice(&self.buffer[..block_len]);
        dest[..block_len].copy_from_slice(&self.buffer[..block_len]);
        dest[BLOCK_SIZE - 2..].copy_from_slice(&encode_len(padding));
        self.buffer.drain(..block_len);
        let bound = self.block_bounds.pop_at(0).unwrap();
        self.block_bounds.iter_mut().for_each(|b| *b -= bound);
        assert!(self.buffer.len() <= BLOCK_SIZE || !self.block_bounds.is_empty());
        self.number += 1;
        dest[..block_len].as_ref()
    }

    pub fn next_selector(&mut self) {
        self.selector = crypto::hash::from_slice(&self.selector);
    }
}

#[derive(component_utils::Codec)]
pub struct MinimalChatData {
    pub number: BlockNumber,
    pub members: HashMap<Identity, Member>,
}
