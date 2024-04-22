#![feature(array_chunks)]
#![feature(let_chains)]
use std::str::FromStr;

#[macro_export]
macro_rules! env_config {
    (
       struct $name:ident {$(
            #[doc = $doc:expr]
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
                    let $field = $crate::get_env(stringify!($field), $doc)
                        .map_err(|e| errors.push(e));
                )*

                match errors.len() {
                    0 => Ok(Self { $( $field: $field.unwrap(),)* }),
                    1 => Err(errors.pop().unwrap()),
                    _ => Err($crate::EnvError::Multiple(errors)),
                }
            }
        }
    };
}

#[must_use]
pub fn get_env<T: std::str::FromStr>(key: &'static str, doc: &'static str) -> Result<T, EnvError>
where
    T::Err: std::fmt::Display,
{
    let key = key.to_uppercase();
    std::env::var(&key)
        .map_err(|_| EnvError::KeyNotFound(key.clone(), doc))?
        .parse::<T>()
        .map_err(|e| EnvError::ParseError(key, std::any::type_name::<T>(), e.to_string(), doc))
}

#[derive(Debug)]
pub enum EnvError {
    KeyNotFound(String, &'static str),
    ParseError(String, &'static str, String, &'static str),
    Multiple(Vec<EnvError>),
}

pub fn combine_errors<A, B>(
    a: Result<A, EnvError>,
    b: Result<B, EnvError>,
) -> Result<(A, B), EnvError> {
    match (a, b) {
        (Ok(a), Ok(b)) => Ok((a, b)),
        (Err(b), Ok(_)) | (Ok(_), Err(b)) => Err(b),
        (Err(a), Err(b)) => {
            let mut err = match (a, b) {
                (EnvError::Multiple(mut errors), EnvError::Multiple(mut new_errors)) => {
                    errors.append(&mut new_errors);
                    errors
                }
                (EnvError::Multiple(mut errors), b) | (b, EnvError::Multiple(mut errors)) => {
                    errors.push(b);
                    errors
                }
                (a, b) => vec![a, b],
            };

            let key = |e: &EnvError| match e {
                EnvError::KeyNotFound(key, _) => key.clone(),
                EnvError::ParseError(key, ..) => key.clone(),
                EnvError::Multiple(_) => unreachable!(),
            };

            err.sort_by_key(key);
            err.dedup_by_key(|e| key(e));
            Err(EnvError::Multiple(err))
        }
    }
}

impl std::fmt::Display for EnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyNotFound(key, doc) => write!(f, "key not found: {key} // {doc}"),
            Self::ParseError(key, ty, value, doc) => {
                write!(f, "failed to parse {key} as {ty}: {value} // {doc}")
            }
            Self::Multiple(errors) => {
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
