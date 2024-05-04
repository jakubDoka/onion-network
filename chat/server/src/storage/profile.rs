use {
    anyhow::Context,
    chat_spec::{ChatError, Identity},
    codec::{Codec, Decode, Encode},
    crypto::{
        enc,
        proof::{Nonce, Proof},
        sign,
    },
    lru::LruCache,
    std::{
        fs,
        io::{Read, Seek, Write},
        os::unix::fs::FileExt,
        path::PathBuf,
        sync::{Arc, Mutex},
    },
};

const PROFILE_DIR: &str = "profiles/";
const MAILBOX_PATH: &str = "mailbox.bin";
const ROOT_PATH: &str = "root.bin";
const VAULT: &str = "vault.lmdb";

pub type Storage = Mutex<LruCache<Identity, Arc<Handle>>>;

pub fn new() -> Storage {
    Mutex::new(LruCache::new(512.try_into().unwrap()))
}

#[derive(Codec, Clone, Copy)]
#[repr(packed)]
pub struct Root {
    pub vault_version: Nonce,
    pub mail_action: Nonce,
    pub sign: sign::PublicKey,
    pub enc: enc::PublicKey,
    pub vault_sig: sign::Signature,
    pub mail_sig: sign::Signature,
    pub vault_root: crypto::Hash,
}

impl Root {
    pub fn as_bytes(&self) -> &[u8; std::mem::size_of::<Root>()] {
        unsafe { std::mem::transmute(self) }
    }

    pub fn is_valid(&self) -> bool {
        let mail = Proof {
            pk: self.sign,
            nonce: self.mail_action,
            signature: self.mail_sig,
            context: chat_spec::Mail::new(),
        };
        let vault = Proof {
            pk: self.sign,
            nonce: self.vault_version,
            signature: self.vault_sig,
            context: self.vault_root,
        };
        mail.verify() && vault.verify()
    }
}

pub struct Handle {
    vault: lmdb_zero::Database<'static>,
    root_file: Mutex<std::fs::File>,
    mailbox: Mutex<std::fs::File>,
}

impl Handle {
    pub fn load(mut path: PathBuf, id: Identity) -> Result<Self, ChatError> {
        path.push(PROFILE_DIR);
        super::push_id(&mut path, &id)?;

        if !path.exists() {
            return Err(ChatError::NotFound);
        }

        path.push(MAILBOX_PATH);
        let mut mailbox = std::fs::OpenOptions::new().read(true).write(true).open(&path)?;
        mailbox.seek(std::io::SeekFrom::End(0))?;
        path.pop();

        path.push(ROOT_PATH);
        let root_file = std::fs::OpenOptions::new().read(true).write(true).open(&path)?;
        path.pop();

        path.push(VAULT);
        let vault = super::open_lmdb(&path).context("opening vault")?;
        path.pop();

        Ok(Self { vault, root_file: root_file.into(), mailbox: mailbox.into() })
    }

    pub fn vault_hashes(&self) -> anyhow::Result<Vec<[crypto::Hash; 2]>> {
        let ro = lmdb_zero::ReadTransaction::new(self.vault.env())?;
        let iter = ro.cursor(&self.vault)?;

        let v = lmdb_zero::CursorIter::new(
            lmdb_zero::MaybeOwned::Owned(iter),
            &ro.access(),
            lmdb_zero::Cursor::first::<Identity, [u8]>,
            lmdb_zero::Cursor::next::<Identity, [u8]>,
        )?
        .map(|r| r.map(|(k, v)| [*k, crypto::hash::kv(k, v)]))
        .collect::<lmdb_zero::Result<Vec<_>>>()
        .context("failed to iterate over vault");
        v
    }

    pub fn insert_to_vault(&self, changes: Vec<(Identity, Vec<u8>)>) -> Result<(), ChatError> {
        let wr = lmdb_zero::WriteTransaction::new(self.vault.env())
            .context("opening write transaction")?;

        for (k, v) in changes {
            wr.access()
                .put(&self.vault, &k, &v, lmdb_zero::put::Flags::empty())
                .context("putting key")?;
        }

        wr.commit().context("commiting")?;
        Ok(())
    }

    pub fn advance_vault_nonce(&self, new: Nonce) -> Result<(), ChatError> {
        self.advancee_nonce(0, new)
    }

    pub fn advance_mail_nonce(&self, new: Nonce) -> Result<(), ChatError> {
        self.advancee_nonce(std::mem::size_of::<Nonce>() as u64, new)
    }

    fn advancee_nonce(&self, offset: u64, new: Nonce) -> Result<(), ChatError> {
        let mut nonce = [0u8; std::mem::size_of::<Nonce>()];
        let file = self.root_file.lock().unwrap();
        file.read_exact_at(&mut nonce, offset)?;
        let nonce = Nonce::from_ne_bytes(nonce);
        if new <= nonce {
            return Err(ChatError::InvalidAction(new));
        }
        file.write_all_at(&new.to_ne_bytes(), offset)?;
        Ok(())
    }

    pub fn create(mut path: PathBuf, root: Root, can_owerride: bool) -> Result<(), ChatError> {
        path.push(PROFILE_DIR);
        super::push_id(&mut path, &root.sign.identity())?;

        if path.exists() {
            if !can_owerride {
                return Err(ChatError::AlreadyExists);
            }

            fs::remove_dir_all(&path).context("removing profile directory")?;
        }
        fs::create_dir_all(&path).context("creating profile directory")?;

        path.push(ROOT_PATH);
        fs::write(&path, root.as_bytes()).context("writing root file")?;
        path.pop();

        path.push(MAILBOX_PATH);
        fs::write(&path, &[]).context("writing mailbox file")?;
        path.pop();

        path.push(VAULT);
        fs::create_dir(&path).context("writing vault file")?;
        path.pop(); // maybe we ll add more

        Ok(())
    }

    pub fn read_mail(&self) -> Result<Vec<u8>, ChatError> {
        let mut buf = Vec::new();
        let mut mailbox = self.mailbox.lock().unwrap();
        mailbox.seek(std::io::SeekFrom::Start(0))?;
        mailbox.read_to_end(&mut buf)?;
        mailbox.set_len(0)?;
        Ok(buf)
    }

    pub fn load_keys(&self) -> Result<chat_spec::FetchProfileResp, ChatError> {
        unsafe { self.read_chunk_from_root(std::mem::size_of::<Nonce>() as u64 * 2) }
    }

    pub fn nonces(&self) -> Result<[Nonce; 2], ChatError> {
        unsafe { self.read_chunk_from_root(0) }
    }

    pub fn root(&self) -> Result<Root, ChatError> {
        unsafe { self.read_chunk_from_root(0) }
    }

    unsafe fn read_chunk_from_root<T>(&self, offset: u64) -> Result<T, ChatError> {
        let mut value = std::mem::zeroed::<T>();
        let root = self.root_file.lock().unwrap();
        let slice = std::slice::from_raw_parts_mut(
            &mut value as *mut T as *mut u8,
            std::mem::size_of::<T>(),
        );
        slice.fill(0xff);
        root.read_exact_at(slice, offset)?;
        Ok(value)
    }

    pub fn fetch_vault(&self) -> Result<Vec<u8>, ChatError> {
        let mut buf = Vec::new();
        self.fetch_vault_low(&mut buf)?;
        Ok(buf)
    }

    pub fn fetch_vault_low(&self, to: &mut Vec<u8>) -> Result<(), ChatError> {
        let ro =
            lmdb_zero::ReadTransaction::new(self.vault.env()).context("opening transaction")?;

        let iter = ro.cursor(&self.vault).context("opening cursor")?;

        for r in lmdb_zero::CursorIter::new(
            lmdb_zero::MaybeOwned::Owned(iter),
            &ro.access(),
            lmdb_zero::Cursor::first::<Identity, [u8]>,
            lmdb_zero::Cursor::next::<Identity, [u8]>,
        )
        .context("initiating iterator")?
        {
            _ = r.context("reading kv")?.encode(to);
        }

        Ok(())
    }

    pub fn append_mail(&self, mail: &[u8]) -> Result<(), ChatError> {
        handlers::ensure!(!mail.is_empty(), ChatError::PleaseDont);
        let mut mailbox = self.mailbox.lock().unwrap();
        let next_len = mailbox.metadata()?.len() as usize + mail.len();
        handlers::ensure!(next_len <= chat_spec::MAIL_BOX_CAP, ChatError::MailboxFull);
        mailbox.write_all(&(mail.len() as u16).to_be_bytes())?;
        mailbox.write_all(mail)?;
        Ok(())
    }

    pub fn recover_vault(&self, vault: Vec<u8>) -> Result<(), ChatError> {
        let wr = lmdb_zero::WriteTransaction::new(self.vault.env())
            .context("opening recovery transaction")?;

        let mut access = wr.access();
        access.clear_db(&self.vault).context("clearing vault")?;

        let mut buffer = vault.as_slice();
        while let Some((id, val)) = <(Identity, &[u8])>::decode(&mut buffer) {
            access
                .put(&self.vault, &id, val, lmdb_zero::put::Flags::empty())
                .context("recovering key")?;
        }

        drop(access);

        wr.commit().context("commiting").map_err(Into::into)
    }
}

pub(crate) fn exists(mut root: PathBuf, profile: Identity) -> bool {
    root.push(PROFILE_DIR);
    _ = super::push_id(&mut root, &profile).inspect_err(|e| log::error!("failed to push id: {e}"));
    root.exists()
}
