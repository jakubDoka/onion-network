use {
    anyhow::Context,
    codec::Codec,
    crypto::proof::Nonce,
    lmdb_zero::{
        open, put, traits::LmdbRaw, Cursor, CursorIter, DatabaseOptions, LmdbResultExt, MaybeOwned,
        ReadTransaction, WriteTransaction,
    },
    rand_core::OsRng,
    std::{
        collections::{hash_map::Entry, HashMap},
        net::SocketAddr,
        sync::RwLock,
    },
    storage_spec::*,
};

const GC_COOLDOWN: u64 = 60 * 60;

// TODO: use lmdb instead
#[derive(Default)]
pub struct NodeProfiles {
    records: Vec<NodeProfile>,
    identity_to_id: HashMap<NodeIdentity, NodeId>,
    free_space_index: Vec<(FreeSpace, NodeId)>,
}

impl<'a> Codec<'a> for NodeProfiles {
    fn encode(&self, buffer: &mut impl codec::Buffer) -> Option<()> {
        self.records.encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let records = Vec::<NodeProfile>::decode(buffer)?;
        let identity_to_id =
            records.iter().enumerate().map(|(i, p)| (p.identity, i as _)).collect();
        let free_space_index =
            records.iter().enumerate().map(|(i, p)| (p.free_blocks, i as _)).collect();

        Some(Self { records, identity_to_id, free_space_index })
    }
}

impl NodeProfiles {
    fn load(metabase_root: &str) -> anyhow::Result<Self> {
        let path = std::path::Path::new(metabase_root).join("node_profiles");
        let bytes = std::fs::read(path)?;
        Self::decode(&mut &bytes[..]).context("failed to decode node profiles")
    }

    pub fn register_node(
        &mut self,
        identity: NodeIdentity,
        pre_identity: NodeIdentity,
        addr: SocketAddr,
        free_blocks: FreeSpace,
    ) -> bool {
        let id = self.records.len() as NodeId;
        let Entry::Vacant(entry) = self.identity_to_id.entry(identity) else {
            return false;
        };

        entry.insert(id);
        self.free_space_index.push((free_blocks, id));
        self.records.push(NodeProfile {
            identity,
            free_blocks,
            pre_identity,
            nonce: 0,
            our_nonce: 0,
            last_requested_gc: 0,
            addr,
        });

        true
    }

    pub fn request_gc(&mut self, identity: NodeIdentity, nonce: Nonce) -> Option<NodeId> {
        let current_time =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

        let id = *self.identity_to_id.get(&identity)?;
        let profile = &mut self.records[id as usize];

        if current_time - profile.last_requested_gc < GC_COOLDOWN || profile.nonce + 1 != nonce {
            return None;
        }

        profile.last_requested_gc = current_time;
        profile.nonce += 1;

        Some(id)
    }

    pub fn allocate_file(&mut self, size: u64) -> Option<(Address, Holders)> {
        let alloc_size = size.div_ceil(DATA_PIECES as _);

        let (_, most_free) = self.free_space_index.split_last_chunk_mut::<MAX_PIECES>()?;
        most_free.iter().any(|(free, _)| *free < alloc_size).then_some(())?;
        for (free, id) in &mut *most_free {
            *free -= alloc_size;
            self.records[*id as usize].free_blocks -= alloc_size;
        }

        let id = crypto::new_secret(OsRng);
        let holders = most_free.map(|(_, id)| id);
        Some((Address { id, size }, holders))
    }

    pub fn expand_holders(&self, holders: [NodeId; MAX_PIECES]) -> [NodeIdentity; MAX_PIECES] {
        holders.map(|id| self.records[id as usize].identity)
    }

    pub fn update_addr(&mut self, identity: NodeIdentity, nonce: Nonce, addr: SocketAddr) -> bool {
        let Some(&id) = self.identity_to_id.get(&identity) else {
            return false;
        };
        let profile = &mut self.records[id as usize];

        if profile.nonce + 1 != nonce {
            return false;
        }

        profile.nonce += 1;
        profile.addr = addr;

        true
    }
}

pub struct LmdbMap<K, V> {
    db: lmdb_zero::Database<'static>,
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
            db: lmdb_zero::Database::open(env, None, &DatabaseOptions::defaults())?,
            phantom: Default::default(),
        })
    }

    pub fn save(&self, k: K, v: V) -> anyhow::Result<bool> {
        let wr = WriteTransaction::new(self.db.env()).context("creating transaction")?;
        let res = wr
            .access()
            .put(&self.db, AsBytes::of_ref(&k), AsBytes::of_ref(&v), put::NOOVERWRITE)
            .map(|()| true)
            .ignore_exists(false)
            .context("putting value")?;
        wr.commit().context("commiting transaction")?;
        Ok(res)
    }

    pub fn load(&self, address: Address) -> anyhow::Result<Option<FileMeta>> {
        let rd = ReadTransaction::new(self.db.env()).context("creating transaction")?;
        let res = rd
            .access()
            .get(&self.db, AsBytes::of_ref(&address))
            .to_opt()
            .context("getting value")?
            .map(|v: &AsBytes<_>| v.0);
        Ok(res)
    }

    pub fn delete(&self, address: K, guard: impl FnOnce(V) -> bool) -> anyhow::Result<bool> {
        let wr = WriteTransaction::new(self.db.env()).context("creating transaction")?;
        if !wr
            .access()
            .get::<_, AsBytes<V>>(&self.db, AsBytes::of_ref(&address))
            .to_opt()
            .context("checking owner")?
            .is_some_and(|v| guard(v.0))
        {
            return Ok(false);
        };
        wr.access().del_key(&self.db, AsBytes::of_ref(&address)).context("deleting value")?;
        wr.commit().context("commiting transaction")?;
        Ok(true)
    }
}

impl LmdbMap<Address, FileMeta> {
    pub fn get_files_for(&self, node: NodeId) -> anyhow::Result<Vec<Address>> {
        // FIXME: find more scalable solution
        let rd = ReadTransaction::new(self.db.env()).context("creating transaction")?;
        let cursor = rd.cursor(&self.db).context("creating cursor")?;

        let res = CursorIter::new(
            MaybeOwned::Owned(cursor),
            &rd.access(),
            Cursor::first::<AsBytes<Address>, AsBytes<FileMeta>>,
            Cursor::next,
        )?
        .filter_map(Result::ok)
        .filter(|(_, v)| { v.0.holders }.contains(&node))
        .map(|(&AsBytes(k), _)| k)
        .collect();

        Ok(res)
    }
}

impl LmdbMap<UserIdentity, UserMeta> {
    pub fn advance_nonce(&self, identity: UserIdentity, new: u64) -> anyhow::Result<bool> {
        let wr = WriteTransaction::new(self.db.env()).context("creating transaction")?;
        {
            let mut access = wr.access();
            let mut cursor = wr.cursor(&self.db).context("creating cursor")?;
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
    pub nonce: Nonce,
}

#[derive(Codec, Clone, Copy)]
pub struct NodeProfile {
    pub identity: NodeIdentity,
    pub pre_identity: NodeIdentity,
    pub nonce: Nonce,
    pub our_nonce: Nonce,
    pub last_requested_gc: u64, // unix timestamp seconds
    pub free_blocks: FreeSpace,
    pub addr: SocketAddr,
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
