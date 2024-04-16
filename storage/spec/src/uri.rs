use {
    crate::{Address, FileId, NodeIdentity},
    arrayvec::ArrayString,
    std::{fmt::Display, str::FromStr},
};

const MAX_PASS_PHRASE_LEN: usize = 48;

pub fn passe_satelite_id(input: &str) -> Result<NodeIdentity, bs58::decode::Error> {
    let mut satelite = NodeIdentity::default();
    bs58::decode(input).onto(&mut satelite)?;
    Ok(satelite)
}

pub struct Uri {
    pub satelite: NodeIdentity,
    pub address: Address,
    pub pass_phrase: ArrayString<MAX_PASS_PHRASE_LEN>,
}

impl Display for Uri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buf = [0u8; 64];
        write!(f, "orion://")?;
        let len = bs58::encode(&self.satelite).onto(buf.as_mut_slice()).unwrap();
        write!(f, "{}", std::str::from_utf8(&buf[..len]).unwrap())?;
        write!(f, "/")?;
        let len = bs58::encode(&self.address.id).onto(buf.as_mut_slice()).unwrap();
        write!(f, "{}", std::str::from_utf8(&buf[..len]).unwrap())?;
        write!(f, "?size={}", { self.address.size })?;
        if !self.pass_phrase.is_empty() {
            write!(f, "&pass={}", self.pass_phrase)?;
        }

        Ok(())
    }
}

impl FromStr for Uri {
    type Err = UriParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use UriParseError as E;

        let path = s.strip_prefix("orion://").ok_or(E::NotOrion)?;
        let (satelite_b58, rest) = path.split_once('/').ok_or(E::MissingAdress)?;
        let mut satelite = NodeIdentity::default();
        bs58::decode(satelite_b58).onto(&mut satelite).map_err(E::InvalidSatelite)?;

        let (address_b58, params) = rest.split_once('?').ok_or(E::MissingSize)?;
        let mut id = FileId::default();
        bs58::decode(address_b58).onto(&mut id).map_err(E::InvalidFileId)?;

        let mut size = None::<u64>;
        let mut pass_phrase = ArrayString::new();

        for (i, (name, value)) in
            params.split('&').map(|p| p.split_once('=').unwrap_or((p, ""))).enumerate()
        {
            match name {
                "size" => size = Some(value.parse().map_err(|_| E::InvalidSize)?),
                "pass" => pass_phrase = ArrayString::from(value).map_err(|_| E::InvalidPass)?,
                _ => return Err(E::UnknownParam(i)),
            }
        }

        let size = size.ok_or(E::MissingSize)?;

        Ok(Self { satelite, address: Address { id, size }, pass_phrase })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UriParseError {
    #[error("not an orion uri, that starts with 'orion://'")]
    NotOrion,
    #[error("missing file adress in the uri")]
    MissingAdress,
    #[error("invalid satelite id: {0}")]
    InvalidSatelite(bs58::decode::Error),
    #[error("invalid file id: {0}")]
    InvalidFileId(bs58::decode::Error),
    #[error("invalid size parameter, expected reasonably big integer")]
    InvalidSize,
    #[error("invalid pass phrase, expected string up to {MAX_PASS_PHRASE_LEN} characters long")]
    InvalidPass,
    #[error("missing 'size' parameter in the uri")]
    MissingSize,
    #[error("param {0} is not recognized")]
    UnknownParam(usize),
}
