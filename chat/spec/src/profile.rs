use {
    crate::{Nonce, Proof},
    arrayvec::ArrayString,
    chain_api::{RawUserName, USER_NAME_CAP},
    codec::Codec,
    crypto::{enc, sign},
    std::{collections::BTreeMap, iter},
};

pub const MAIL_BOX_CAP: usize = 1024 * 1024;

pub type UserName = ArrayString<32>;

#[derive(Clone, Codec)]
pub struct Profile {
    pub sign: sign::PublicKey,
    pub enc: enc::PublicKey,
    pub vault_sig: sign::Signature,
    pub vault_version: Nonce,
    pub mail_action: Nonce,
    #[codec(with = profile_vault_codec)]
    pub vault: BTreeMap<crypto::Hash, Vec<u8>>,
    pub mail: Vec<u8>,
}

impl Profile {
    pub fn is_valid(&self) -> bool {
        Proof {
            pk: self.sign,
            context: Self::vault_hash(&self.vault),
            nonce: self.vault_version,
            signature: self.vault_sig,
        }
        .verify()
    }

    pub fn vault_hash(values: &BTreeMap<crypto::Hash, Vec<u8>>) -> crypto::Hash {
        values.keys().copied().reduce(crypto::hash::combine).unwrap_or_default()
    }
}

mod profile_vault_codec {
    use codec::{Buffer, Codec};

    pub fn encode(
        values: &std::collections::BTreeMap<crypto::Hash, Vec<u8>>,
        buffer: &mut impl Buffer,
    ) -> Option<()> {
        values.len().encode(buffer)?;
        for v in values.values() {
            v.encode(buffer)?;
        }
        Some(())
    }

    pub fn decode(buffer: &mut &[u8]) -> Option<std::collections::BTreeMap<crypto::Hash, Vec<u8>>> {
        let len = usize::decode(buffer)?;
        let mut values = std::collections::BTreeMap::new();
        for _ in 0..len {
            let value = Vec::<u8>::decode(buffer)?;
            let key = crypto::hash::new(&value);
            values.insert(key, value);
        }
        Some(values)
    }
}

impl Profile {
    pub fn read_mail(&mut self) -> &[u8] {
        // SAFETY: thre resulting slice locks mutable access to self, we just need to truncate
        // while preserving the borrow
        let slice = unsafe { std::mem::transmute(self.mail.as_slice()) };
        // SAFETY: while the slice exists we cannot push to `self.mail` thus truncating is safe, we
        // avoid truncate since it calls destructors witch requires mutable access to slice memory,
        // we dont want that
        unsafe { self.mail.set_len(0) };
        slice
    }

    pub fn push_mail(&mut self, content: &[u8]) {
        self.mail.extend((content.len() as u16).to_be_bytes());
        self.mail.extend_from_slice(content);
    }
}

impl From<&Profile> for FetchProfileResp {
    fn from(profile: &Profile) -> Self {
        Self { sign: profile.sign, enc: profile.enc }
    }
}

#[derive(Codec)]
pub struct FetchProfileResp {
    pub sign: sign::PublicKey,
    pub enc: enc::PublicKey,
}

#[must_use]
pub fn username_to_raw(u: UserName) -> RawUserName {
    let mut arr = [0; USER_NAME_CAP];
    arr[..u.len()].copy_from_slice(u.as_bytes());
    arr
}

#[must_use]
pub fn username_from_raw(name: RawUserName) -> Option<UserName> {
    let len = name.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    let name = &name[..len];
    UserName::from(core::str::from_utf8(name).ok()?).ok()
}

pub fn unpack_mail(mut buffer: &[u8]) -> impl Iterator<Item = &[u8]> {
    iter::from_fn(move || {
        let len = buffer.take(..2)?;
        let len = u16::from_be_bytes(len.try_into().unwrap());
        buffer.take(..len as usize)
    })
}
