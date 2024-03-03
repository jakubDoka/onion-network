use {
    chat_spec::{
        advance_nonce, retain_messages_in_vec, rpcs, unpack_messages_ref, BlockNumber, ChatError,
        ChatEvent, ChatName, Cursor, Identity, Member, Message, Proof, REPLICATION_FACTOR,
    },
    component_utils::{encode_len, Buffer, NoCapOverflow, Reminder},
    std::{
        collections::{HashMap, VecDeque},
        sync::Arc,
        usize,
    },
    tokio::sync::RwLock,
};

const MAX_MESSAGE_SIZE: usize = 1024;
const MESSAGE_FETCH_LIMIT: usize = 20;
const BLOCK_SIZE: usize = if cfg!(test) { 1024 * 4 } else { 1024 * 32 };
const BLOCK_HISTORY: usize = 32;

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
            cx.replicate_rpc_no_resp(
                proof.context,
                rpcs::BLOCK_PROPOSAL,
                (proof.context, bn, hash),
            )
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
    (chat_name, number, phash): (ChatName, BlockNumber, crypto::Hash),
) {
    let Some(index) = repl.iter().position(|id| *id == origin) else {
        return;
    };

    let Some(chat) = cx.chats.get_mut(&chat_name).map(|c| c.value().clone()) else {
        return;
    };

    {
        let chat = chat.read().await;

        let our_finalized = chat.last_finalized_block();
        match number.cmp(&our_finalized) {
            std::cmp::Ordering::Less => {
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

                cx.send_rpc_no_resp(
                    chat_name,
                    origin,
                    rpcs::SEND_BLOCK,
                    (chat_name, number, Reminder(block)),
                )
                .await;
                return;
            }
            std::cmp::Ordering::Equal => {}
            std::cmp::Ordering::Greater if number - our_finalized <= 1 => {}
            std::cmp::Ordering::Greater => todo!("we are behind, so I guess just wait for blocks"),
        }
    }

    let mut chat = chat.write().await;

    let BlockStage::Unfinalized { proposed, others } = &mut chat.stage else {
        return;
    };

    let we_finalized = proposed.is_some();
    let we_match = proposed.as_ref().map(|p| p.hash) == Some(phash);

    others[index] = phash;

    if others.iter().filter(|h| **h == phash).count()
        > REPLICATION_FACTOR.get() / 2 - usize::from(we_match)
    {
        chat.stage = if let Some(block) = proposed.take()
            && block.hash == phash
        {
            chat.push_to_finalized(block);
            BlockStage::default()
        } else {
            BlockStage::Recovering { final_hash: phash, we_finalized }
        };
    } else if !others.contains(&Default::default()) && we_finalized {
        todo!("no majority, we need to initialize recovery");
    }
}

pub async fn send_block(
    cx: crate::Context,
    origin: super::Origin,
    repl: super::ReplGroup,
    (chat, number, Reminder(block)): (ChatName, BlockNumber, Reminder<'_>),
) -> Result<()> {
    use {chat_spec::InvalidBlockReason::*, ChatError::*};

    let Some(index) = repl.iter().position(|id| *id == origin) else {
        return Err(NoReplicator);
    };

    let chat = cx.chats.get(&chat).ok_or(NotFound)?.clone();
    let mut chat = chat.write().await;

    crate::ensure!(chat.last_finalized_block() == number, InvalidBlock(Outdated));

    match &mut chat.stage {
        BlockStage::Unfinalized { proposed: Some(_), others } => {
            let hash = Chat::hash_block(block, &mut vec![]);

            others[index] = hash;

            if others.iter().filter(|h| **h == hash).count() < REPLICATION_FACTOR.get() / 2 {
                Err(InvalidBlock(MajorityMismatch))
            } else {
                chat.stage = BlockStage::default();
                chat.push_to_finalized(Block { hash, data: block.into() });

                Ok(())
            }
        }
        BlockStage::Unfinalized { .. } => Err(InvalidBlock(NotExpected)),
        BlockStage::Recovering { final_hash, .. } => {
            let hash_temp = &mut vec![];
            let hash = Chat::hash_block(block, hash_temp);
            crate::ensure!(hash == *final_hash, InvalidBlock(MajorityMismatch));

            retain_messages_in_vec(&mut chat.current_block, |msg| {
                // this is fine since message contains sender id and nonce which is
                // unique for each messsage
                !hash_temp.contains(&crypto::hash::from_slice(msg))
            });

            chat.push_to_finalized(Block { hash, data: block.into() });

            Ok(())
        }
    }
}

pub async fn fetch_minimal_chat_data(
    cx: crate::Context,
    chat: ChatName,
) -> Result<(BlockNumber, HashMap<Identity, Member>, Vec<u8>)> {
    let chat = cx.chats.get(&chat).ok_or(ChatError::NotFound)?;
    let chat = chat.read().await;

    Ok((chat.number, chat.members.clone(), chat.current_block.clone()))
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
        Some(block) => block.data.as_ref(),
        None => return bail,
    };

    if cursor.offset == 0 {
        cursor.offset = block.len();
    }

    let slice = &block[cursor.offset - MESSAGE_FETCH_LIMIT..cursor.offset];
    cursor.offset -= slice.len();

    Ok((cursor, slice.to_vec()))
}

#[derive(component_utils::Codec)]
struct Block {
    hash: crypto::Hash,
    data: Box<[u8]>,
}

#[derive(component_utils::Codec)]
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
    fn _unfinalized_block(&mut self) -> Option<&mut [u8]> {
        match self {
            Self::Unfinalized { proposed, .. } => proposed.as_mut().map(|p| p.data.as_mut()),
            _ => None,
        }
    }
}

#[derive(component_utils::Codec, Default)]
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

pub async fn recover(cx: crate::Context, name: ChatName) -> Result<()> {
    todo!("recover chat")
}
