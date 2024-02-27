#![feature(array_chunks)]
#![feature(let_chains)]
use std::str::FromStr;

#[macro_export]
macro_rules! env_config {
    (
       struct $name:ident {$(
            $field:ident: $ty:ty $(= $default:expr)?,
       )*}
    ) => {
        pub struct $name {$(
            pub $field: $ty,
        )*}

        impl $name {
            pub fn from_env() -> Self {
                $name {$(
                    $field: $crate::get_env(stringify!($field), $crate::env_config!(@default $($default)?)),
                )*}
            }
        }
    };

    (@default $value:expr) => { Some($value) };
    (@default ) => { None };
}

#[must_use]
pub fn get_env<T: std::str::FromStr>(key: &str, default: Option<&str>) -> T
where
    T::Err: std::fmt::Display,
{
    let key = key.to_uppercase();
    if cfg!(debug_assertions)
        && let Some(default) = default
    {
        std::env::var(&key).unwrap_or_else(|_| default.to_string())
    } else {
        std::env::var(&key).unwrap_or_else(|_| panic!("{key} is not set"))
    }
    .parse()
    .unwrap_or_else(|e| panic!("{key} is not valid {}: {e:#}", std::any::type_name::<T>()))
}

pub struct List<T>(pub Vec<T>);

impl<T: FromStr> FromStr for List<T> {
    type Err = <T as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut peers = Vec::new();
        for peer in s.split(',') {
            peers.push(peer.parse::<T>()?);
        }
        Ok(Self(peers))
    }
}

impl<T> Default for List<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Hex([u8; 32]);

impl Hex {
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl AsRef<[u8]> for Hex {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsMut<[u8]> for Hex {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

impl FromStr for Hex {
    type Err = SecretKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(SecretKeyError::InvalidLength(s.len()));
        }

        let mut bytes = [0; 32];
        for (&[a, b], dest) in s.as_bytes().array_chunks().zip(bytes.iter_mut()) {
            const fn hex_to_u8(e: u8) -> Result<u8, SecretKeyError> {
                Ok(match e {
                    b'0'..=b'9' => e - b'0',
                    b'a'..=b'f' => e - b'a' + 10,
                    b'A'..=b'F' => e - b'A' + 10,
                    _ => return Err(SecretKeyError::InvalidHex),
                })
            }

            *dest = hex_to_u8(a)? << 4 | hex_to_u8(b)?;
        }

        Ok(Self(bytes))
    }
}

#[derive(Debug)]
pub enum SecretKeyError {
    InvalidLength(usize),
    InvalidHex,
}

impl std::fmt::Display for SecretKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLength(len) => {
                write!(f, "invalid length (expected 64): {len}")
            }
            Self::InvalidHex => write!(f, "invalid hex (expected [0-9a-fA-F])"),
        }
    }
}

impl std::error::Error for SecretKeyError {}
