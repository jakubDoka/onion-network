use {
    chat_spec::{
        advance_nonce, retain_messages_in_vec, rpcs, unpack_messages_ref, BlockNumber, ChatError,
        ChatEvent, ChatName, Cursor, Identity, Member, Message, Proof, ReplVec, REPLICATION_FACTOR,
    },
    component_utils::{encode_len, Buffer, Codec, NoCapOverflow, Reminder},
    dashmap::mapref::entry::Entry,
    libp2p::{futures::StreamExt, PeerId},
    std::{
        collections::{HashMap, HashSet, VecDeque},
        mem::ManuallyDrop,
        ops::DerefMut,
        sync::Arc,
    },
    tokio::sync::RwLock,
};

const MAX_MESSAGE_SIZE: usize = 1024;
const MESSAGE_FETCH_LIMIT: usize = 20;
const BLOCK_SIZE: usize = if cfg!(test) { 1024 * 4 } else { 1024 * 128 };
const BLOCK_HISTORY: usize = 8;

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub async fn create(cx: crate::Context, (name, identity): (ChatName, Identity)) -> Result<()> {
    let chat_entry = cx.chats.entry(name);
    crate::ensure!(
        let dashmap::mapref::entry::Entry::Vacant(entry) = chat_entry,
        ChatError::AlreadyExists
    );

    entry.insert(Arc::new(RwLock::new(Chat::new(identity))));

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
    (proof, Reminder(msg)): (Proof<ChatName>, Reminder<'_>),
) -> Result<()> {
    let chat = cx.chats.get(&proof.context).ok_or(ChatError::NotFound)?.clone();
    let mut chat = chat.write().await;

    crate::ensure!(proof.verify(), ChatError::InvalidProof);

    let sender_id = crypto::hash::from_raw(&proof.pk);
    let bn = chat.number;
    let sender = chat.members.get_mut(&sender_id).ok_or(ChatError::NotMember)?;

    crate::ensure!(
        advance_nonce(&mut sender.action, proof.nonce),
        ChatError::InvalidChatAction(sender.action)
    );

    crate::ensure!(msg.len() <= MAX_MESSAGE_SIZE, ChatError::MessageTooLarge);

    let message = Message { identiy: sender_id, nonce: sender.action - 1, content: Reminder(msg) };
    let push_res = { chat }.push_message(message, &mut vec![]);
    match push_res {
        Err(Some(hash)) => {
            cx.repl_rpc_no_resp(proof.context, rpcs::BLOCK_PROPOSAL, (proof.context, bn, hash))
                .await;
        }
        Err(None) => return Err(ChatError::MessageBlockNotFinalized),
        Ok(()) => (),
    }

    cx.push_chat_event(proof.context, ChatEvent::Message(proof, Reminder(msg))).await;

    Ok(())
}

pub async fn propose_msg_block(
    cx: crate::Context,
    origin: super::Origin,
    repl: super::ReplGroup,
    (name, number, phash): (ChatName, BlockNumber, crypto::Hash),
) {
    let Some(index) = repl.iter().position(|id| *id == origin) else {
        return;
    };

    let Some(chat) = cx.chats.get_mut(&name).map(|c| c.value().clone()) else {
        return;
    };

    'a: {
        let chat = chat.read().await;
        let our_finalized = chat.last_finalized_block();

        if number >= our_finalized {
            break 'a;
        }

        let block_index = our_finalized - number - 1;

        if let Some(block) = chat.finalized.get(block_index as usize)
            && block.hash == phash
        {
            return;
        };

        let (block, number) = chat
            .finalized
            .get(block_index as usize)
            .or(chat.finalized.back())
            .map(|b| (b.data.as_ref(), number))
            .unwrap_or_else(|| (chat.current_block.as_ref(), our_finalized));

        cx.send_rpc_no_resp(name, origin, rpcs::MAJOR_BLOCK, (number, Reminder(block))).await;

        return;
    }

    let mut chat = chat.write().await;
    let chat = chat.deref_mut();

    let BlockStage::Unfinalized { others } = &mut chat.stage else {
        return;
    };

    let we_match = chat.proposed_block.as_ref().map(|p| p.hash) == Some(phash);

    debug_assert!(others[index] == crypto::Hash::default());
    others[index] = phash;

    if others.iter().filter(|&&h| h == phash).count()
        <= REPLICATION_FACTOR.get() / 2 - usize::from(we_match)
    {
        if !others.contains(&Default::default()) {
            if let Some(block) = &chat.proposed_block {
                cx.repl_rpc_no_resp(name, rpcs::MAJOR_BLOCK, (number, Reminder(&block.data))).await;
            }
            chat.stage = BlockStage::MajorMismatch { blocks: Default::default() };
        }
        return;
    }

    let Some(block) = chat.proposed_block.take() else {
        chat.stage = BlockStage::MinorMismatch { final_hash: phash };
        return;
    };

    if block.hash != phash {
        chat.stage = BlockStage::MinorMismatch { final_hash: phash };
        chat.proposed_block = Some(block);
        return;
    }

    for (&peer, _) in repl
        .iter()
        .zip(others.iter())
        .filter(|(_, hash)| **hash != phash && **hash != crypto::Hash::default())
    {
        cx.send_rpc_no_resp(name, peer, rpcs::MAJOR_BLOCK, (number, Reminder(&block.data))).await;
    }

    chat.push_to_finalized(block);
    chat.stage = BlockStage::default()
}

pub async fn receive_block(
    cx: crate::Context,
    origin: super::Origin,
    chat: ChatName,
    req: (BlockNumber, Reminder<'_>),
) {
    if let Err(e) = receive_block_low(cx, origin, chat, req).await {
        log::warn!("error while receiving block: {:?}", e);
    }
}

pub async fn receive_block_low(
    cx: crate::Context,
    origin: super::Origin,
    name: ChatName,
    (number, Reminder(block)): (BlockNumber, Reminder<'_>),
) -> Result<()> {
    use {chat_spec::InvalidBlockReason::*, ChatError::*};

    let chat = cx.chats.get(&name).ok_or(NotFound)?.clone();
    let mut chat = chat.write().await;
    let chat = chat.deref_mut();

    crate::ensure!(chat.last_finalized_block() == number, InvalidBlock(Outdated));

    match &mut chat.stage {
        BlockStage::Unfinalized { .. } if let Some(proposed) = &chat.proposed_block => {
            // TODO: attacker could trigger useless recovery

            cx.repl_rpc_no_resp(name, rpcs::MAJOR_BLOCK, (number, Reminder(&proposed.data))).await;
            chat.stage = BlockStage::MajorMismatch { blocks: Default::default() };
            return Ok(());
        }
        BlockStage::Unfinalized { .. } => todo!(), //return Err(InvalidBlock(NotExpected)),
        &mut BlockStage::MinorMismatch { final_hash } => {
            let hash_temp = &mut vec![];
            let hash = Chat::hash_block(block, hash_temp);
            crate::ensure!(hash == final_hash, InvalidBlock(MajorityMismatch));

            retain_messages_in_vec(&mut chat.current_block, |msg| {
                // this is fine since message contains sender id and nonce which is
                // unique for each messsage
                !hash_temp.contains(&crypto::hash::from_slice(msg))
            });

            if let Some(block) = chat.proposed_block.take() {
                for message in unpack_messages_ref(&block.data) {
                    if !hash_temp.contains(&crypto::hash::from_slice(message)) {
                        chat.current_block.extend_from_slice(message);
                        chat.current_block.extend_from_slice(&encode_len(message.len()));
                    }
                }
            }

            log::warn!("minor mismatch resolved");

            chat.push_to_finalized(Block { hash, data: block.into() });
        }
        BlockStage::MajorMismatch { blocks } => {
            crate::ensure!(blocks.iter().all(|&(p, _)| p != origin), InvalidBlock(NotExpected));

            let hash_temp = &mut vec![];
            let hash = Chat::hash_block(block, hash_temp);
            let block = Block { hash, data: block.into() };
            blocks.push((origin, block));

            // TODO: we need to prevent nodes from witholding blocks, timeout should be enough
            if blocks.len() != REPLICATION_FACTOR.get() {
                return Ok(());
            }

            let Some(mut proposed) = chat.proposed_block.take() else {
                return Ok(());
            };

            merge_blocks(
                blocks.iter().map(|(_, b)| b.data.as_slice()),
                &mut proposed.data,
                &mut chat.current_block,
            );

            log::warn!("major mismatch resolved");

            chat.push_to_finalized(proposed);
        }
    }

    chat.stage = BlockStage::default();

    Ok(())
}

fn merge_blocks<'a>(
    blocks: impl Iterator<Item = &'a [u8]>,
    proposed: &mut Vec<u8>,
    current: &mut Vec<u8>,
) {
    let mut message_hash_counters = HashMap::<crypto::Hash, usize>::new();
    let mut count = |messages: &[u8]| {
        for message in unpack_messages_ref(messages) {
            let hash = crypto::hash::from_slice(message);
            *message_hash_counters.entry(hash).or_default() += 1;
        }
    };

    blocks.for_each(&mut count);
    count(proposed);
    count(current.as_slice());
    message_hash_counters.retain(|_, v| *v > REPLICATION_FACTOR.get() / 2);

    let mut retain = |to_retain: &mut Vec<u8>, to_append: &mut Vec<u8>, inverted: bool| {
        retain_messages_in_vec(to_retain, |msg| {
            let valid = message_hash_counters.remove(&crypto::hash::from_slice(msg)).is_some();
            if !valid ^ inverted {
                to_append.extend_from_slice(msg);
                to_append.extend_from_slice(&encode_len(msg.len()));
            }
            valid
        });
    };

    // we cannot say much about messages in next block so backshifting messages from next
    // block is out of the question
    let prev_len = current.len();
    retain(proposed, current, false);
    current.rotate_left(prev_len);
    retain(current, proposed, true);
}

#[derive(component_utils::Codec)]
pub struct MinimalChatData {
    pub number: BlockNumber,
    pub members: HashMap<Identity, Member>,
    pub current_block: Vec<u8>,
    #[codec(skip)]
    hash: crypto::Hash,
}

pub async fn fetch_minimal_chat_data(
    cx: crate::Context,
    name: ChatName,
) -> Result<MinimalChatData> {
    let chat = cx.chats.get(&name).ok_or(ChatError::NotFound)?;
    let chat = chat.read().await;

    Ok(MinimalChatData {
        number: chat.number,
        members: chat.members.clone(),
        current_block: chat.current_block.clone(),
        hash: Default::default(),
    })
}

pub async fn fetch_messages(
    cx: crate::Context,
    (chat, mut cursor): (ChatName, Cursor),
) -> Result<(Cursor, Vec<u8>)> {
    let chat = cx.chats.get(&chat).ok_or(ChatError::NotFound)?.clone();
    let chat = chat.read().await;

    if cursor == Cursor::INIT {
        cursor.block = chat.number;
        cursor.offset = chat.current_block.len();
    }

    let bail = Ok((Cursor::INIT, vec![]));

    if cursor.offset == 0 {
        if cursor.block == 0 {
            return bail;
        }
        cursor.block += 1;
    }

    let block = match chat.finalized.get(chat.number.saturating_sub(cursor.block) as usize) {
        Some(block) => block.data.as_slice(),
        None => return bail,
    };

    if cursor.offset == 0 {
        cursor.offset = block.len();
    }

    let slice = &block[cursor.offset - MESSAGE_FETCH_LIMIT..cursor.offset];
    cursor.offset -= slice.len();

    Ok((cursor, slice.to_vec()))
}

#[derive(component_utils::Codec, Default)]
struct Block {
    hash: crypto::Hash,
    data: Vec<u8>,
}

enum BlockStage {
    Unfinalized { others: [crypto::Hash; REPLICATION_FACTOR.get()] },
    MinorMismatch { final_hash: crypto::Hash },
    MajorMismatch { blocks: Box<ReplVec<(PeerId, Block)>> },
}

impl Default for BlockStage {
    fn default() -> Self {
        Self::Unfinalized { others: Default::default() }
    }
}

#[derive(Default)]
pub struct Chat {
    members: HashMap<Identity, Member>,
    finalized: VecDeque<Block>,
    current_block: Vec<u8>,
    proposed_block: Option<Block>,
    pub(crate) number: BlockNumber,
    stage: BlockStage,
}

impl Chat {
    pub fn new(id: Identity) -> Self {
        Self {
            members: [(id, Member::default())].into(),
            current_block: Vec::with_capacity(BLOCK_SIZE),
            ..Default::default()
        }
    }

    pub fn push_message<'a>(
        &mut self,
        msg: impl component_utils::Codec<'a>,
        hash_temp: &mut Vec<crypto::Hash>,
    ) -> Result<(), Option<crypto::Hash>> {
        let prev_len = self.current_block.len();

        fn try_push<'a>(block: &mut Vec<u8>, msg: impl component_utils::Codec<'a>) -> Option<()> {
            let len = block.len();
            let buffer = NoCapOverflow::new(block);
            msg.encode(buffer)?;
            let len = buffer.as_mut().len() - len;
            buffer.extend_from_slice(&encode_len(len))
        }

        if try_push(&mut self.current_block, &msg).is_some() {
            return Ok(());
        }

        self.current_block.truncate(prev_len);

        if self.proposed_block.is_some() {
            return Err(None);
        }

        let hash = Self::hash_block(self.current_block.as_slice(), hash_temp);
        let proposed = Block { hash, data: self.current_block.as_slice().into() };
        self.current_block.clear();
        self.number += 1;

        let can_finalize = match &self.stage {
            BlockStage::Unfinalized { others } => {
                others.iter().filter(|h| **h == hash).count() > REPLICATION_FACTOR.get() / 2
            }
            _ => false,
        };

        if can_finalize {
            self.push_to_finalized(proposed);
        } else {
            self.proposed_block = Some(proposed);
        }

        try_push(&mut self.current_block, msg).expect("we checked size limits");

        Err(Some(hash))
    }

    fn push_to_finalized(&mut self, block: Block) {
        if self.finalized.len() == BLOCK_HISTORY {
            self.finalized.pop_back();
        }
        self.finalized.push_front(block);
    }

    fn hash_block(block: &[u8], hash_temp: &mut Vec<crypto::Hash>) -> crypto::Hash {
        hash_temp.clear();
        unpack_messages_ref(block).map(crypto::hash::from_slice).collect_into(hash_temp);
        hash_temp.sort_unstable();
        hash_temp.iter().copied().reduce(crypto::hash::combine).expect("we checked size limits")
    }

    fn last_finalized_block(&self) -> BlockNumber {
        self.number - self.proposed_block.is_some() as BlockNumber
    }
}

pub fn _defer<F>(f: F) -> impl Drop
where
    F: FnOnce(),
{
    struct Defer<F: FnOnce()>(ManuallyDrop<F>);

    impl<F: FnOnce()> Drop for Defer<F> {
        fn drop(&mut self) {
            let f: F = unsafe { ManuallyDrop::take(&mut self.0) };
            f();
        }
    }

    Defer(ManuallyDrop::new(f))
}

pub async fn recover(cx: crate::Context, name: ChatName) -> Result<()> {
    let mut repl_chat_data = cx
        .replicate_rpc(name, rpcs::FETCH_MINIMAL_CHAT_DATA, name)
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

    let Entry::Vacant(entry) = cx.chats.entry(name) else {
        return Ok(());
    };

    if repl_chat_data.len() <= REPLICATION_FACTOR.get() / 2 {
        log::warn!("not enough data to recover chat: {:?}", name);
        return Err(ChatError::NotFound);
    }

    let mut chat_data = reconstruct_chat(&mut repl_chat_data)?;

    let mut chat =
        Chat { members: chat_data.members, number: chat_data.number, ..Default::default() };
    chat.current_block.reserve(BLOCK_SIZE);
    chat.current_block.append(&mut chat_data.current_block);
    entry.insert(Arc::new(RwLock::new(chat)));

    Ok(())
}

fn reconstruct_chat(
    block_data: &mut ReplVec<(PeerId, MinimalChatData)>,
) -> Result<MinimalChatData> {
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
        let mut member_counte_map = HashMap::<Identity, ReplVec<Member>>::new();
        for (id, other) in block_data.iter().flat_map(|(_, data)| &data.members) {
            member_counte_map.entry(*id).or_default().push(*other);
        }
        member_counte_map
            .values_mut()
            .for_each(|v| retain_outliers(v, |m| m.clamp(3, 10), |m| m.action as _));
        member_counte_map.retain(|_, v| v.len() >= REPLICATION_FACTOR.get() / 2);

        member_counte_map
            .into_iter()
            .map(|(id, v)| (id, v.into_iter().reduce(Member::max).unwrap()))
            .collect()
    }

    fn compute_message_hashes<'a>(
        buffer: &'a mut Vec<crypto::Hash>,
        blocks: &[(PeerId, MinimalChatData)],
    ) -> ReplVec<&'a mut [crypto::Hash]> {
        let mut ranges = ReplVec::new();
        for (_, chat) in blocks {
            let prev_len = buffer.len();
            buffer.extend(unpack_messages_ref(&chat.current_block).map(crypto::hash::from_slice));
            ranges.push(buffer.len() - prev_len);
        }
        ranges.into_iter().scan(&mut buffer[..], |buffer, len| buffer.take_mut(..len)).collect()
    }

    retain_outliers(block_data, |_| 0, |(_, data)| data.number as _);
    retain_outliers(block_data, |m| m.clamp(1000, 3000), |(_, data)| data.current_block.len());
    retain_outliers(block_data, |m| m.clamp(1, 3), |(_, data)| data.members.len());

    let members = retain_members(block_data);

    let mut hash_buffer = vec![];
    let mut hashes = compute_message_hashes(&mut hash_buffer, block_data);

    hashes.iter_mut().for_each(|h| h.sort_unstable());

    hashes
        .iter()
        .map(|h| h.iter().copied().reduce(crypto::hash::combine).unwrap_or_default())
        .zip(block_data.iter_mut())
        .for_each(|(hash, (_, data))| data.hash = hash);
    block_data.sort_unstable_by_key(|(_, data)| data.hash);

    let best_hash = block_data
        .as_slice()
        .chunk_by(|(_, a), (_, b)| a.hash == b.hash)
        .max_by_key(|slc| slc.len())
        .filter(|slc| slc.len() > REPLICATION_FACTOR.get() / 2)
        .map(|slc| slc[0].1.hash);

    if let Some(best_hash) = best_hash {
        block_data.retain(|(_, data)| data.hash == best_hash);
        return Ok(MinimalChatData { members, ..block_data.pop().unwrap().1 });
    }

    let mut proposed = {
        let mut seed = HashSet::<Identity>::new();
        let mut all = vec![];

        let mut iters = block_data
            .iter_mut()
            .map(|(_, data)| unpack_messages_ref(&data.current_block))
            .collect::<ReplVec<_>>();
        let mut current = (0..iters.len()).cycle();
        let column_iter = std::iter::from_fn(|| {
            let mut exhausted = 0;
            while exhausted != iters.len() {
                let i = current.next().unwrap();
                let iter = &mut iters[i];
                if let Some(msg) = iter.next() {
                    return Some(msg);
                }
                exhausted += 1;
            }

            None
        });

        for msg in column_iter {
            let id = crypto::hash::from_slice(msg);
            if seed.insert(id) {
                all.extend_from_slice(msg);
                all.extend_from_slice(&encode_len(msg.len()));
            }
        }

        all
    };

    merge_blocks(
        block_data.iter().map(|(_, data)| data.current_block.as_slice()),
        &mut proposed,
        &mut vec![],
    );

    Err(ChatError::NotFound)
}
