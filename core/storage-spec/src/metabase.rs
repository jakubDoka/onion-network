use {crate::MAX_PIECES, disk_pool::Record, std::ops::Range};

pub mod disk_pool;

pub const BLOCK_CHUNK_SIZE: usize = 4;

macro_rules! decl_db {
    (
        $cx_name:ident, $db_name:ident;
        $($field:ident: $id:ident, $cx:ident, $db:ident, $elem:ty, $id_repr:ty, $element:ty;)*
    ) => {
        $(
            pub type $id = $id_repr;
            pub type $cx = disk_pool::Context<$id>;
            pub type $db = disk_pool::Db<$id, $element>;
        )*

        pub struct $cx_name {
            $(
                pub $field: std::sync::Mutex<$cx>,
            )*
        }

        impl $cx_name {
            pub fn open(root_dir: &std::path::Path) -> std::io::Result<Self> {
                Ok(Self {$(
                    $field: {
                        let root_dir = root_dir.join(stringify!($id));
                        std::fs::create_dir_all(&root_dir)?;
                        std::sync::Mutex::new(disk_pool::Context::open(&root_dir)?)
                    },
                )*})
            }

            pub fn save(&self, root_dir: &std::path::Path) -> std::io::Result<()> {
                $(
                    self.$field.lock().unwrap().save(&root_dir.join(stringify!($id)))?;
                )*
                Ok(())
            }
        }

        pub struct $db_name {
            $(
                pub $field: $db,
            )*
        }

        impl $db_name {
            pub fn open(root_dir: &std::path::Path) -> std::io::Result<Self> {
                Ok(Self {$(
                    $field: {
                        let root_dir = root_dir.join(stringify!($id));
                        std::fs::create_dir_all(&root_dir)?;
                        disk_pool::Db::open(&root_dir)?
                    },
                )*})
            }
        }

        $(
            unsafe impl Record for $element {
                const EXT: &'static str = stringify!($field);
                type Elem = $elem;
            }
        )*
    };
}

decl_db! {
    DbContext, Db;
    stores: StoreId, StoreCx, StoreDb, u16, u16, Store;
    blocks: BlockId, BlockCx, BlockDb, u32, u32, Block;
    files: FileId, FileCx, FileDb, u64, u64, FileMeta;
    block_lists: BlockListId, BlockListCx, BlockListDb, u32, u32, BlockChunk;
}

pub type StoreIdentity = [u8; 32];
pub type BlockHolders = [StoreId; MAX_PIECES];
pub type BlockChunk = [BlockId; BLOCK_CHUNK_SIZE];

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Block {
    pub free_space: u16,
    pub group: BlockHolders,
}

#[derive(Clone)]
#[repr(C)]
pub struct Store {
    pub identity: StoreIdentity,
    pub block_cunks: Range<BlockListId>,
    pub block_count: u32,
}

#[derive(Clone)]
#[repr(C)]
pub struct FileMeta {
    pub in_block_start: u32,
    pub in_block_end: u32,
    pub piece_count: u64,
    pub blocks: Range<BlockListId>,
    pub block_count: u32,
}
