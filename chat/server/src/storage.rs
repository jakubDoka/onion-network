use {
    chat_spec::{ChatError, Identity},
    crypto::proof::Proof,
    merkle_tree::MerkleTree,
    std::{path::PathBuf, sync::Arc},
};

pub mod profile;

pub struct Storage {
    root: PathBuf,
    profiles: profile::Storage,
}

impl Storage {
    pub fn new(root: PathBuf) -> Self {
        Self { root, profiles: profile::new() }
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

        profile::Handle::create(self.root.clone(), root, false)
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

    pub(crate) fn recover_profile(
        &self,
        profile: profile::Root,
        vault: Vec<u8>,
    ) -> Result<(), ChatError> {
        profile::Handle::create(self.root.clone(), profile, true)?;
        self.get_profile(profile.sign.identity())?.recover_vault(vault)
    }
}
