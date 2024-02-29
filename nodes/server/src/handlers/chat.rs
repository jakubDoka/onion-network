use {
    super::{Codec, Protocol, ProtocolResult, RequestOrigin, Scope, SyncHandler},
    chat_spec::{
        advance_nonce, retain_messages_in_vec, unpack_messages, unpack_messages_ref, BlockNumber,
        ChatAction, ChatActionError, ChatEvent, ChatName, CreateChat, CreateChatError, Cursor,
        FetchLatestBlockError, FetchMessages, FetchMessagesError, FetchMinimalChatData, Identity,
        InvalidBlockReason, Member, Message, Nonce, PerformChatAction, ProposeMsgBlock,
        ProposeMsgBlockError, SendBlock, SendBlockError, REPLICATION_FACTOR,
    },
    component_utils::{encode_len, Buffer, NoCapOverflow, Reminder},
    std::{
        borrow::Cow,
        collections::{HashMap, VecDeque},
        iter, usize,
    },
};

const MAX_MESSAGE_SIZE: usize = 1024;
const MESSAGE_FETCH_LIMIT: usize = 20;
const BLOCK_SIZE: usize = if cfg!(test) { 1024 * 4 } else { 1024 * 32 };
const BLOCK_HISTORY: usize = 32;

impl SyncHandler for CreateChat {
    fn execute<'a>(
        mut cx: Scope<'a>,
        (name, identity): Self::Request<'_>,
    ) -> ProtocolResult<'a, Self> {
        let chat_entry = cx.storage.chats.entry(name);
        crate::ensure!(
            let std::collections::hash_map::Entry::Vacant(entry) = chat_entry,
            CreateChatError::AlreadyExists
        );

        entry.insert(Chat::new(identity));

        Ok(())
    }
}

impl SyncHandler for PerformChatAction {
    fn execute<'a>(
        mut sc: Scope<'a>,
        (proof, action): Self::Request<'_>,
    ) -> ProtocolResult<'a, Self> {
        crate::ensure!(proof.verify(), ChatActionError::InvalidProof);

        let chat =
            sc.cx.storage.chats.get_mut(&proof.context).ok_or(ChatActionError::ChatNotFound)?;

        let sender_id = crypto::hash::from_raw(&proof.pk);
        let sender = chat.members.get_mut(&sender_id).ok_or(ChatActionError::NotMember)?;

        crate::ensure!(
            advance_nonce(&mut sender.action, proof.nonce),
            ChatActionError::InvalidAction(sender.action)
        );

        match action {
            ChatAction::AddUser(id) => {
                // TODO: write the member addition to message history so it can be finalized
                crate::ensure!(
                    chat.members.try_insert(id, Member::default()).is_ok(),
                    ChatActionError::AlreadyMember
                );
            }
            ChatAction::SendMessage(Reminder(msg)) => {
                crate::ensure!(msg.len() <= MAX_MESSAGE_SIZE, ChatActionError::MessageTooLarge);

                // TODO: move this to context
                let bn = chat.number;
                let message = Message {
                    identiy: sender_id,
                    nonce: sender.action - 1,
                    content: Reminder(msg),
                };
                match chat.push_message(message, &mut sc.cx.res.hashes) {
                    Err(Some(hash)) => send_block_proposals(sc.reborrow(), proof.context, bn, hash),
                    Err(None) => return Err(ChatActionError::MessageBlockNotFinalized),
                    Ok(()) => (),
                }

                sc.push(proof.context, ChatEvent::Message(proof, Reminder(msg)));
            }
        }

        Ok(())
    }
}

fn send_block_proposals(sc: Scope, name: ChatName, number: BlockNumber, hash: crypto::Hash) {
    let us = *sc.cx.swarm.local_peer_id();
    let beh = sc.cx.swarm.behaviour_mut();
    let mut msg = [0; std::mem::size_of::<(u8, ChatName, crypto::Hash)>()];
    ProposeMsgBlock::rpc((name, number, hash)).encode(&mut msg.as_mut_slice()).unwrap();
    for recip in crate::other_replicators_for(&beh.dht.table, name, us) {
        _ = beh.rpc.request(recip, msg);
    }
}

impl SyncHandler for ProposeMsgBlock {
    fn execute<'a>(
        sc: Scope<'a>,
        (chat, number, phash): Self::Request<'_>,
    ) -> ProtocolResult<'a, Self> {
        crate::ensure!(let RequestOrigin::Server(origin) = sc.origin, ProposeMsgBlockError::NotServer);

        let index = sc
            .other_replicators_for(chat)
            .map(RequestOrigin::Server)
            .position(|id| id == sc.origin)
            .ok_or(ProposeMsgBlockError::NoReplicator)?;

        crate::ensure!(
            let Some(chat_data) = sc.cx.storage.chats.get_mut(&chat),
            ProposeMsgBlockError::ChatNotFound
        );

        let our_finalized = chat_data.last_finalized_block();
        match number.cmp(&our_finalized) {
            std::cmp::Ordering::Less if our_finalized - number <= 1 => {
                crate::ensure!(
                    let Some(block) = chat_data.finalized.front(),
                    ProposeMsgBlockError::NoBlocks
                );

                if block.hash == phash {
                    return Ok(());
                }

                let packet =
                    SendBlock::rpc((chat, number, Reminder(block.data.as_ref()))).to_bytes();
                _ = sc.cx.swarm.behaviour_mut().rpc.request(origin, packet);
                return Ok(());
            }
            std::cmp::Ordering::Less => todo!("sender needs more complex data recovery"),
            std::cmp::Ordering::Equal => {}
            std::cmp::Ordering::Greater if number - our_finalized <= 1 => {}
            std::cmp::Ordering::Greater => todo!("we are behind, so I guess just wait for blocks"),
        }

        let BlockStage::Unfinalized { proposed, others } = &mut chat_data.stage else {
            return Ok(());
        };

        let we_finalized = proposed.is_some();
        let we_match = proposed.as_ref().map(|p| p.hash) == Some(phash);

        others[index] = phash;

        if others.iter().filter(|h| **h == phash).count()
            > REPLICATION_FACTOR.get() / 2 - usize::from(we_match)
        {
            chat_data.stage = if let Some(block) = proposed.take()
                && block.hash == phash
            {
                chat_data.push_to_finalized(block);
                BlockStage::default()
            } else {
                BlockStage::Recovering { final_hash: phash, we_finalized }
            };
        } else if !others.contains(&Default::default()) && we_finalized {
            todo!("no majority, we need to initialize recovery");
        }

        Ok(())
    }
}

impl SyncHandler for SendBlock {
    fn execute<'a>(
        sc: Scope<'a>,
        (chat, number, Reminder(block)): Self::Request<'_>,
    ) -> ProtocolResult<'a, Self> {
        use {
            InvalidBlockReason::{MajorityMismatch, NotExpected, Outdated},
            SendBlockError::{ChatNotFound, InvalidBlock, NoReplicator},
        };

        let index = sc
            .other_replicators_for(chat)
            .map(RequestOrigin::Server)
            .position(|id| id == sc.origin)
            .ok_or(NoReplicator)?;

        crate::ensure!(
            let Some(chat_data) = sc.cx.storage.chats.get_mut(&chat),
            ChatNotFound
        );

        crate::ensure!(chat_data.last_finalized_block() == number, InvalidBlock(Outdated));

        match &mut chat_data.stage {
            BlockStage::Unfinalized { proposed: Some(_), others } => {
                let hash = Chat::hash_block(block, &mut sc.cx.res.hashes);

                others[index] = hash;

                if others.iter().filter(|h| **h == hash).count() < REPLICATION_FACTOR.get() / 2 {
                    Err(InvalidBlock(MajorityMismatch))
                } else {
                    chat_data.stage = BlockStage::default();
                    chat_data.push_to_finalized(Block { hash, data: block.into() });

                    Ok(())
                }
            }
            BlockStage::Unfinalized { .. } => Err(InvalidBlock(NotExpected)),
            BlockStage::Recovering { final_hash, .. } => {
                let hash_temp = &mut sc.cx.res.hashes;
                let hash = Chat::hash_block(block, hash_temp);
                crate::ensure!(hash == *final_hash, InvalidBlock(MajorityMismatch));

                retain_messages_in_vec(&mut chat_data.current_block, |msg| {
                    // this is fine since message contains sender id and nonce which is
                    // unique for each messsage
                    !hash_temp.contains(&crypto::hash::from_slice(msg))
                });

                chat_data.push_to_finalized(Block { hash, data: block.into() });

                Ok(())
            }
        }
    }
}

impl SyncHandler for FetchMinimalChatData {
    fn execute<'a>(cx: Scope<'a>, req: Self::Request<'_>) -> ProtocolResult<'a, Self> {
        let chat = cx.cx.storage.chats.get(&req).ok_or(FetchLatestBlockError::ChatNotFound)?;
        Ok((chat.number, Cow::Borrowed(&chat.members), Reminder(chat.current_block.as_slice())))
    }
}

impl SyncHandler for FetchMessages {
    fn execute<'a>(
        sc: Scope<'a>,
        (chat, mut cursor): Self::Request<'_>,
    ) -> ProtocolResult<'a, Self> {
        let chat = sc.cx.storage.chats.get_mut(&chat).ok_or(FetchMessagesError::ChatNotFound)?;

        if cursor == Cursor::INIT {
            cursor.block = chat.number;
            cursor.offset = chat.current_block.len();
        }

        let bail = Ok((Cursor::INIT, Reminder(&[])));

        if cursor.offset == 0 {
            if cursor.block == 0 {
                return bail;
            }
            cursor.block += 1;
        }

        let Some(block) = iter::once(chat.current_block.as_mut_slice())
            .chain(chat.stage.unfinalized_block())
            .chain(chat.finalized.iter_mut().map(|b| b.data.as_mut()))
            .nth(chat.number.saturating_sub(cursor.block) as usize)
        else {
            return bail;
        };

        if cursor.offset == 0 {
            cursor.offset = block.len();
        }

        let Some(block) = block.get_mut(..cursor.offset) else {
            return bail;
        };

        let message_length = unpack_messages(block)
            .take(MESSAGE_FETCH_LIMIT)
            .map(|msg| msg.len() + 2)
            .sum::<usize>();

        let slice = &block[cursor.offset - message_length..cursor.offset];
        cursor.offset -= message_length;

        Ok((cursor, Reminder(slice)))
    }
}

#[derive(Codec)]
struct Block {
    hash: crypto::Hash,
    data: Box<[u8]>,
}

#[derive(Codec)]
enum BlockStage {
    Unfinalized { proposed: Option<Block>, others: [crypto::Hash; REPLICATION_FACTOR.get()] },
    Recovering { final_hash: crypto::Hash, we_finalized: bool },
}

impl Default for BlockStage {
    fn default() -> Self {
        Self::Unfinalized { proposed: None, others: Default::default() }
    }
}

impl BlockStage {
    fn unfinalized_block(&mut self) -> Option<&mut [u8]> {
        match self {
            Self::Unfinalized { proposed, .. } => proposed.as_mut().map(|p| p.data.as_mut()),
            _ => None,
        }
    }
}

#[derive(Codec, Default)]
pub struct Chat {
    members: HashMap<Identity, Member>,
    finalized: VecDeque<Block>,
    current_block: Vec<u8>,
    pub(crate) number: BlockNumber,
    stage: BlockStage,
}

impl Chat {
    pub fn new(id: Identity) -> Self {
        Self {
            members: [(id, Member::default())].into(),
            finalized: Default::default(),
            current_block: Vec::with_capacity(BLOCK_SIZE),
            number: 0,
            stage: Default::default(),
        }
    }

    pub fn recovered(
        members: HashMap<Identity, Member>,
        number: BlockNumber,
        current_block: Vec<u8>,
    ) -> Self {
        Self {
            members,
            finalized: Default::default(),
            current_block,
            number,
            stage: Default::default(),
        }
    }

    pub fn is_recovering(&self) -> bool {
        self.members.is_empty()
    }

    pub fn push_message<'a>(
        &mut self,
        msg: impl Codec<'a>,
        hash_temp: &mut Vec<crypto::Hash>,
    ) -> Result<(), Option<crypto::Hash>> {
        let prev_len = self.current_block.len();

        fn try_push<'a>(block: &mut Vec<u8>, msg: impl Codec<'a>) -> Option<()> {
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

        let err = match &mut self.stage {
            BlockStage::Unfinalized { proposed, .. } if proposed.is_some() => return Err(None),
            BlockStage::Unfinalized { proposed, others } => {
                let hash = Self::hash_block(self.current_block.as_slice(), hash_temp);
                if others.iter().filter(|h| **h == hash).count() >= REPLICATION_FACTOR.get() / 2 {
                    self.finalize_current_block(hash);
                } else {
                    *proposed = Some(Block { hash, data: self.current_block.as_slice().into() });
                    self.current_block.clear();
                    self.number += 1;
                }
                Some(hash)
            }
            BlockStage::Recovering { we_finalized, .. } if *we_finalized => return Err(None),
            BlockStage::Recovering { final_hash, we_finalized } => {
                *we_finalized = true;
                let hash = Self::hash_block(self.current_block.as_slice(), hash_temp);
                if hash == *final_hash {
                    self.finalize_current_block(hash);
                } else {
                    self.current_block.clear();
                }
                Some(hash)
            }
        };

        try_push(&mut self.current_block, msg).expect("we checked size limits");

        Err(err)
    }

    fn finalize_current_block(&mut self, hash: crypto::Hash) {
        self.stage = BlockStage::default();
        self.push_to_finalized(Block { hash, data: self.current_block.as_slice().into() });
        self.current_block.clear();
        self.number += 1;
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
        self.number
            - u64::from(matches!(
                self.stage,
                BlockStage::Unfinalized { proposed: Some(_), .. } | BlockStage::Recovering { .. }
            ))
    }
}
