use {
    anyhow::Context,
    codec::Codec,
    lmdb_zero::{
        open, put, traits::LmdbRaw, DatabaseOptions, LmdbResultExt, ReadTransaction,
        WriteTransaction,
    },
    rand_core::OsRng,
    std::{
        collections::{hash_map::Entry, HashMap},
        sync::RwLock,
    },
    storage_spec::*,
};

const GC_COOLDOWN: u64 = 60 * 60;

#[derive(Default)]
pub struct NodeProfiles {
    records: Vec<NodeProfile>,
    identity_to_id: HashMap<NodeIdentity, NodeId>,
    free_space_index: Vec<(FreeSpace, NodeId)>,
    fills: Vec<(BlockFill, [NodeId; MAX_PIECES], BlockId)>,
}

impl<'a> Codec<'a> for NodeProfiles {
    fn encode(&self, buffer: &mut impl codec::Buffer) -> Option<()> {
        self.records.encode(buffer)?;
        self.fills.encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let records = Vec::<NodeProfile>::decode(buffer)?;
        let fills = Vec::<(BlockFill, [NodeId; MAX_PIECES], BlockId)>::decode(buffer)?;
        let identity_to_id =
            records.iter().enumerate().map(|(i, p)| (p.identity, i as _)).collect();
        let free_space_index =
            records.iter().enumerate().map(|(i, p)| (p.free_blocks, i as _)).collect();

        Some(Self { records, fills, identity_to_id, free_space_index })
    }
}

impl NodeProfiles {
    fn load(metabase_root: &str) -> anyhow::Result<Self> {
        let path = std::path::Path::new(metabase_root).join("node_profiles");
        let bytes = std::fs::read(&path)?;
        Self::decode(&mut &bytes[..]).context("failed to decode node profiles")
    }

    pub fn register_node(&mut self, identity: NodeIdentity, free_blocks: FreeSpace) -> bool {
        let id = self.records.len() as NodeId;
        let Entry::Vacant(entry) = self.identity_to_id.entry(identity) else {
            return false;
        };

        entry.insert(id);
        self.free_space_index.push((free_blocks, id));
        self.records.push(NodeProfile { identity, free_blocks, ..Default::default() });

        true
    }

    pub fn request_gc(&mut self, identity: NodeIdentity, nonce: u64) -> Option<()> {
        let current_time =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

        let id = *self.identity_to_id.get(&identity)?;
        let profile = &mut self.records[id as usize];

        if current_time - profile.last_requested_gc < GC_COOLDOWN || profile.nonce + 1 != nonce {
            return None;
        }

        profile.last_requested_gc = current_time;
        profile.nonce += 1;

        Some(())
    }

    pub fn allocate_file(&mut self, size: u64) -> Option<(Address, Holders)> {
        let alloc_size = size.div_ceil(PIECE_SIZE as _) as usize;
        let fill_res = self
            .fills
            .last_mut()
            .map(|(fill, nodes, id)| fill.alloc(alloc_size).map(|index| (index, *nodes, *id)))
            .unwrap_or_else(|| Err(BlockFill::split_size(alloc_size)));

        let (count, last_fill) = match fill_res {
            Ok((block_offset, holders, starting_block)) => {
                return Some((Address { starting_block, block_offset, size }, holders));
            }
            Err(e) => e,
        };

        let (_, most_free) = self.free_space_index.split_last_chunk_mut::<MAX_PIECES>()?;
        most_free.iter().any(|(free, _)| *free < count).then_some(())?;

        for (free, id) in &mut *most_free {
            *free -= count;
            self.records[*id as usize].free_blocks = *free;
        }

        let starting_block = crypto::new_secret(OsRng);
        let reminder = (0..count).fold(starting_block, |acc, _| crypto::hash::new(acc));
        let holders = most_free.map(|(_, id)| id);
        self.fills.push((last_fill, holders, reminder));

        Some((Address { starting_block, block_offset: 0, size }, holders))
    }

    pub fn expand_holders(&self, holders: [NodeId; MAX_PIECES]) -> [NodeIdentity; MAX_PIECES] {
        holders.map(|id| self.records[id as usize].identity)
    }
}

pub struct LmdbMap<K, V> {
    files: lmdb_zero::Database<'static>,
    phantom: std::marker::PhantomData<(K, V)>,
}

impl<K: Copy, V: Copy> LmdbMap<K, V> {
    fn new(metabase_root: &str, name: &str) -> anyhow::Result<Self> {
        let env = unsafe {
            lmdb_zero::EnvBuilder::new()?.open(
                &format!("{metabase_root}/{name}"),
                open::Flags::empty(),
                0o600,
            )?
        };
        Ok(Self {
            files: lmdb_zero::Database::open(env, None, &DatabaseOptions::defaults())?,
            phantom: Default::default(),
        })
    }

    pub fn save(&self, k: K, v: V) -> anyhow::Result<bool> {
        let wr = WriteTransaction::new(self.files.env()).context("creating transaction")?;
        let res = wr
            .access()
            .put(&self.files, AsBytes::of_ref(&k), AsBytes::of_ref(&v), put::NOOVERWRITE)
            .map(|()| true)
            .ignore_exists(false)
            .context("putting value")?;
        wr.commit().context("commiting transaction")?;
        Ok(res)
    }

    pub fn load(&self, address: Address) -> anyhow::Result<Option<FileMeta>> {
        let rd = ReadTransaction::new(self.files.env()).context("creating transaction")?;
        let res = rd
            .access()
            .get(&self.files, AsBytes::of_ref(&address))
            .to_opt()
            .context("getting value")?
            .map(|v: &AsBytes<_>| v.0);
        Ok(res)
    }

    pub fn delete(&self, address: K, guard: impl FnOnce(V) -> bool) -> anyhow::Result<bool> {
        let wr = WriteTransaction::new(self.files.env()).context("creating transaction")?;
        if !wr
            .access()
            .get::<_, AsBytes<V>>(&self.files, AsBytes::of_ref(&address))
            .to_opt()
            .context("checking owner")?
            .is_some_and(|v| guard(v.0))
        {
            return Ok(false);
        };
        wr.access().del_key(&self.files, AsBytes::of_ref(&address)).context("deleting value")?;
        wr.commit().context("commiting transaction")?;
        Ok(true)
    }
}

impl LmdbMap<UserIdentity, UserMeta> {
    pub fn advance_nonce(&self, identity: UserIdentity, new: u64) -> anyhow::Result<bool> {
        let wr = WriteTransaction::new(self.files.env()).context("creating transaction")?;
        {
            let mut access = wr.access();
            let mut cursor = wr.cursor(&self.files).context("creating cursor")?;
            let meta = cursor
                .overwrite_in_place::<_, AsBytes<UserMeta>>(
                    &mut access,
                    &identity,
                    put::Flags::empty(),
                )
                .context("accessing nonce")?;
            if meta.0.nonce + 1 != new {
                return Ok(false);
            }
            meta.0.nonce += 1;
        }

        wr.commit().context("commiting transaction")?;

        Ok(true)
    }
}

pub struct Storage {
    pub nodes: RwLock<NodeProfiles>,
    pub files: LmdbMap<Address, FileMeta>,
    pub users: LmdbMap<UserIdentity, UserMeta>,
}

impl Storage {
    pub(crate) fn new(metabase_root: &str) -> anyhow::Result<Self> {
        Ok(Self {
            nodes: NodeProfiles::load(metabase_root)?.into(),
            files: LmdbMap::new(metabase_root, "files")?,
            users: LmdbMap::new(metabase_root, "users")?,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub struct UserMeta {
    pub nonce: u64,
}

#[derive(Default, Codec, Clone, Copy)]
pub struct NodeProfile {
    pub identity: NodeIdentity,
    pub nonce: u64,
    pub last_requested_gc: u64, // unix timestamp seconds
    pub free_blocks: FreeSpace,
}

#[derive(Codec)]
pub struct BlockFill {
    free_pieces: u16,
}

impl Default for BlockFill {
    fn default() -> Self {
        Self { free_pieces: BLOCK_PIECES as _ }
    }
}

impl BlockFill {
    pub fn alloc(&mut self, size: usize) -> std::result::Result<u16, (u32, BlockFill)> {
        self.alloc_current(size).ok_or(Self::split_size(size))
    }

    fn alloc_current(&mut self, size: usize) -> Option<u16> {
        let len = size.div_ceil(PIECE_SIZE * DATA_PIECES);
        self.free_pieces = self.free_pieces.checked_sub(len as _)?;
        Some(self.free_pieces)
    }

    fn split_size(size: usize) -> (u32, BlockFill) {
        let free_pieces = (BLOCK_PIECES - size % DATA_PER_BLOCK / PIECE_SIZE) as _;
        (size.div_ceil(DATA_PER_BLOCK) as _, BlockFill { free_pieces })
    }
}

#[repr(packed)]
#[derive(Clone, Copy)]
struct AsBytes<T>(T);

impl<T> AsBytes<T> {
    fn of_ref(value: &T) -> &Self {
        unsafe { &*(value as *const T as *const Self) }
    }
}

unsafe impl<T: Copy> LmdbRaw for AsBytes<T> {}
