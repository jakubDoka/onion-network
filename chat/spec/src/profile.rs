use {
    arrayvec::ArrayString,
    chain_api::{RawUserName, USER_NAME_CAP},
    codec::Codec,
    crypto::{enc, sign},
    std::iter,
};

pub const MAIL_BOX_CAP: usize = 1024 * 1024;
pub const MAX_MAIL_SIZE: usize = 1024 * 8;
pub const MAX_VAULT_KEY_COUNT: usize = 4096;
pub const MAX_VAULT_VALUE_SIZE: usize = 1024 * 8;
pub const MAX_VAULT_UPDATE_SIZE: usize = 1024 * 32;

pub type UserName = ArrayString<32>;

#[derive(Codec)]
#[repr(packed)]
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
