use {
    crate::Cache,
    chain_api::encrypt,
    chat_spec::*,
    codec::{Codec, Decode, DecodeOwned, Encode},
    crypto::{decrypt, proof::Nonce},
    double_ratchet::DoubleRatchet,
    onion::SharedSecret,
    rand::rngs::OsRng,
    std::collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    web_sys::wasm_bindgen::JsValue,
};

pub type FriendId = SharedSecret;

#[derive(Codec)]
pub struct VaultValue(Vec<u8>);

#[derive(Codec)]
pub struct VaultChanges(BTreeSet<crypto::Hash>);

#[derive(Codec)]
pub struct VaultHeader {
    pub version: Nonce,
    pub keys: BTreeSet<crypto::Hash>,
}

pub struct Vault {
    pub chats: HashMap<ChatName, ChatMeta>,
    pub friend_index: HashMap<crypto::Hash, UserName>,
    pub friends: HashMap<UserName, FriendMeta>,
    pub theme: Theme,
    last_update: instant::Instant,
    change_count: usize,
    changed_keys: BTreeSet<crypto::Hash>,
    raw: chat_spec::Vault,
}

impl Default for Vault {
    fn default() -> Self {
        Self::deserialize(Default::default(), Default::default())
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
    values: &BTreeMap<crypto::Hash, Vec<u8>>,
) -> Option<T> {
    values
        .get(&key)
        .and_then(|v| Decode::decode(&mut &*decrypt(&mut v.to_owned(), decryption_key)?))
}

fn get_plain<T: DecodeOwned>(
    key: crypto::Hash,
    values: &BTreeMap<crypto::Hash, Vec<u8>>,
) -> Option<T> {
    values.get(&key).and_then(|v| Decode::decode(&mut v.as_slice()))
}

impl Vault {
    pub(crate) fn deserialize(values: BTreeMap<crypto::Hash, Vec<u8>>, key: SharedSecret) -> Self {
        let friends = get_encrypted::<Vec<(UserName, FriendId)>>(friends(), key, &values)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(name, id)| {
                get_encrypted(id, key, &values).map(|pk| (name, FriendMeta { id, ..pk }))
            })
            .collect::<HashMap<_, _>>();

        Self {
            chats: get_encrypted(chats(), key, &values).unwrap_or_default(),
            theme: get_plain(theme(), &values).unwrap_or_default(),
            friend_index: friends.iter().map(|(&n, f)| (f.dr.receiver_hash(), n)).collect(),
            friends,
            last_update: instant::Instant::now(),
            change_count: 0,
            changed_keys: Cache::get([]).map(|VaultChanges(c)| c).unwrap_or_default(),
            raw: {
                let mut raw = chat_spec::Vault {
                    values,
                    merkle_tree: Default::default(),
                    sig: unsafe { std::mem::zeroed() },
                    version: 0,
                };
                raw.recompute();
                raw
            },
        }
    }

    pub fn values_from_cache(version: Nonce) -> Option<BTreeMap<crypto::Hash, Vec<u8>>> {
        let VaultHeader { version: v, keys } = Cache::get([])?;

        if v != version {
            return None;
        }

        let values = keys
            .iter()
            .filter_map(|k| Cache::get(k).map(|VaultValue(v)| (*k, v)))
            .collect::<BTreeMap<_, _>>();

        if values.len() != keys.len() {
            return None;
        }

        Some(values)
    }

    pub fn merkle_hash(&self) -> crypto::Hash {
        *self.raw.merkle_tree.root()
    }

    pub fn changes(&mut self) -> Vec<(crypto::Hash, Vec<u8>)> {
        if self.change_count > 40
            || self.last_update.elapsed() > instant::Duration::from_secs(60)
            || self.changed_keys.len() > 10
        {
            self.changed_keys
                .iter()
                .filter_map(|&k| self.raw.values.get(&k).cloned().map(|v| (k, v)))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn clear_changes(&mut self, version: Nonce) {
        self.change_count = 0;
        self.changed_keys.clear();
        self.last_update = instant::Instant::now();
        Cache::insert([], &VaultChanges(self.changed_keys.clone()));
        Cache::insert([], &VaultHeader {
            version,
            keys: self.raw.values.keys().cloned().collect(),
        });
    }

    pub fn update(&mut self, id: VaultComponentId, key: SharedSecret) -> Option<()> {
        use VaultComponentId as VCI;
        let hash = match id {
            VCI::Chats => chats(),
            VCI::Theme => theme(),
            VCI::FriendNames => friends(),
            VCI::Friend(name) => self.friends.get(&name).map(|f| f.id)?,
        };

        let value = match id {
            VCI::Chats => self.chats.to_bytes(),
            VCI::Theme => self.theme.to_bytes(),
            VCI::Friend(name) => self.friends.get(&name).map(Encode::to_bytes)?,
            VCI::FriendNames => {
                self.friends.iter().map(|(&n, f)| (n, f.id)).collect::<Vec<_>>().to_bytes()
            }
        };

        let value = match id {
            VCI::Theme => value,
            _ => encrypt(value, key),
        };

        if self.raw.values.get(&hash) == Some(&value) {
            return None;
        }

        let value = VaultValue(value);
        Cache::insert(hash, &value);
        self.raw.values.insert(hash, value.0);
        self.raw.recompute();

        self.change_count += 1;
        self.changed_keys.insert(hash);
        Cache::insert([], &VaultChanges(self.changed_keys.clone()));

        Some(())
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::large_enum_variant)]
pub enum VaultComponentId {
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

#[derive(Codec)]
pub struct RawChatMessage {
    pub sender: UserName,
    pub content: String,

    #[codec(skip)]
    pub identity: Identity,
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
