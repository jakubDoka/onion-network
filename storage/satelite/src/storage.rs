use {
    lmdb_zero::{
        traits::{AsLmdbBytes, FromLmdbBytes, LmdbRaw},
        DatabaseOptions,
    },
    storage_spec::NodeIdentity,
};

pub struct Storage {
    db: lmdb_zero::Database<'static>,
}

impl Storage {
    pub fn new(root: &str) -> lmdb_zero::Result<Self> {
        let env = unsafe {
            lmdb_zero::EnvBuilder::new()?.open(root, lmdb_zero::open::Flags::empty(), 0o600)?
        };
        let db = lmdb_zero::Database::open(env, None, &DatabaseOptions::defaults())?;
        Ok(Self { db })
    }

    pub fn register(&self, identity: NodeIdentity) -> lmdb_zero::Result<bool> {
        let wt = lmdb_zero::WriteTransaction::new(self.db.env())?;
        {
            let mut access = wt.access();
            match access.get::<_, [u8]>(&self.db, &identity) {
                Ok(_) => return Ok(false),
                Err(lmdb_zero::error::Error::Code(lmdb_zero::error::NOTFOUND)) => (),
                Err(e) => return Err(e),
            };
            access.put(
                &self.db,
                &identity,
                &NodeProfile::default(),
                lmdb_zero::put::Flags::empty(),
            )?;
        }
        wt.commit()?;

        Ok(true)
    }

    pub fn request_gc(&self, identity: NodeIdentity) -> lmdb_zero::Result<Option<NodeProfile>> {
        lmdb_zero::ReadTransaction::new(self.db.env())?
            .access()
            .get::<_, NodeProfile>(&self.db, &identity)
            .map(Some)
            .or_else(|e| match e {
                lmdb_zero::error::Error::Code(lmdb_zero::error::NOTFOUND) => Ok(None),
                e => Err(e),
            })
            .map(Option::<&_>::cloned)
    }
}

#[repr(packed)]
#[derive(Default, Clone, Copy)]
pub struct NodeProfile {
    pub nonce: u64,
    pub last_requested_gc: u64, // unix timestamp seconds
}

unsafe impl LmdbRaw for NodeProfile {}
