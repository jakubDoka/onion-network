use {
    super::MissingTopic,
    chain_api::Nonce,
    chat_spec::{
        retain_messages, rpcs, unpack_messages_ref, BlockNumber, ChatError, ChatEvent, ChatName,
        Cursor, Identity, Member, Message, Permissions, Proof, ReplVec, REPLICATION_FACTOR,
    },
    codec::{Codec, Reminder},
    component_utils::encode_len,
    dht::{try_peer_id_to_ed, U256},
    libp2p::{futures::StreamExt, PeerId},
    std::{
        collections::{btree_map, BTreeMap, HashMap, HashSet, VecDeque},
        ops::DerefMut,
        sync::Arc,
    },
    tokio::sync::{OwnedRwLockWriteGuard, RwLock},
};

const MAX_MESSAGE_SIZE: usize = 1024 - 40;
const MESSAGE_FETCH_LIMIT: usize = 20;
const BLOCK_SIZE: usize = 1024 * 32;
const MAX_UNFINALIZED_BLOCKS: usize = 5;
const UNFINALIZED_BUFFER_CAP: usize = BLOCK_SIZE * MAX_UNFINALIZED_BLOCKS;
const BLOCK_HISTORY: usize = 8;

type Result<T, E = ChatError> = std::result::Result<T, E>;

fn default<T: Default>() -> T {
    T::default()
}

fn advance_nonce(nonce: &mut Nonce, new_nonce: Nonce) -> Result<()> {
    if new_nonce <= *nonce {
        return Err(ChatError::InvalidChatAction(*nonce));
    }
    *nonce = new_nonce;
    Ok(())
}

pub async fn create(
    cx: crate::Context,
    missing_topic: Option<MissingTopic>,
    name: ChatName,
    identity: Identity,
) -> Result<()> {
    if let Some(missing_topic) = missing_topic {
        crate::ensure!(
            let MissingTopic::Chat { mut lock, .. } = missing_topic,
            ChatError::AlreadyExists
        );

        *lock = Chat::new(identity, name);
    } else {
        crate::ensure!(
            let dashmap::mapref::entry::Entry::Vacant(v) = cx.chats.entry(name),
            ChatError::AlreadyExists
        );

        v.insert(Arc::new(RwLock::new(Chat::new(identity, name))));
    }

    Ok(())
}

pub async fn add_member(
    cx: crate::Context,
    (proof, identity, member): (Proof<ChatName>, Identity, Member),
) -> Result<()> {
    let chat = cx.chats.get(&proof.context).ok_or(ChatError::NotFound)?.clone();
    let mut chat = chat.write().await;

    crate::ensure!(proof.verify(), ChatError::InvalidProof);

    let sender_id = crypto::hash::new(proof.pk);
    let is_update = chat.members.contains_key(&identity);
    let sender = chat.members.get_mut(&sender_id).ok_or(ChatError::NotMember)?;
    let rank = sender.rank;

    advance_nonce(&mut sender.action, proof.nonce)?;
    if !is_update {
        sender.allocate_action(Permissions::INVITE)?;
    }

    let member = Member {
        rank: sender.rank.max(member.rank),
        permissions: member.permissions & sender.permissions,
        action_cooldown_ms: sender.action_cooldown_ms.max(member.action_cooldown_ms),
        ..default()
    };

    match chat.members.entry(identity) {
        btree_map::Entry::Occupied(existing)
            if existing.get().rank > rank || identity == sender_id =>
        {
            let existing = existing.into_mut();
            existing.rank = member.rank;
            existing.permissions = member.permissions;
            existing.action_cooldown_ms = member.action_cooldown_ms;
        }
        btree_map::Entry::Vacant(v) => _ = v.insert(member),
        _ => return Err(ChatError::NoPermission),
    }

    cx.push_chat_event(proof.context, ChatEvent::Member(identity, member)).await;

    Ok(())
}

pub async fn kick_member(
    cx: crate::Context,
    (proof, identity): (Proof<ChatName>, Identity),
) -> Result<()> {
    let chat = cx.chats.get(&proof.context).ok_or(ChatError::NotFound)?.clone();
    {
        let mut chat = chat.write().await;

        crate::ensure!(proof.verify(), ChatError::InvalidProof);

        let sender_id = crypto::hash::new(proof.pk);
        let sender = chat.members.get_mut(&sender_id).ok_or(ChatError::NotMember)?;

        advance_nonce(&mut sender.action, proof.nonce)?;
        if identity != sender_id {
            sender.allocate_action(Permissions::KICK)?;
        }
        let rank = sender.rank;

        crate::ensure!(
            let btree_map::Entry::Occupied(entry) = chat.members.entry(identity),
            ChatError::NotFound
        );
        crate::ensure!(identity == sender_id || entry.get().rank > rank, ChatError::NoPermission);

        entry.remove();
    }

    cx.push_chat_event(proof.context, ChatEvent::MemberRemoved(identity)).await;

    Ok(())
}

pub async fn fetch_members(
    cx: crate::Context,
    name: ChatName,
    (identity, count): (Identity, usize),
) -> Result<Vec<(Identity, Member)>> {
    Ok(cx
        .chats
        .get(&name)
        .ok_or(ChatError::NotFound)?
        .read()
        .await
        .members
        .range(identity..)
        .take(count.min(50))
        .map(|(id, member)| (*id, *member))
        .collect::<Vec<_>>())
}

pub async fn send_message(
    cx: crate::Context,
    group: super::FullReplGroup,
    name: ChatName,
    proof: Proof<Reminder<'_>>,
) -> Result<()> {
    let msg = proof.context.0;
    crate::ensure!(msg.len() <= MAX_MESSAGE_SIZE, ChatError::MessageTooLarge);
    crate::ensure!(proof.verify(), ChatError::InvalidProof);

    let chat = cx.chats.get(&name).ok_or(ChatError::NotFound)?.clone();
    let identity = crypto::hash::new(proof.pk);

    {
        let mut chat_guard = chat.write().await;
        let chat = chat_guard.deref_mut();

        let sender = chat.members.get_mut(&identity).ok_or(ChatError::NotMember)?;
        let nonce = sender.action;

        advance_nonce(&mut sender.action, proof.nonce)?;
        sender.allocate_action(Permissions::SEND)?;

        let message = Message { identity, nonce, content: Reminder(msg) };
        let encoded_len = message.encoded_len();

        if chat.buffer.len() + encoded_len + 2 > UNFINALIZED_BUFFER_CAP {
            return Err(ChatError::MessageOverload);
        }

        _ = message.encode(&mut chat.buffer);
        chat.buffer.extend(encode_len(encoded_len));

        if chat.buffer.len() > BLOCK_SIZE
            && chat.buffer.len() % BLOCK_SIZE > BLOCK_SIZE / 2
            && (chat.buffer.len() - encoded_len - 2) % BLOCK_SIZE < BLOCK_SIZE / 2
        {
            chat.resolve_finalization(cx, group, name).await;
        }
    }

    cx.push_chat_event(name, ChatEvent::Message(identity, proof.context)).await;

    Ok(())
}

pub async fn vote(
    cx: crate::Context,
    origin: super::Origin,
    group: super::ReplGroup,
    name: ChatName,
    (hash, bn, agrees): (crypto::Hash, BlockNumber, bool),
) -> Result<()> {
    if !group.contains(&origin) {
        return Err(ChatError::NoReplicator);
    }

    let compressed =
        dht::try_peer_id_to_ed(origin).map(U256::from).ok_or(ChatError::NoReplicator)?;
    let chat = cx.chats.get(&name).ok_or(ChatError::NotFound)?.clone();
    let mut chat = chat.write().await;
    let chat = chat.deref_mut();

    if bn < chat.number {
        return Ok(());
    }

    let found = chat.votes.iter_mut().enumerate().find(|(_, vote)| vote.block.hash == hash);
    let (i, vote) = match found {
        Some(vote) => vote,
        None => {
            if chat.votes.iter().filter(|vote| vote.votes.contains(&compressed)).count() > 4 {
                return Err(ChatError::VoteNotFound);
            }
            chat.votes.push_back(BlockVote {
                number: bn,
                block: Block { hash, data: vec![] },
                ..default()
            });
            (chat.votes.len(), chat.votes.back_mut().unwrap())
        }
    };

    if vote.votes.contains(&compressed) {
        return Err(ChatError::AlreadyVoted);
    }

    let number = vote.number;
    vote.votes.push(compressed);
    let decision = vote.vote(agrees as usize, !agrees as usize);
    chat.process_vote_decision(cx.local_peer_id, i, number, decision);

    Ok(())
}

pub async fn handle_message_block(
    cx: crate::Context,
    origin: super::Origin,
    group: super::FullReplGroup,
    name: ChatName,
    (number, base_hash, Reminder(block)): (BlockNumber, crypto::Hash, Reminder<'_>),
) -> Result<()> {
    use {chat_spec::InvalidBlockReason::*, ChatError::*};

    async fn apply_vote(chat: &mut Chat, cx: crate::Context, name: ChatName, vote: BlockVote) {
        cx.repl_rpc_no_resp(name, rpcs::VOTE_BLOCK, (vote.block.hash, vote.number, vote.no == 0))
            .await;
        if let Some((i, v)) =
            chat.votes.iter_mut().enumerate().find(|(_, v)| v.block.hash == vote.block.hash)
        {
            v.block = vote.block;
            let decision = v.vote(vote.yes, vote.no);
            chat.process_vote_decision(cx.local_peer_id, i, vote.number, decision);
        } else {
            chat.votes.push_back(vote);
        }
    }

    let compressed_origin = dht::try_peer_id_to_ed(origin).map(U256::from).ok_or(NoReplicator)?;
    let chat = cx.chats.get(&name).ok_or(NotFound)?.clone();

    let block = Block::from(block);
    let vote = BlockVote {
        number,
        block,
        yes: 1,
        votes: [compressed_origin].into_iter().collect(),
        ..default()
    };

    let mut chat = chat.write().await;
    let chat = chat.deref_mut();

    if number < chat.number {
        return Err(ChatError::Outdated);
    }

    let latest_hash = chat.get_latest_hash(number).unwrap_or_default();
    if base_hash != latest_hash {
        log::warn!("block not based on latest hash: us:\ne: {:?}\ng: {:?}", latest_hash, base_hash,);
        apply_vote(chat, cx, name, BlockVote { no: 1, ..vote }).await;
        return Err(ChatError::Outdated);
    }

    'a: {
        let finalizer = chat.select_finalizer(group.clone(), number, vote.block.data.len());
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
        apply_vote(chat, cx, name, BlockVote { no: 1, ..vote }).await;
        return Err(InvalidBlock(NotExpected));
    }

    let proposed = unpack_messages_ref(&vote.block.data).collect::<HashSet<_>>();
    let message_count =
        unpack_messages_ref(&chat.buffer).filter(|msg| proposed.contains(msg)).count();

    if message_count != proposed.len() {
        log::warn!(
            "extra messages in block: message_count: {:?} proposed: {:?} us: {} them: {}",
            message_count,
            proposed.len(),
            cx.local_peer_id,
            origin,
        );
        apply_vote(chat, cx, name, BlockVote { no: 1, ..vote }).await;
        return Err(InvalidBlock(ExtraMessages));
    }

    apply_vote(chat, cx, name, BlockVote { yes: 2, ..vote }).await;
    Ok(())
}

pub async fn fetch_chat_data(cx: crate::Context, name: ChatName, _: ()) -> Result<MinimalChatData> {
    let chat = cx.chats.get(&name).ok_or(ChatError::NotFound)?.clone();
    let chat = chat.read().await;
    Ok(MinimalChatData { number: chat.number, members: chat.members.clone() })
}

pub async fn fetch_messages(
    cx: crate::Context,
    name: ChatName,
    mut cursor: Cursor,
) -> Result<(Cursor, Vec<u8>)> {
    let chat = cx.chats.get(&name).ok_or(ChatError::NotFound)?.clone();
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
        cursor.block -= 1;
    }

    if cursor.offset > UNFINALIZED_BUFFER_CAP {
        return bail;
    }

    let block = loop {
        let block = match chat.number <= cursor.block {
            true => &chat.buffer,
            false => &chat.finalized[cursor.block as usize % BLOCK_HISTORY].data,
        };
        if cursor.offset <= block.len() {
            break block;
        }
        cursor.block += 1;
        cursor.offset -= block.len();
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
    ) -> BTreeMap<Identity, Member> {
        let mut member_count_map = HashMap::<Identity, ReplVec<Member>>::new();
        for (id, other) in block_data.iter().flat_map(|(_, data)| &data.members) {
            member_count_map.entry(*id).or_default().push(*other);
        }

        member_count_map.values_mut().for_each(|v| retain_outliers(v, |_| 10, |m| m.action as _));
        member_count_map.retain(|_, v| v.len() >= REPLICATION_FACTOR.get() / 2);

        member_count_map.into_iter().map(|(id, mut v)| (id, Member::combine(&mut v))).collect()
    }

    retain_outliers(block_data, |m| (m / 10).clamp(1, 3), |(_, data)| data.members.len());

    let members = retain_members(block_data);
    let number = block_data.first().map(|(_, data)| data.number).unwrap_or(0);
    MinimalChatData { number, members }
}

#[derive(Default)]
pub struct Block {
    pub hash: crypto::Hash,
    pub data: Vec<u8>,
}

impl From<&[u8]> for Block {
    fn from(data: &[u8]) -> Self {
        Self { hash: crypto::hash::new(data), data: data.to_vec() }
    }
}

#[derive(Default)]
pub struct BlockVote {
    pub number: BlockNumber,
    pub yes: usize,
    pub no: usize,
    pub block: Block,
    pub votes: ReplVec<U256>,
}

impl BlockVote {
    fn vote(&mut self, yes: usize, no: usize) -> Option<Option<Block>> {
        self.yes += yes;
        if self.yes > REPLICATION_FACTOR.get() / 2 && !self.block.data.is_empty() {
            return Some(Some(std::mem::take(&mut self.block)));
        }
        self.no += no;
        if self.no > REPLICATION_FACTOR.get() / 2 {
            return Some(None);
        }
        None
    }
}

#[derive(Default)]
pub struct Chat {
    pub members: BTreeMap<Identity, Member>,
    pub finalized: VecDeque<Block>,
    pub number: BlockNumber,
    pub buffer: Vec<u8>,
    pub votes: VecDeque<BlockVote>,
}

impl Chat {
    pub fn new(id: Identity, _: ChatName) -> Self {
        Self { members: [(id, Member::best())].into(), ..Default::default() }
    }

    pub async fn resolve_finalization(
        &mut self,
        cx: crate::Context,
        group: super::FullReplGroup,
        name: ChatName,
    ) {
        let selected =
            self.select_finalizer(group.clone(), self.number, self.buffer.len() - BLOCK_SIZE);
        let us = try_peer_id_to_ed(cx.local_peer_id).map(U256::from).expect("we can trust us");
        if selected != us {
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
            (
                self.number,
                self.get_latest_hash(self.number).unwrap_or_default(),
                Reminder(&self.buffer[..actual_block_size]),
            ),
        )
        .await;

        self.votes.push_back(BlockVote {
            number: self.number,
            yes: 1,
            no: 0,
            block: Block::from(&self.buffer[..actual_block_size]),
            votes: ReplVec::new(),
        });
    }

    pub fn finalize_block(&mut self, _us: PeerId, number: BlockNumber, block: Block) {
        let mapping = unpack_messages_ref(&block.data).collect::<HashSet<_>>();

        let mut new_len = 0;
        let mut delete_shapshot = None;
        retain_messages(&mut self.buffer, |msg| {
            let keep = !mapping.contains(msg);
            new_len += (msg.len() + 2) * keep as usize;
            if !keep {
                delete_shapshot = Some(new_len);
            }
            keep
        });

        let len = self.buffer.len();
        if let Some(delete_shapshot) = delete_shapshot {
            self.buffer.drain(..len - delete_shapshot);
        } else {
            self.buffer.drain(..len - new_len);
        }

        self.finalized.push_back(block);
        if self.finalized.len() > BLOCK_HISTORY {
            self.finalized.pop_front();
        }
        self.number = number + 1;
    }

    pub fn select_finalizer(
        &self,
        members: super::FullReplGroup,
        number: BlockNumber,
        buffer_len: usize,
    ) -> U256 {
        let Some(last_block) = self.get_latest_hash(number) else {
            return select_finalizer(members, crypto::Hash::default(), 0);
        };

        select_finalizer(members, last_block, buffer_len)
    }

    pub fn get_latest_hash(&self, number: BlockNumber) -> Option<crypto::Hash> {
        self.votes
            .iter()
            .rfind(|v| {
                v.yes > REPLICATION_FACTOR.get() / 2 && v.number.checked_sub(1) == Some(number)
            })
            .map(|v| &v.block)
            .or(self.finalized.back())
            .map(|b| b.hash)
    }

    fn process_vote_decision(
        &mut self,
        us: PeerId,
        i: usize,
        number: BlockNumber,
        decision: Option<Option<Block>>,
    ) {
        match decision {
            Some(Some(block)) => {
                self.finalize_block(us, number, block);
                self.votes.drain(..i + 1);
            }
            Some(None) => _ = self.votes.remove(i),
            None => {}
        }
    }
}

pub fn select_finalizer(
    mut members: super::FullReplGroup,
    selector: crypto::Hash,
    block_len: usize,
) -> U256 {
    let selector = U256::from(selector);
    let round_count = block_len / BLOCK_SIZE;

    assert!(round_count < MAX_UNFINALIZED_BLOCKS, "block_len: {}", block_len);
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

#[derive(Codec)]
pub struct MinimalChatData {
    pub number: BlockNumber,
    pub members: BTreeMap<Identity, Member>,
}
