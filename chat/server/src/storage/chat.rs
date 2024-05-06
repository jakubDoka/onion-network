use {
    crate::{api::FullReplGroup, Context},
    anyhow::Context as _,
    chain_api::{NodeIdentity, Nonce},
    chat_spec::{
        BlockNumber, ChatError, ChatName, Cursor, Identity, Member, ReplVec,
        MAX_MESSAGE_FETCH_SIZE, MAX_MESSAGE_SIZE, REPLICATION_FACTOR,
    },
    codec::{Encode, ReminderOwned},
    crypto::proof::{NonceInt, Proof},
    dht::U256,
    lmdb_zero::traits::LmdbResultExt,
    lru::LruCache,
    std::{
        collections::{HashMap, HashSet},
        fs,
        io::{Read, Seek, Write},
        ops::Deref,
        os::unix::fs::FileExt,
        path::PathBuf,
        sync::{atomic::AtomicU64, Arc, Mutex},
    },
};

const CHAT_DIR: &str = "chats/";
const BLOCKS_PATH: &str = "blocks.bin";
const MEMBERS_PATH: &str = "members.lmdb";
const ROOT_PATH: &str = "root.bin";

const BLOCK_SIZE: usize = 1024 * 16;
const MAX_UNFINALIZED_BLOCKS: usize = 5;
pub const UNFINALIZED_BUFFER_CAP: usize = BLOCK_SIZE * MAX_UNFINALIZED_BLOCKS;
const BLOCK_HISTORY: usize = 8;

pub type Storage = Mutex<LruCache<ChatName, Arc<Handle>>>;

pub fn new() -> Storage {
    Mutex::new(LruCache::new(512.try_into().unwrap()))
}

pub fn exists(mut root: PathBuf, chat: ChatName) -> bool {
    root.push(CHAT_DIR);
    _ = super::push_id(&mut root, chat.as_bytes());
    root.exists()
}

#[repr(packed)]
#[derive(Default)]
struct Root {
    number: BlockNumber,
    buffer_len: usize,
    checkpoint: u64,
    latest_block: crypto::Hash,
}

impl Root {
    fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self as *const _ as *const u8, std::mem::size_of::<Self>())
        }
    }

    fn buffer_start(&self) -> std::io::SeekFrom {
        std::io::SeekFrom::End(-(self.buffer_len as i64))
    }

    fn load(file: &mut fs::File) -> Result<Root, ChatError> {
        let mut buf = [0; std::mem::size_of::<Root>()];
        file.read_exact_at(&mut buf, 0)?;
        Ok(unsafe { std::ptr::read(buf.as_ptr() as *const _) })
    }

    fn store(&self, file: &mut fs::File) -> Result<(), ChatError> {
        file.write_all_at(self.as_bytes(), 0).map_err(Into::into)
    }
}

fn select_finalizer(
    selector: crypto::Hash,
    mut members: FullReplGroup,
    block_len: usize,
) -> crypto::Hash {
    debug_assert!(members.is_full());
    let selector = U256::from(selector);
    let round_count = block_len / BLOCK_SIZE;
    members.sort_unstable_by_key(|&member| U256::from(member) ^ selector);
    members[round_count % members.len()].into()
}

pub struct Handle {
    members: lmdb_zero::Database<'static>,
    voting: Mutex<HashMap<crypto::Hash, BlockProposal>>,
    root: Mutex<fs::File>,
    blocks: Mutex<fs::File>,
    discarded_bytes: AtomicU64,
}

impl Handle {
    pub fn load(mut root_path: PathBuf, name: ChatName) -> Result<Self, ChatError> {
        root_path.push(CHAT_DIR);
        super::push_id(&mut root_path, name.as_bytes())?;

        root_path.push(ROOT_PATH);
        let root = fs::OpenOptions::new().read(true).write(true).open(&root_path)?;
        root_path.pop();

        root_path.push(BLOCKS_PATH);
        let blocks = fs::OpenOptions::new().read(true).write(true).open(&root_path)?;
        root_path.pop();

        root_path.push(MEMBERS_PATH);
        let members = super::open_lmdb(&root_path).context("opening members")?;
        root_path.pop();

        Ok(Self {
            members,
            voting: Default::default(),
            root: root.into(),
            blocks: blocks.into(),
            discarded_bytes: Default::default(),
        })
    }

    pub fn create(mut root: PathBuf, name: ChatName) -> Result<(), ChatError> {
        root.push(CHAT_DIR);
        super::push_id(&mut root, name.as_bytes())?;
        if root.exists() {
            return Err(ChatError::AlreadyExists);
        }
        fs::create_dir_all(&root).context("creating chat directory")?;

        root.push(ROOT_PATH);
        fs::write(&root, Root::default().as_bytes()).context("writing root")?;
        root.pop();

        root.push(BLOCKS_PATH);
        fs::write(&root, &[]).context("writing blocks")?;
        root.pop();

        root.push(MEMBERS_PATH);
        fs::create_dir(&root).context("creating members directory")?;
        root.pop();

        Ok(())
    }

    pub fn create_member(&self, id: Identity, member: Member) -> Result<(), ChatError> {
        let txn = lmdb_zero::WriteTransaction::new(self.members.env())
            .context("starting create member transaction")?;
        txn.access()
            .put(&self.members, &id, &super::DontCare(member), lmdb_zero::put::NOOVERWRITE)
            .context("putting member")?;
        txn.commit().context("committing create member transaction").map_err(Into::into)
    }

    pub fn add_member(
        &self,
        by: Proof<ChatName>,
        id: Identity,
        member: Member,
    ) -> Result<(), ChatError> {
        handlers::ensure!(by.verify(), ChatError::InvalidProof);

        let txn = lmdb_zero::WriteTransaction::new(self.members.env())
            .context("starting add member transaction")?;

        {
            let mut access = txn.access();
            let mut cursor = txn.cursor(&self.members).context("opening add member cursor")?;
            let by_id = by.identity();

            let super::DontCare(by_member): &mut super::DontCare<Member> = cursor
                .overwrite_in_place(&mut access, &by_id, lmdb_zero::put::Flags::empty())
                .context("overwriting by member")?;

            by_member.action = advance_nonce(by_member.action, by.nonce)?;
            by_member.allocate_action(chat_spec::Permissions::INVITE)?;

            let member = Member {
                rank: by_member.rank.max(member.rank),
                permissions: member.permissions & by_member.permissions,
                action_cooldown_ms: by_member.action_cooldown_ms.max(member.action_cooldown_ms),
                ..Default::default()
            };

            cursor
                .put(&mut access, &id, &super::DontCare(member), lmdb_zero::put::NOOVERWRITE)
                .map(|()| Ok(()))
                .ignore_exists(Err(ChatError::AlreadyExists))
                .context("putting added member")??;
        }

        txn.commit().context("committing add member transaction").map_err(Into::into)
    }

    pub fn kick_member(&self, by: Proof<ChatName>, id: Identity) -> Result<(), ChatError> {
        handlers::ensure!(by.verify(), ChatError::InvalidProof);

        let txn = lmdb_zero::WriteTransaction::new(self.members.env())
            .context("starting add member transaction")?;

        {
            let mut access = txn.access();
            let mut cursor = txn.cursor(&self.members).context("opening add member cursor")?;
            let by_id = by.identity();

            let super::DontCare(by_member): &mut super::DontCare<Member> = cursor
                .overwrite_in_place(&mut access, &by_id, lmdb_zero::put::Flags::empty())
                .context("overwriting by member")?;

            by_member.action = advance_nonce(by_member.action, by.nonce)?;

            if by_id != id {
                by_member.allocate_action(chat_spec::Permissions::KICK)?;
                let by_rank = by_member.rank;

                let super::DontCare(member): &super::DontCare<Member> =
                    access.get(&self.members, &id).context("getting member")?;
                handlers::ensure!(member.rank < by_rank, ChatError::NoPermission);
            }

            access.del_key(&self.members, &id).context("deleting member")?;
        }

        txn.commit().context("committing add member transaction").map_err(Into::into)
    }

    pub fn fetch_members(
        &self,
        from: Identity,
        count: usize,
    ) -> Result<Vec<(Identity, Member)>, ChatError> {
        let txn = lmdb_zero::ReadTransaction::new(self.members.env())
            .context("starting fetch members transaction")?;

        let members = {
            let access = txn.access();
            let cursor = txn.cursor(&self.members).context("opening fetch members cursor")?;

            let iter = lmdb_zero::CursorIter::new(
                lmdb_zero::MaybeOwned::Owned(cursor),
                &access,
                |cursor, access| {
                    if from != crypto::Hash::default() {
                        cursor.seek_k_both(access, &from)
                    } else {
                        cursor.first(access)
                    }
                },
                lmdb_zero::Cursor::next::<Identity, super::DontCare<Member>>,
            )
            .context("creating iterator")?;

            iter.map(|r| r.map(|(&id, &super::DontCare(member))| (id, member)))
                .take(count.min(31))
                .collect::<lmdb_zero::Result<Vec<_>>>()
                .context("collecting members")?
        };

        Ok(members)
    }

    // TODO: we need a recovery in case nodes repeatedly ail to finalize a block, and it grows too
    // large, in this case, we should probaby discard the oldest unfinalized block and slide like
    // this untill nodes agree on messsages
    pub fn send_message(
        &self,
        cx: Context,
        members: FullReplGroup,
        by: &Proof<ReminderOwned>,
    ) -> Result<Option<(BlockNumber, ReminderOwned)>, ChatError> {
        let sender = by.identity();
        handlers::ensure!(by.verify(), ChatError::InvalidProof);

        let txn = lmdb_zero::WriteTransaction::new(self.members.env())
            .context("starting send message transaction")?;

        {
            let mut access = txn.access();
            let mut cursor = txn.cursor(&self.members).context("opening send message cursor")?;

            let super::DontCare(by_member): &mut super::DontCare<Member> = cursor
                .overwrite_in_place(&mut access, &sender, lmdb_zero::put::Flags::empty())
                .context("overwriting by member")?;

            by_member.action = advance_nonce(by_member.action, by.nonce)?;
            by_member.allocate_action(chat_spec::Permissions::SEND)?;
        }

        txn.commit().context("committing send message transaction")?;

        let mut root_file = self.root.lock().unwrap();
        let mut root = Root::load(&mut root_file)?;
        let mut blocks = self.blocks.lock().unwrap();

        let file_len = blocks.seek(std::io::SeekFrom::End(0))?;
        let prev_buffer_len = {
            let mut buffer = [0; MAX_MESSAGE_SIZE + std::mem::size_of::<chat_spec::Message>()];
            let message =
                chat_spec::Message { sender, nonce: by.nonce, content: by.context.as_slice() };
            let encoded_message =
                message.encode_to_slice(&mut buffer).context("encoding message")?;

            // FIXME: do this in one write
            blocks.write_all(encoded_message)?;
            blocks.write_all(&(encoded_message.len() as u16).to_be_bytes())?;
            root.buffer_len += encoded_message.len() + 2;

            root.buffer_len - encoded_message.len() - 2
        };

        if file_len - root.checkpoint > (BLOCK_HISTORY * BLOCK_SIZE) as u64 {
            let mut to_copy = file_len - root.checkpoint;
            let mut copied = 0;
            const BUFFER_LEN: usize = 1024 * 8;
            let mut buffer = [0u8; BUFFER_LEN];
            while to_copy > 0 {
                let red = BUFFER_LEN.min(to_copy as usize);
                blocks.read_exact_at(&mut buffer[..red], root.checkpoint + copied)?;
                blocks.write_all_at(&mut buffer[..red], copied)?;
                to_copy -= red as u64;
                copied += red as u64;
            }
            blocks.set_len(copied)?;
            self.discarded_bytes.fetch_add(copied, std::sync::atomic::Ordering::Relaxed);
            root.checkpoint = file_len - root.checkpoint - root.buffer_len as u64;
        }

        if root.buffer_len > UNFINALIZED_BUFFER_CAP {
            log::warn!("discarding oldest unfinalized block");
            let mut block = vec![0; root.buffer_len];
            blocks.seek(root.buffer_start())?;
            blocks.read_exact(&mut block)?;

            let mut kept_chunk_len = 0;
            for msg in chat_spec::unpack_messages_ref(&block) {
                kept_chunk_len += msg.len() + 2;
                if root.buffer_len - kept_chunk_len <= BLOCK_SIZE {
                    break;
                }
            }

            let pos = blocks.seek(root.buffer_start())?;
            blocks.write_all(&block[block.len() - kept_chunk_len..])?;
            blocks.set_len(pos + kept_chunk_len as u64)?;

            root.buffer_len = kept_chunk_len;
        }

        root.store(&mut root_file)?;
        let root = root;

        let should_finalize = root.buffer_len > BLOCK_SIZE
            && root.buffer_len % BLOCK_SIZE > BLOCK_SIZE / 2
            && prev_buffer_len % BLOCK_SIZE < BLOCK_SIZE / 2;

        if !should_finalize {
            return Ok(None);
        }

        let selected = select_finalizer(root.latest_block, members, root.buffer_len);

        if selected != cx.local_peer_id {
            return Ok(None);
        }

        let mut block = vec![0; root.buffer_len];
        blocks.seek(root.buffer_start())?;
        blocks.read_exact(&mut block)?;

        {
            let desired_block_size = root.buffer_len / BLOCK_SIZE * BLOCK_SIZE;
            let mut block_size = root.buffer_len;
            for message in chat_spec::unpack_messages(&mut block) {
                block_size -= message.len() + 2;
                if block_size <= desired_block_size {
                    break;
                }
            }
            block.truncate(block_size);
        }

        let latest_hash = crypto::hash::new(&block);

        self.voting.lock().unwrap().insert(latest_hash, BlockProposal {
            number: root.number,
            yes: 1,
            no: 0,
            proposal: Proposal::Local(block.len()),
            votes: ReplVec::new(),
        });

        Ok(Some((root.number, ReminderOwned(block))))
    }

    pub fn vote(
        &self,
        origin: NodeIdentity,
        bn: BlockNumber,
        hash: crypto::Hash,
        agrees: bool,
    ) -> Result<(), ChatError> {
        let mut voting = self.voting.lock().unwrap();
        let proposal = voting
            .entry(hash)
            .or_insert_with(|| BlockProposal { number: bn, ..Default::default() });

        handlers::ensure!(proposal.number == bn, ChatError::WrongBlockNumber);
        handlers::ensure!(!proposal.votes.contains(&origin), ChatError::AlreadyVoted);

        proposal.votes.push(origin);
        proposal.yes += agrees as usize;
        proposal.no += !agrees as usize;
        let Some(decision) = proposal.try_resolve() else {
            return Ok(());
        };

        if let Some(block) = decision {
            self.accept_block(hash, bn, block, &mut voting)?;
        }

        Ok(())
    }

    pub fn proposal(
        &self,
        origin: NodeIdentity,
        group: FullReplGroup,
        number: BlockNumber,
        block: Vec<u8>,
    ) -> Result<(crypto::Hash, Option<ChatError>), ChatError> {
        let root = Root::load(&mut self.root.lock().unwrap())?;

        handlers::ensure!(number >= root.number, ChatError::OutdatedBlockNumber);
        let base_hash = crypto::hash::new(&block);

        let mut voting = self.voting.lock().unwrap();
        let proposal =
            voting.entry(base_hash).or_insert(BlockProposal { number, ..Default::default() });
        proposal.yes += 1;

        let res = self.proposal_low(origin, group, number, block, proposal, &root);
        proposal.no += res.is_err() as usize;
        proposal.yes += res.is_ok() as usize;

        if let Some(decision) = proposal.try_resolve() {
            if let Some(block) = decision {
                self.accept_block(base_hash, number, block, &mut voting)?;
            } else {
                voting.remove(&base_hash);
            }
        }

        Ok((base_hash, res.err()))
    }

    fn proposal_low(
        &self,
        origin: NodeIdentity,
        group: FullReplGroup,
        number: BlockNumber,
        block: Vec<u8>,
        proposal: &mut BlockProposal,
        root: &Root,
    ) -> Result<(), ChatError> {
        use ChatError as CE;

        handlers::ensure!(matches!(proposal.proposal, Proposal::Absent), CE::AlreadyVoted);
        handlers::ensure!(!proposal.votes.contains(&origin), CE::AlreadyVoted);
        let selected = select_finalizer(root.latest_block, group, block.len() + BLOCK_SIZE - 1);
        handlers::ensure!(selected == origin, CE::WrongProposer);

        proposal.proposal = Proposal::Remote(block);
        let Proposal::Remote(block) = &proposal.proposal else { unreachable!() };

        handlers::ensure!(proposal.number == number, CE::WrongBlockNumber);

        // FIXME: dont allocate buffer, use fixed buffer to count messages
        let buffer = {
            let blocks = self.blocks.lock().unwrap();
            let mut blocks = blocks.deref();
            blocks.seek(root.buffer_start())?;
            let mut buffer = vec![0; root.buffer_len];
            blocks.read_exact(&mut buffer)?;
            buffer
        };

        let proposed = chat_spec::unpack_messages_ref(block).collect::<HashSet<_>>();
        let present_count =
            chat_spec::unpack_messages_ref(&buffer).filter(|msg| proposed.contains(msg)).count();
        handlers::ensure!(present_count == proposed.len(), CE::BlockUnexpectedMessages);

        Ok(())
    }

    pub fn export_data(&self) -> Result<Vec<(Identity, Member)>, ChatError> {
        let txn = lmdb_zero::ReadTransaction::new(self.members.env())
            .context("starting export data transaction")?;
        let access = txn.access();
        let cursor = txn.cursor(&self.members).context("opening export data cursor")?;

        let iter = lmdb_zero::CursorIter::new(
            lmdb_zero::MaybeOwned::Owned(cursor),
            &access,
            |cursor, access| cursor.first(access),
            lmdb_zero::Cursor::next::<Identity, super::DontCare<Member>>,
        )
        .context("creating iterator")?;

        Ok(iter
            .map(|r| r.map(|(&id, &super::DontCare(member))| (id, member)))
            .collect::<lmdb_zero::Result<Vec<_>>>()
            .context("collecting members")?)
    }

    pub fn fetch_messages(
        &self,
        mut cursor: Cursor,
    ) -> Result<([u8; MAX_MESSAGE_FETCH_SIZE], Cursor), ChatError> {
        let blocks = self.blocks.lock().unwrap();
        if cursor == Cursor::MAX {
            cursor = blocks.metadata()?.len() as u64
                + self.discarded_bytes.load(std::sync::atomic::Ordering::Relaxed);
        }

        let mut buf = [0; MAX_MESSAGE_FETCH_SIZE];

        let offset = cursor - self.discarded_bytes.load(std::sync::atomic::Ordering::Relaxed);

        let final_offset = offset.saturating_sub(MAX_MESSAGE_FETCH_SIZE as u64);
        let red = offset - final_offset;

        blocks.read_exact_at(
            &mut buf[MAX_MESSAGE_FETCH_SIZE - red as usize..],
            final_offset as u64,
        )?;

        cursor -= chat_spec::unpack_messages_ref(&buf[MAX_MESSAGE_FETCH_SIZE - red as usize..])
            .map(|msg| msg.len() + 2)
            .sum::<usize>() as u64;

        if red == 0 {
            cursor = Cursor::MAX;
        }

        Ok((buf, cursor))
    }

    pub fn remove(mut root: PathBuf, name: ChatName) -> Result<(), ChatError> {
        root.push(CHAT_DIR);
        super::push_id(&mut root, name.as_bytes())?;
        fs::remove_dir_all(&root).map_err(Into::into)
    }

    pub fn recover_members(&self, chat_data: HashMap<Identity, Member>) -> Result<(), ChatError> {
        let txn = lmdb_zero::WriteTransaction::new(self.members.env())
            .context("starting recover members transaction")?;

        {
            let mut access = txn.access();
            access.clear_db(&self.members).context("clearing members")?;
            let mut cursor = txn.cursor(&self.members).context("opening recover members cursor")?;

            for (id, member) in chat_data {
                cursor
                    .put(&mut access, &id, &super::DontCare(member), lmdb_zero::put::NOOVERWRITE)
                    .context("putting member")?;
            }
        }

        txn.commit().context("committing recover members transaction").map_err(Into::into)
    }

    fn accept_block(
        &self,
        hash: crypto::Hash,
        number: BlockNumber,
        block: Result<usize, Vec<u8>>,
        voting: &mut HashMap<crypto::Hash, BlockProposal>,
    ) -> Result<(), ChatError> {
        let mut root_file = self.root.lock().unwrap();
        let mut root = Root::load(&mut root_file)?;
        root.number = number;
        root.latest_block = hash;

        voting.retain(|_, proposal| proposal.number >= root.number);

        match block {
            Ok(size) => {
                root.buffer_len -= size;
            }
            Err(data) => {
                let mut blocks = self.blocks.lock().unwrap();

                let mut buffer = vec![0; root.buffer_len];
                let pos = blocks.seek(root.buffer_start())?;
                blocks.read_exact(&mut buffer)?;
                blocks.set_len(pos)?;

                let lookup = chat_spec::unpack_messages_ref(&data).collect::<HashSet<_>>();
                chat_spec::retain_messages_in_vec(&mut buffer, |message| !lookup.contains(message));
                root.buffer_len = buffer.len();
                buffer.splice(0..0, data);

                blocks.seek(std::io::SeekFrom::End(0))?;
                blocks.write_all(&buffer)?;
            }
        }

        root.store(&mut root_file)
    }

    pub fn update_member(
        &self,
        by: Proof<ChatName>,
        id: Identity,
        member: Member,
    ) -> Result<(), ChatError> {
        handlers::ensure!(by.verify(), ChatError::InvalidProof);

        let txn = lmdb_zero::WriteTransaction::new(self.members.env())
            .context("starting add member transaction")?;

        {
            let mut access = txn.access();
            let mut cursor = txn.cursor(&self.members).context("opening add member cursor")?;
            let by_id = by.identity();

            let super::DontCare(by_member): &mut super::DontCare<Member> = cursor
                .overwrite_in_place(&mut access, &by_id, lmdb_zero::put::Flags::empty())
                .context("overwriting by member")?;

            by_member.action = advance_nonce(by_member.action, by.nonce)?;
            let rank = by_member.rank;

            let member = Member {
                rank: by_member.rank.max(member.rank),
                permissions: member.permissions & by_member.permissions,
                action_cooldown_ms: by_member.action_cooldown_ms.max(member.action_cooldown_ms),
                ..Default::default()
            };

            let super::DontCare(dest): &mut super::DontCare<Member> = cursor
                .overwrite_in_place(&mut access, &id, lmdb_zero::put::Flags::empty())
                .context("overwriting member")?;

            handlers::ensure!(dest.rank > rank, ChatError::NoPermission);
            *dest = member;
        }

        txn.commit().context("committing add member transaction").map_err(Into::into)
    }
}

fn advance_nonce(mut nonce: Nonce, new_nonce: Nonce) -> Result<Nonce, ChatError> {
    if !nonce.advance_to(new_nonce) {
        return Err(ChatError::InvalidAction(nonce + 1));
    }
    Ok(nonce)
}

#[derive(Default)]
enum Proposal {
    #[default]
    Absent,
    Local(usize),
    Remote(Vec<u8>),
}

impl Proposal {
    fn take(&mut self) -> Option<Result<usize, Vec<u8>>> {
        match std::mem::replace(self, Self::Absent) {
            Self::Absent => None,
            Self::Local(size) => Some(Ok(size)),
            Self::Remote(data) => Some(Err(data)),
        }
    }
}

#[derive(Default)]
struct BlockProposal {
    number: BlockNumber,
    yes: usize,
    no: usize,
    proposal: Proposal,
    votes: ReplVec<crypto::Hash>,
}

impl BlockProposal {
    fn try_resolve(&mut self) -> Option<Option<Result<usize, Vec<u8>>>> {
        if self.yes > REPLICATION_FACTOR.get() / 2
            && let Some(prop) = self.proposal.take()
        {
            return Some(Some(prop));
        }
        if self.no > REPLICATION_FACTOR.get() / 2 {
            return Some(None);
        }
        None
    }
}
