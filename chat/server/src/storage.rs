use {
    base64::Engine,
    chat_spec::{ChatError, ChatName, Identity, Member},
    crypto::proof::Proof,
    merkle_tree::MerkleTree,
    std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, Mutex},
    },
};

pub mod chat;
pub mod profile;

#[repr(transparent)]
struct DontCare<T>(T);

impl<T> lmdb_zero::traits::AsLmdbBytes for DontCare<T> {
    fn as_lmdb_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self as *const _ as *const u8, std::mem::size_of::<Self>())
        }
    }
}

impl<T> lmdb_zero::traits::FromLmdbBytes for DontCare<T> {
    fn from_lmdb_bytes(bytes: &[u8]) -> Result<&Self, String> {
        let size = std::mem::size_of::<Self>();
        if bytes.len() != size {
            return Err(format!("Expected {} bytes, got {}", size, bytes.len()));
        }
        Ok(unsafe { &*(bytes.as_ptr() as *const _) })
    }
}

impl<T> lmdb_zero::traits::FromReservedLmdbBytes for DontCare<T> {
    unsafe fn from_reserved_lmdb_bytes(bytes: &mut [u8]) -> &mut Self {
        &mut *(bytes.as_mut_ptr() as *mut _)
    }
}

fn push_id(root: &mut PathBuf, id: &[u8]) -> anyhow::Result<()> {
    let mut id_buf = [0u8; 64];
    let len = base64::prelude::BASE64_URL_SAFE_NO_PAD.encode_slice(id, &mut id_buf)?;
    root.push(std::str::from_utf8(&id_buf[..len])?);
    Ok(())
}

pub struct Storage {
    root: PathBuf,
    profiles: profile::Storage,
    chats: chat::Storage,
    creation_lock: Mutex<()>,
}

impl Storage {
    pub fn new(root: PathBuf) -> Self {
        Self { root, profiles: profile::new(), chats: chat::new(), creation_lock: Mutex::new(()) }
    }

    pub fn get_chat(&self, id: ChatName) -> Result<Arc<chat::Handle>, ChatError> {
        self.chats
            .lock()
            .unwrap()
            .try_get_or_insert(id, || chat::Handle::load(self.root.clone(), id).map(Arc::new))
            .cloned()
    }

    pub fn get_profile(&self, id: Identity) -> Result<Arc<profile::Handle>, ChatError> {
        self.profiles
            .lock()
            .unwrap()
            .try_get_or_insert(id, || profile::Handle::load(self.root.clone(), id).map(Arc::new))
            .cloned()
    }

    pub fn has_profile(&self, profile: Identity) -> bool {
        profile::exists(self.root.clone(), profile)
    }

    pub fn has_chat(&self, chat: ChatName) -> bool {
        chat::exists(self.root.clone(), chat)
    }

    pub fn create_profile(
        &self,
        proof: Proof<crypto::Hash>,
        enc: crypto::enc::PublicKey,
    ) -> Result<(), ChatError> {
        handlers::ensure!(proof.nonce == 0, ChatError::InvalidAction(0));
        handlers::ensure!(proof.context == crypto::Hash::default(), ChatError::InvalidProofContext);
        handlers::ensure!(!self.has_profile(proof.identity()), ChatError::AlreadyExists);
        handlers::ensure!(proof.verify(), ChatError::InvalidProof);

        let root = profile::Root {
            sign: proof.pk,
            enc,
            mail_action: 0,
            vault_version: 0,
            vault_sig: proof.signature,
            mail_sig: proof.signature,
            vault_root: crypto::Hash::default(),
        };

        let _lock = self.creation_lock.lock().unwrap();
        profile::Handle::create(self.root.clone(), root, false)
    }

    pub fn create_chat(&self, name: ChatName, identity: Identity) -> Result<(), ChatError> {
        {
            let _lock = self.creation_lock.lock().unwrap();
            chat::Handle::create(self.root.clone(), name)?;
        }

        self.get_chat(name)?.create_member(identity, Member::best())
    }

    pub(crate) fn insert_to_vault(
        &self,
        identity: Identity,
        changes: Vec<(crypto::Hash, Vec<u8>)>,
        proof: Proof<crypto::Hash>,
    ) -> Result<(), ChatError> {
        handlers::ensure!(
            changes.iter().all(|(_, v)| v.len() <= chat_spec::MAX_VAULT_VALUE_SIZE),
            ChatError::ValueTooLarge
        );

        let pf = self.get_profile(identity)?;

        'verify_root: {
            let mut hashes = pf
                .vault_hashes()?
                .into_iter()
                .filter_map(|[k, h]| changes.iter().all(|&(k2, _)| k != k2).then(|| h))
                .chain(changes.iter().map(|(k, val)| crypto::hash::kv(k, val)))
                .collect::<Vec<_>>();
            handlers::ensure!(
                hashes.len() < chat_spec::MAX_VAULT_KEY_COUNT,
                ChatError::TooManyKeys
            );
            hashes.sort_unstable();
            hashes.dedup();
            let hashes = hashes.into_iter().collect::<MerkleTree<_>>();
            handlers::ensure!(*hashes.root() == proof.context, ChatError::InvalidProofContext);
        }

        pf.advance_vault_nonce(proof.nonce)?;
        pf.insert_to_vault(changes)
    }

    pub fn read_mail(&self, proof: Proof<chat_spec::Mail>) -> Result<Vec<u8>, ChatError> {
        let pf = self.get_profile(proof.identity())?;
        handlers::ensure!(proof.verify(), ChatError::InvalidProof);
        pf.advance_mail_nonce(proof.nonce)?;
        pf.read_mail()
    }

    pub fn recover_profile(&self, profile: profile::Root, vault: Vec<u8>) -> Result<(), ChatError> {
        profile::Handle::create(self.root.clone(), profile, true)?;
        self.get_profile(profile.sign.identity())?.recover_vault(vault)
    }

    pub fn remove_chat(&self, name: ChatName) -> Result<(), ChatError> {
        chat::Handle::remove(self.root.clone(), name)
    }

    pub fn recover_chat(
        &self,
        name: ChatName,
        chat_data: HashMap<Identity, Member>,
    ) -> Result<(), ChatError> {
        chat::Handle::create(self.root.clone(), name)?;
        self.get_chat(name)?.recover_members(chat_data)
    }
}

fn open_lmdb(path: &PathBuf) -> lmdb_zero::Result<lmdb_zero::Database<'static>> {
    let path = path.to_str().ok_or(lmdb_zero::Error::Mismatch)?;
    let flags = lmdb_zero::open::Flags::empty();
    let lmdb = unsafe { lmdb_zero::EnvBuilder::new()?.open(&path, flags, 0o600)? };
    lmdb_zero::Database::open(lmdb, None, &lmdb_zero::DatabaseOptions::defaults())
}
