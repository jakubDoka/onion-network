use {
    chain_api::NodeIdentity,
    chat_spec::{
        rpcs, BlockNumber, ChatError, ChatEvent, ChatName, Cursor, Identity, Member, ReplVec,
        REPLICATION_FACTOR,
    },
    codec::ReminderOwned,
    crypto::proof::Proof,
    handlers::Dec,
    opfusk::PeerIdExt,
    std::collections::HashMap,
    tokio::task::block_in_place,
};

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub async fn create(
    cx: crate::Context,
    name: ChatName,
    Dec(identity): Dec<Identity>,
) -> Result<()> {
    block_in_place(|| cx.storage.create_chat(name, identity))?;
    cx.not_found.remove(&name.into());
    Ok(())
}

pub async fn add_member(
    cx: crate::Context,
    Dec((proof, identity, member)): Dec<(Proof<ChatName>, Identity, Member)>,
) -> Result<()> {
    block_in_place(|| cx.storage.get_chat(proof.context)?.add_member(proof, identity, member))?;
    cx.push_chat_event(proof.context, ChatEvent::Member(identity, member)).await;
    Ok(())
}

pub async fn kick_member(
    cx: crate::Context,
    Dec((proof, identity)): Dec<(Proof<ChatName>, Identity)>,
) -> Result<()> {
    block_in_place(|| cx.storage.get_chat(proof.context)?.kick_member(proof, identity))?;
    cx.push_chat_event(proof.context, ChatEvent::MemberRemoved(identity)).await;
    Ok(())
}

pub async fn fetch_members(
    cx: crate::Context,
    name: ChatName,
    Dec((identity, count)): Dec<(Identity, usize)>,
) -> Result<Vec<(Identity, Member)>> {
    block_in_place(|| cx.storage.get_chat(name)?.fetch_members(identity, count))
}

pub async fn send_message(
    cx: crate::Context,
    group: super::FullReplGroup,
    name: ChatName,
    Dec(proof): Dec<Proof<ReminderOwned>>,
) -> Result<()> {
    let finalization =
        block_in_place(|| cx.storage.get_chat(name)?.send_message(cx, group, &proof))?;
    if let Some(req) = finalization {
        _ = cx.repl_rpc::<()>(name, rpcs::SEND_BLOCK, req).await;
    }
    cx.push_chat_event(name, ChatEvent::Message(proof.identity(), proof.context)).await;
    Ok(())
}

pub async fn vote(
    cx: crate::Context,
    origin: super::Origin,
    group: super::ReplGroup,
    name: ChatName,
    Dec((hash, bn, agrees)): Dec<(crypto::Hash, BlockNumber, bool)>,
) -> Result<()> {
    origin.try_to_hash().filter(|c| group.contains(c)).ok_or(ChatError::NoReplicator)?;
    block_in_place(|| cx.storage.get_chat(name)?.vote(cx.local_peer_id, bn, hash, agrees))
}

pub async fn proposal(
    cx: crate::Context,
    origin: super::Origin,
    group: super::FullReplGroup,
    name: ChatName,
    Dec((number, ReminderOwned(block))): Dec<(BlockNumber, ReminderOwned)>,
) -> Result<()> {
    // FIXME: make this check when processing request header
    let origin =
        origin.try_to_hash().filter(|c| group.contains(c)).ok_or(ChatError::NoReplicator)?;
    let vote =
        block_in_place(|| cx.storage.get_chat(name)?.proposal(origin, group, number, block))?;

    if let Some((base_hash, vote, error)) = vote {
        _ = cx.repl_rpc::<()>(name, rpcs::VOTE_BLOCK, (number, base_hash, vote)).await;
        if let Some(error) = error {
            return Err(error);
        }
    }

    Ok(())
}

pub async fn fetch_chat_data(
    cx: crate::Context,
    name: ChatName,
    _: (),
) -> Result<Vec<(Identity, Member)>> {
    // FIXME: stream the response
    block_in_place(|| cx.storage.get_chat(name)?.export_data())
}

pub async fn fetch_messages(
    cx: crate::Context,
    name: ChatName,
    Dec(cursor): Dec<Cursor>,
) -> Result<([u8; chat_spec::MAX_MESSAGE_FETCH_SIZE], Cursor)> {
    block_in_place(|| cx.storage.get_chat(name)?.fetch_messages(cursor))

    // let chat = cx.chats.get(&name).ok_or(ChatError::NotFound)?.clone();
    // let chat = chat.read().await;

    // if cursor == Cursor::INIT {
    //     cursor.block = chat.number + 1;
    //     cursor.offset = chat.buffer.len();
    // }

    // let bail = Ok((Cursor::INIT, vec![]));

    // if cursor.offset == 0 {
    //     if cursor.block == 0 {
    //         return bail;
    //     }
    //     cursor.block -= 1;
    // }

    // if cursor.offset > UNFINALIZED_BUFFER_CAP {
    //     return bail;
    // }

    // let block = loop {
    //     let block = match chat.number <= cursor.block {
    //         true => &chat.buffer,
    //         false => &chat.finalized[cursor.block as usize % BLOCK_HISTORY].data,
    //     };
    //     if cursor.offset <= block.len() {
    //         break block;
    //     }
    //     cursor.block += 1;
    //     cursor.offset -= block.len();
    // };

    // if cursor.offset == 0 {
    //     cursor.offset = block.len();
    // }

    // let Some(slice) = block.get(..cursor.offset) else { return bail };
    // let len = unpack_messages_ref(slice)
    //     .take(MESSAGE_FETCH_LIMIT)
    //     .map(|msg| msg.len() + 2)
    //     .sum::<usize>();
    // cursor.offset -= len;

    // Ok((cursor, slice[cursor.offset..].to_vec()))
}

pub async fn recover(cx: crate::Context, name: ChatName) -> Result<()> {
    let mut repl_chat_data = cx
        .repl_rpc::<Result<HashMap<Identity, Member>>>(name, rpcs::FETCH_CHAT_DATA, ())
        .await?
        .into_iter()
        .filter_map(|(p, s)| s.ok().map(|s| (p, s)))
        .collect::<ReplVec<_>>();

    if repl_chat_data.len() <= REPLICATION_FACTOR.get() / 2 {
        log::warn!("not enough data to recover chat: {:?}", name);
        cx.storage.remove_chat(name)?;
        return Err(ChatError::NotFound);
    }

    let chat_data = reconstruct_chat(&mut repl_chat_data);
    cx.storage.recover_chat(name, chat_data).map_err(Into::into)
}

fn reconstruct_chat(
    block_data: &mut ReplVec<(NodeIdentity, HashMap<Identity, Member>)>,
) -> HashMap<Identity, Member> {
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
        block_data: &mut ReplVec<(NodeIdentity, HashMap<Identity, Member>)>,
    ) -> HashMap<Identity, Member> {
        let mut member_count_map = HashMap::<Identity, ReplVec<Member>>::new();
        for (id, other) in block_data.iter().flat_map(|(_, data)| data) {
            member_count_map.entry(*id).or_default().push(*other);
        }

        member_count_map.values_mut().for_each(|v| retain_outliers(v, |_| 10, |m| m.action as _));
        member_count_map.retain(|_, v| v.len() >= REPLICATION_FACTOR.get() / 2);

        member_count_map.into_iter().map(|(id, mut v)| (id, Member::combine(&mut v))).collect()
    }

    retain_outliers(block_data, |m| (m / 10).clamp(1, 3), |(_, data)| data.len());

    retain_members(block_data)
}

//#[derive(Default)]
//pub struct Block {
//    pub hash: crypto::Hash,
//    pub data: Vec<u8>,
//}
//
//impl Block {
//    pub fn new(data: Vec<u8>, prev_hash: crypto::Hash) -> Self {
//        Self { hash: crypto::hash::combine(crypto::hash::new(&data), prev_hash), data }
//    }
//}
//
//#[derive(Default)]
//pub struct BlockVote {
//    pub number: BlockNumber,
//    pub yes: usize,
//    pub no: usize,
//    pub block: Block,
//    pub votes: ReplVec<crypto::Hash>,
//}
//
//impl BlockVote {
//    fn vote(&mut self, yes: usize, no: usize) -> Option<Option<Block>> {
//        self.yes += yes;
//        if self.yes > REPLICATION_FACTOR.get() / 2 && !self.block.data.is_empty() {
//            return Some(Some(std::mem::take(&mut self.block)));
//        }
//        self.no += no;
//        if self.no > REPLICATION_FACTOR.get() / 2 {
//            return Some(None);
//        }
//        None
//    }
//}
//
//#[derive(Default)]
//pub struct Chat {
//    pub members: BTreeMap<Identity, Member>,
//    pub finalized: VecDeque<Block>,
//    pub number: BlockNumber,
//    pub buffer: Vec<u8>,
//    pub votes: VecDeque<BlockVote>,
//}
//
//impl Chat {
//    pub fn new(id: Identity, _: ChatName) -> Self {
//        Self { members: [(id, Member::best())].into(), ..Default::default() }
//    }
//
//    pub fn resolve_finalization(
//        &mut self,
//        cx: crate::Context,
//        group: super::FullReplGroup,
//        name: ChatName,
//    ) -> Option<crypto::Hash> {
//        let selected =
//            self.select_finalizer(group.clone(), self.number, self.buffer.len() - BLOCK_SIZE);
//        if selected != cx.local_peer_id {
//            return None;
//        }
//
//        let actual_block_size = {
//            let trashold = self.buffer.len() / BLOCK_SIZE * BLOCK_SIZE;
//            let mut res = self.buffer.len();
//            for msg in unpack_messages_ref(&self.buffer) {
//                res -= msg.len() + 2;
//                if res <= trashold {
//                    break;
//                }
//            }
//            res
//        };
//
//        let latest_hash = self.get_latest_base_hash(self.number);
//        // _ = cx
//        //     .repl_rpc::<()>(
//        //         name,
//        //         rpcs::SEND_BLOCK,
//        //         (self.number, latest_hash, Reminder(&self.buffer[..actual_block_size])),
//        //     )
//        //     .await;
//
//        self.votes.push_back(BlockVote {
//            number: self.number,
//            yes: 1,
//            no: 0,
//            block: Block::new(self.buffer[..actual_block_size].to_vec(), latest_hash),
//            votes: ReplVec::new(),
//        });
//
//        Some(latest_hash)
//    }
//
//    pub fn finalize_block(&mut self, _us: NodeIdentity, number: BlockNumber, block: Block) {
//        let mapping = unpack_messages_ref(&block.data).collect::<HashSet<_>>();
//
//        let mut new_len = 0;
//        let mut delete_shapshot = None;
//        retain_messages(&mut self.buffer, |msg| {
//            let keep = !mapping.contains(msg);
//            new_len += (msg.len() + 2) * keep as usize;
//            if !keep {
//                delete_shapshot = Some(new_len);
//            }
//            keep
//        });
//
//        let len = self.buffer.len();
//        if let Some(delete_shapshot) = delete_shapshot {
//            self.buffer.drain(..len - delete_shapshot);
//        } else {
//            self.buffer.drain(..len - new_len);
//        }
//
//        self.finalized.push_back(block);
//        if self.finalized.len() > BLOCK_HISTORY {
//            self.finalized.pop_front();
//        }
//        self.number = number + 1;
//    }
//
//    pub fn select_finalizer(
//        &self,
//        members: super::FullReplGroup,
//        number: BlockNumber,
//        buffer_len: usize,
//    ) -> crypto::Hash {
//        let Some(last_block) = self.get_latest_block(number) else {
//            return select_finalizer(members, crypto::Hash::default(), 0);
//        };
//
//        select_finalizer(members, last_block.hash, buffer_len)
//    }
//
//    pub fn get_latest_block(&self, number: BlockNumber) -> Option<&Block> {
//        self.votes
//            .iter()
//            .rfind(|v| {
//                v.yes > REPLICATION_FACTOR.get() / 2 && v.number.checked_sub(1) == Some(number)
//            })
//            .map(|v| &v.block)
//            .or(self.finalized.back())
//    }
//
//    pub fn get_latest_base_hash(&self, number: BlockNumber) -> crypto::Hash {
//        self.get_latest_block(number).map(|b| b.hash).unwrap_or_default()
//    }
//
//    fn process_vote_decision(
//        &mut self,
//        us: NodeIdentity,
//        i: usize,
//        number: BlockNumber,
//        decision: Option<Option<Block>>,
//    ) {
//        match decision {
//            Some(Some(block)) => {
//                self.finalize_block(us, number, block);
//                self.votes.drain(..i + 1);
//            }
//            Some(None) => _ = self.votes.remove(i),
//            None => {}
//        }
//    }
//}
//
