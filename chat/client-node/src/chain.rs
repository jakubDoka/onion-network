pub use chain_api::Client;
use {
    crate::{VaultChanges, VaultKeys, VaultValue, VaultVersion},
    anyhow::Context,
    chain_api::{Nonce, Profile},
    chat_spec::{username_from_raw, username_to_raw, FetchProfileResp, Identity, UserName},
    codec::{Codec, Encode},
    libp2p::futures::TryFutureExt,
    std::{collections::BTreeMap, future::Future, sync::Mutex},
};

#[allow(async_fn_in_trait)]
pub trait ChainClientExt {
    async fn fetch_profile(&self, name: UserName) -> Result<Profile, anyhow::Error>;
    async fn fetch_username(&self, id: Identity) -> Result<UserName, anyhow::Error>;
}

impl ChainClientExt for Client {
    async fn fetch_profile(&self, name: UserName) -> Result<Profile, anyhow::Error> {
        match Storage::get_or_insert_opt(username_to_raw(name), |name| {
            self.get_profile_by_name(name).map_err(anyhow::Error::from)
        })
        .await
        {
            Ok(Some(u)) => Ok(u),
            Ok(None) => anyhow::bail!("user {name} does not exist"),
            Err(e) => anyhow::bail!("failed to fetch user: {e}"),
        }
    }

    async fn fetch_username(&self, id: Identity) -> Result<UserName, anyhow::Error> {
        match Storage::get_or_insert_opt(id, |id| {
            self.get_username(id).map_err(anyhow::Error::from)
        })
        .await
        {
            Ok(Some(u)) => username_from_raw(u).context("invalid name"),
            Ok(None) => anyhow::bail!("user {id:?} does not exist"),
            Err(e) => anyhow::bail!("failed to fetch user: {e}"),
        }
    }
}

pub fn min_nodes() -> usize {
    chat_spec::REPLICATION_FACTOR.get() + 1
}

pub trait StorageBackend: Send + Sync {
    fn load(&mut self, key: crypto::Hash) -> Option<Vec<u8>>;
    fn store(&mut self, key: crypto::Hash, value: &[u8]);
}

pub struct Storage {
    id: Identity,
    hot_entries: BTreeMap<crypto::Hash, Vec<u8>>,
    backend: Option<Box<dyn StorageBackend>>,
}

fn with_instance<T>(f: impl FnOnce(&mut Storage) -> T) -> T {
    static INSTANCE: Mutex<Option<Storage>> = Mutex::new(None);
    let mut r = INSTANCE.lock().unwrap();
    f(r.get_or_insert(Storage {
        id: Identity::default(),
        hot_entries: BTreeMap::new(),
        backend: None,
    }))
}

impl Storage {
    pub fn set_id(id: Identity) {
        with_instance(|i| i.id = id);
    }

    pub fn set_backend(impl_: impl StorageBackend + 'static) {
        with_instance(|i| i.backend = Some(Box::new(impl_)));
    }

    pub async fn get_or_insert<K: AsRef<[u8]>, T: Cached, F: Future<Output = anyhow::Result<T>>>(
        key: K,
        or_compute: impl FnOnce(K) -> F,
    ) -> anyhow::Result<T> {
        let hash = Self::compute_key::<T>(&key);

        if let Some(res) = Self::get_low::<T>(hash) {
            return Ok(res);
        }
        let res = or_compute(key).await?;
        Self::insert_low(hash, &res);

        Ok(res)
    }

    pub async fn get_or_insert_opt<
        K: AsRef<[u8]>,
        T: Cached,
        F: Future<Output = anyhow::Result<Option<T>>>,
    >(
        key: K,
        or_compute: impl FnOnce(K) -> F,
    ) -> anyhow::Result<Option<T>> {
        let hash = Self::compute_key::<T>(&key);

        if let Some(res) = Self::get_low::<T>(hash) {
            return Ok(Some(res));
        }
        let res = or_compute(key).await?;
        if let Some(res) = &res {
            Self::insert_low(hash, res);
        }

        Ok(res)
    }

    pub fn ensure<T: Cached>(friends: impl AsRef<[u8]>, key: &T) -> bool {
        let hash = Self::compute_key::<T>(friends);
        if Self::get_low::<T>(hash).is_none() {
            Self::insert_low(hash, key);
            true
        } else {
            false
        }
    }

    pub fn get<T: Cached>(key: impl AsRef<[u8]>) -> Option<T> {
        Self::get_low::<T>(Self::compute_key::<T>(key))
    }

    pub fn insert<T: Cached>(key: impl AsRef<[u8]>, value: &T) {
        Self::insert_low(Self::compute_key::<T>(key), value);
    }

    pub fn update<T: Cached + Default>(key: impl AsRef<[u8]>, f: impl FnOnce(&mut T)) {
        let hash = Self::compute_key::<T>(key);
        let mut current: T = Self::get_low(hash).unwrap_or_default();
        let bytes = current.to_bytes();
        f(&mut current);
        if current.to_bytes() != bytes {
            Self::insert_low(hash, &current);
        }
    }

    fn compute_key<T: Cached>(key: impl AsRef<[u8]>) -> crypto::Hash {
        let id = with_instance(|i| i.id);
        debug_assert_ne!(id, Identity::default());

        crypto::xor_secrets(crypto::hash::with_nonce(key.as_ref(), T::NONCE), id)
    }

    fn get_low<T: Codec>(key: crypto::Hash) -> Option<T> {
        with_instance(|i| {
            if let Some(res) = i.hot_entries.get(&key).and_then(|v| T::decode_exact(v)) {
                return Some(res);
            }

            if let Some(be) = i.backend.as_mut() {
                if let Some(value) = be.load(key) {
                    return T::decode_exact(&value);
                }
            }

            None
        })
    }

    pub fn insert_low<T: Encode>(key: crypto::Hash, value: &T) {
        let bytes = value.to_bytes();
        with_instance(|i| {
            if let Some(be) = i.backend.as_mut() {
                be.store(key, &bytes);
            }
            i.hot_entries.insert(key, bytes);
        });
    }
}

pub trait Cached: Codec {
    const NONCE: Nonce;
}

macro_rules! impl_cached {
    ($($t:ty,)*) => { $(impl Cached for $t { const NONCE: Nonce = ${index(0)}; })* };
}

impl_cached! {
    Profile,
    FetchProfileResp,
    crypto::enc::PublicKey,
    UserName,
    VaultValue,
    VaultKeys,
    VaultChanges,
    VaultVersion,
    Identity,
}

#[cfg(target_arch = "wasm32")]
pub struct LocalStorageCache;

#[cfg(target_arch = "wasm32")]
impl StorageBackend for LocalStorageCache {
    fn load(&mut self, key: crypto::Hash) -> Option<Vec<u8>> {
        web_sys::window()?
            .local_storage()
            .unwrap()?
            .get(&encode_base64(&key))
            .unwrap()
            .and_then(|v| decode_base64(&v))
    }

    fn store(&mut self, key: crypto::Hash, value: &[u8]) {
        let store = || {
            web_sys::window()?
                .local_storage()
                .unwrap()?
                .set(&encode_base64(&key), &encode_base64(value))
                .unwrap();
            Some(())
        };

        store();
    }
}

#[cfg(target_arch = "wasm32")]
fn encode_base64(t: &[u8]) -> String {
    use base64::Engine;
    base64::prelude::BASE64_STANDARD_NO_PAD.encode(t)
}

#[cfg(target_arch = "wasm32")]
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::prelude::BASE64_STANDARD_NO_PAD.decode(s).ok()
}
