use {
    crate::Storage,
    anyhow::Context,
    chain_api::encrypt,
    chat_spec::*,
    codec::{Codec, Decode, DecodeOwned, Encode},
    crypto::proof::Nonce,
    double_ratchet::DoubleRatchet,
    merkle_tree::MerkleTree,
    onion::SharedSecret,
    rand::rngs::OsRng,
    std::collections::{BTreeSet, HashMap, HashSet},
    web_sys::wasm_bindgen::JsValue,
};

pub type FriendId = SharedSecret;

#[derive(Codec, PartialEq, Eq)]
pub struct VaultValue(Vec<u8>);

#[derive(Codec, Default)]
pub struct VaultChanges(BTreeSet<VaultKey>);

#[derive(Codec)]
pub struct VaultVersion(Nonce);

#[derive(Codec, Default)]
pub struct VaultKeys(BTreeSet<VaultKey>);

pub struct Vault {
    pub chats: HashMap<ChatName, ChatMeta>,
    pub friend_index: HashMap<crypto::Hash, UserName>,
    pub friends: HashMap<UserName, FriendMeta>,
    pub theme: Theme,
    last_update: instant::Instant,
    change_count: usize,
}

impl Default for Vault {
    fn default() -> Self {
        Self {
            chats: Default::default(),
            friend_index: Default::default(),
            friends: Default::default(),
            theme: Default::default(),
            last_update: instant::Instant::now(),
            change_count: Default::default(),
        }
    }
}

fn constant_key(name: &str) -> SharedSecret {
    crypto::hash::new(crypto::hash::new(name))
}

fn chats() -> SharedSecret {
    constant_key("chats")
}

fn theme() -> SharedSecret {
    constant_key("theme")
}

fn friends() -> SharedSecret {
    constant_key("friends")
}

fn get_encrypted<T: DecodeOwned>(
    key: crypto::Hash,
    decryption_key: SharedSecret,
) -> anyhow::Result<T> {
    let VaultValue(v) = Storage::get(key).context("not found")?;
    let v = chain_api::decrypt(v, decryption_key).context("decryption failed")?;
    log::info!("decrypted: {:?}", v);
    Decode::decode_exact(&v).context("invalid encoding")
}

fn get_plain<T: DecodeOwned>(key: crypto::Hash) -> anyhow::Result<T> {
    let VaultValue(v) = Storage::get(key).context("not found")?;
    Decode::decode_exact(&v).context("invalid encoding")
}

impl Vault {
    pub fn new(key: SharedSecret) -> Self {
        Self::repair(key);
        Self::default()
    }

    pub fn repair(key: SharedSecret) {
        fn ensure(key: SharedSecret, id: VaultKey, value: Vec<u8>) {
            if Storage::ensure(key, &VaultValue(value)) {
                Storage::update([], |VaultKeys(keys)| _ = keys.insert(id));
                Storage::update([], |VaultChanges(changes)| _ = changes.insert(id));
            }
        }

        ensure(chats(), VaultKey::Chats, encrypt(vec![0], key));
        ensure(theme(), VaultKey::Theme, Theme::default().to_bytes());
        ensure(friends(), VaultKey::FriendNames, encrypt(vec![0], key));
    }

    pub fn from_storage(key: SharedSecret) -> anyhow::Result<Self> {
        let friends = get_encrypted::<Vec<(UserName, FriendId)>>(friends(), key)
            .context("loading friends")?
            .into_iter()
            .map(|(name, id)| {
                get_encrypted(id, key)
                    .map(|pk| (name, FriendMeta { id, ..pk }))
                    .with_context(|| format!("loading friend'{name}'"))
            })
            .collect::<anyhow::Result<HashMap<_, _>>>()?;

        Ok(Self {
            chats: get_encrypted(chats(), key).context("loading chats")?,
            theme: get_plain(theme()).context("loading theme")?,
            friend_index: friends.iter().map(|(&n, f)| (f.dr.receiver_hash(), n)).collect(),
            friends,
            last_update: instant::Instant::now(),
            change_count: 0,
        })
    }

    pub fn needs_refresh(for_version: Nonce) -> bool {
        Storage::get([]).map_or(true, |VaultVersion(v)| v < for_version)
    }

    pub fn refresh(values: Vec<(crypto::Hash, Vec<u8>)>) {
        for (key, value) in values {
            Storage::insert(key, &VaultValue(value));
        }
    }

    pub fn merkle_hash(&self) -> crypto::Hash {
        let VaultKeys(keys) = Storage::get([]).unwrap_or_default();

        log::info!("keys: {:?}", keys);

        let mut hashes = keys
            .iter()
            .filter_map(|k| self.get_key(*k))
            .filter_map(|k| Storage::get(k).map(|VaultValue(v)| crypto::hash::kv(&k, &v)))
            .collect::<Vec<_>>();
        hashes.sort_unstable();

        log::info!("hashes: {:?}", hashes);

        *hashes.into_iter().collect::<MerkleTree<_>>().root()
    }

    pub fn changes(&mut self) -> Vec<(crypto::Hash, Vec<u8>)> {
        let VaultChanges(changes) = Storage::get([]).unwrap_or_default();
        if self.change_count > 40
            || self.last_update.elapsed() > instant::Duration::from_secs(60)
            || changes.len() > 10
        {
            changes
                .into_iter()
                .filter_map(|k| self.get_key(k))
                .filter_map(|k| Some((k, Storage::get::<VaultValue>(k)?.0)))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn clear_changes(&mut self, version: Nonce) {
        self.change_count = 0;
        self.last_update = instant::Instant::now();
        Storage::insert([], &VaultChanges::default());
        Storage::insert([], &VaultVersion(version));
    }

    pub fn get_repr(&self, id: VaultKey, key: SharedSecret) -> Option<Vec<u8>> {
        use VaultKey as VCI;
        let repr = match id {
            VCI::Chats => self.chats.to_bytes(),
            VCI::Theme => self.theme.to_bytes(),
            VCI::Friend(name) => self.friends.get(&name)?.to_bytes(),
            VCI::FriendNames => {
                self.friends.iter().map(|(n, f)| (n, f.id)).collect::<Vec<_>>().to_bytes()
            }
        };

        let value = match id {
            VCI::Theme => repr,
            _ => encrypt(repr, key),
        };

        Some(value)
    }

    pub fn get_key(&self, id: VaultKey) -> Option<SharedSecret> {
        use VaultKey as VCI;
        let key = match id {
            VCI::Chats => chats(),
            VCI::Theme => theme(),
            VCI::Friend(name) => self.friends.get(&name).map(|f| f.id)?,
            VCI::FriendNames => friends(),
        };

        Some(key)
    }

    pub fn update(&mut self, id: VaultKey, key: SharedSecret) -> Option<()> {
        let value = VaultValue(self.get_repr(id, key)?);
        let key = self.get_key(id)?;

        let current: Option<VaultValue> = Storage::get(key);
        if current.as_ref() == Some(&value) {
            return None;
        }

        Storage::insert(key, &value);
        Storage::update([], |VaultKeys(keys)| _ = keys.insert(id));
        Storage::update([], |VaultChanges(changes)| _ = changes.insert(id));
        self.change_count += 1;

        Some(())
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy, Codec)]
pub enum VaultKey {
    Chats,
    Friend(UserName),
    FriendNames,
    Theme,
}

#[derive(Codec, Clone, Copy)]
pub struct ChatMeta {
    pub secret: crypto::SharedSecret,
    #[codec(skip)]
    pub action_no: Nonce,
}

impl Default for ChatMeta {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatMeta {
    pub fn new() -> Self {
        Self::from_secret(crypto::new_secret(OsRng))
    }

    pub fn from_secret(secret: SharedSecret) -> Self {
        Self { secret, action_no: Default::default() }
    }
}

#[derive(Default, Codec, Clone)]
pub struct FriendChatMeta {
    pub members: HashSet<UserName>,
}

#[derive(Codec, Clone)]
pub struct FriendMeta {
    pub dr: DoubleRatchet,
    pub identity: crypto::Hash,
    #[codec(skip)]
    pub id: FriendId,
}

pub struct RawChatMessage {
    pub content: String,
    pub identity: Identity,
    pub sender: UserName,
}

pub fn try_set_color(name: &str, value: u32) -> Result<(), JsValue> {
    web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .body()
        .ok_or("no body")?
        .style()
        .set_property(name, &format!("#{:08x}", value))
}

pub fn try_load_color_from_style(name: &str) -> Result<u32, JsValue> {
    u32::from_str_radix(
        web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .body()
            .ok_or("no body")?
            .style()
            .get_property_value(name)?
            .strip_prefix('#')
            .ok_or("expected # to start the color")?,
        16,
    )
    .map_err(|e| e.to_string().into())
}

macro_rules! gen_theme {
    ($(
        $name:ident: $value:literal,
    )*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Codec)]
        pub struct Theme { $(
            pub $name: u32,
        )* }

        impl Theme {
            pub fn apply(self) -> Result<(), JsValue> {
                $(try_set_color(concat!("--", stringify!($name), "-color"), self.$name)?;)*
                Ok(())
            }

            pub fn from_current() -> Result<Self, JsValue> {
                Ok(Self { $(
                    $name: try_load_color_from_style(concat!("--", stringify!($name), "-color"))?,
                )* })
            }

            pub const KEYS: &'static [&'static str] = &[$(stringify!($name),)*];
        }

        impl Default for Theme {
            fn default() -> Self {
                Self { $( $name: $value,)* }
            }
        }
    };
}

gen_theme! {
    primary: 0x0000_00ff,
    secondary: 0x3333_33ff,
    highlight: 0xffff_ffff,
    font: 0xffff_ffff,
    error: 0xff00_00ff,
}
