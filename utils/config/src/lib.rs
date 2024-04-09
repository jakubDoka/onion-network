#![feature(array_chunks)]
#![feature(let_chains)]
use std::str::FromStr;

#[macro_export]
macro_rules! env_config {
    (
       struct $name:ident {$(
            $field:ident: $ty:ty,
       )*}
    ) => {
        pub struct $name {$(
            pub $field: $ty,
        )*}

        impl $name {
            pub fn from_env() -> Result<Self, $crate::EnvError> {
                let mut errors = Vec::new();

                $(
                    let $field = $crate::get_env(stringify!($field))
                        .map_err(|e| errors.push(e));
                )*

                match errors.len() {
                    0 => Ok(Self { $( $field: $field.unwrap(),)* }),
                    1 => Err(errors.pop().unwrap()),
                    _ => Err($crate::EnvError::MultipleErrors(errors)),
                }
            }
        }
    };
}

#[must_use]
pub fn get_env<T: std::str::FromStr>(key: &'static str) -> Result<T, EnvError>
where
    T::Err: std::fmt::Display,
{
    let key = key.to_uppercase();
    std::env::var(&key)
        .map_err(|_| EnvError::KeyNotFound(key.clone()))?
        .parse::<T>()
        .map_err(|e| EnvError::ParseError(key, std::any::type_name::<T>(), e.to_string()))
}

#[derive(Debug)]
pub enum EnvError {
    KeyNotFound(String),
    ParseError(String, &'static str, String),
    MultipleErrors(Vec<EnvError>),
}

impl std::fmt::Display for EnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyNotFound(key) => write!(f, "key not found: {key}"),
            Self::ParseError(key, ty, value) => write!(f, "failed to parse {key} as {ty}: {value}"),
            Self::MultipleErrors(errors) => {
                write!(f, "multiple env errors:")?;
                for error in errors {
                    write!(f, "\n  {error}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for EnvError {}

pub struct List<T>(pub Vec<T>);

impl<T: FromStr> FromStr for List<T> {
    type Err = <T as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut peers = Vec::new();
        for peer in s.split(',').filter(|s| !s.is_empty()) {
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
