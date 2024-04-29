pub use chain_api::Client;
use {
    base64::Engine,
    chain_api::{Nonce, Profile},
    chat_spec::{username_to_raw, FetchProfileResp, UserName},
    codec::{Codec, DecodeOwned, Encode},
    std::{cell::RefCell, collections::BTreeMap, future::Future},
};

pub(crate) trait ChainClientExt {
    async fn fetch_profile(&self, name: UserName) -> Result<Profile, anyhow::Error>;
}

impl ChainClientExt for Client {
    async fn fetch_profile(&self, name: UserName) -> Result<Profile, anyhow::Error> {
        match Cache::get_or_insert_opt(username_to_raw(name), |name| self.get_profile_by_name(name))
            .await
        {
            Ok(Some(u)) => Ok(u),
            Ok(None) => anyhow::bail!("user {name} does not exist"),
            Err(e) => anyhow::bail!("failed to fetch user: {e}"),
        }
    }
}

pub fn min_nodes() -> usize {
    component_utils::build_env!(MIN_NODES);
    MIN_NODES.parse().unwrap()
}

pub struct Cache {
    hot_entries: BTreeMap<crypto::Hash, Vec<u8>>,
}

thread_local! {
    static INSTANCE: RefCell<Cache> = Cache {
        hot_entries: BTreeMap::new(),
    }.into();
}

impl Cache {
    pub async fn get_or_insert<K: AsRef<[u8]>, T: Cached, F: Future<Output = anyhow::Result<T>>>(
        key: K,
        or_compute: impl FnOnce(K) -> F,
    ) -> anyhow::Result<T> {
        let hash = crypto::hash::with_nonce(key.as_ref(), T::NONCE);

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
        E: std::error::Error + Send + Sync + 'static,
        F: Future<Output = Result<Option<T>, E>>,
    >(
        key: K,
        or_compute: impl FnOnce(K) -> F,
    ) -> anyhow::Result<Option<T>> {
        let hash = crypto::hash::with_nonce(key.as_ref(), T::NONCE);

        if let Some(res) = Self::get_low::<T>(hash) {
            return Ok(Some(res));
        }
        let res = or_compute(key).await?;
        if let Some(res) = &res {
            Self::insert_low(hash, res);
        }

        Ok(res)
    }

    pub fn get<T: Cached>(key: impl AsRef<[u8]>) -> Option<T> {
        Self::get_low::<T>(crypto::hash::with_nonce(key.as_ref(), T::NONCE))
    }

    pub fn insert<T: Cached>(key: impl AsRef<[u8]>, value: &T) {
        Self::insert_low(crypto::hash::with_nonce(key.as_ref(), T::NONCE), value);
    }

    fn get_low<T: Codec>(key: crypto::Hash) -> Option<T> {
        if let Some(entry) = INSTANCE.with(|i| i.borrow().hot_entries.get(&key).cloned())
            && let Some(res) = T::decode(&mut entry.as_slice())
        {
            return Some(res);
        }

        if let Some(win) = web_sys::window()
            && let Some(local_storage) = win.local_storage().unwrap()
            && let Some(value) = local_storage.get(&encode_base64(&key)).unwrap()
            && let Some(res) = decode_base64::<T>(&value)
        {
            INSTANCE.with(|i| i.borrow_mut().hot_entries.insert(key, res.to_bytes()));
            return Some(res);
        }

        None
    }

    pub fn insert_low<T: Codec>(key: crypto::Hash, value: &T) {
        INSTANCE.with(|i| i.borrow_mut().hot_entries.insert(key, value.to_bytes()));

        if let Some(win) = web_sys::window()
            && let Some(local_storage) = win.local_storage().unwrap()
        {
            local_storage.set(&encode_base64(&key), &encode_base64(&value)).unwrap();
        }
    }
}

pub trait Cached: Codec {
    const NONCE: Nonce;
}

impl Cached for Profile {
    const NONCE: Nonce = 0;
}

impl Cached for FetchProfileResp {
    const NONCE: Nonce = 1;
}

impl Cached for crypto::enc::PublicKey {
    const NONCE: Nonce = 2;
}

impl Cached for UserName {
    const NONCE: Nonce = 3;
}

fn encode_base64<T: Encode>(t: &T) -> String {
    base64::prelude::BASE64_STANDARD_NO_PAD.encode(&t.to_bytes())
}

fn decode_base64<T: DecodeOwned>(s: &str) -> Option<T> {
    base64::prelude::BASE64_STANDARD_NO_PAD
        .decode(s)
        .ok()
        .and_then(|b| T::decode(&mut b.as_slice()))
}
