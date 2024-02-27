use {
    crate::{Identity, Nonce, Proof, Topic},
    chain_api::{RawUserName, USER_NAME_CAP},
    component_utils::{arrayvec::ArrayString, encode_len, Codec, Reminder},
    crypto::{enc, sign, Serialized},
    std::iter,
};

pub const MAIL_BOX_CAP: usize = 1024 * 1024;

pub type UserName = ArrayString<32>;

#[derive(Clone, Codec)]
pub struct Profile {
    pub sign: Serialized<sign::PublicKey>,
    pub enc: Serialized<enc::PublicKey>,
    pub last_sig: Serialized<sign::Signature>,
    pub vault_version: Nonce,
    pub mail_action: Nonce,
    pub vault: Vec<u8>,
    pub mail: Vec<u8>,
}

#[derive(Clone, Copy, Codec)]
pub struct BorrowedProfile<'a> {
    pub sign: Serialized<sign::PublicKey>,
    pub enc: Serialized<enc::PublicKey>,
    pub last_sig: Serialized<sign::Signature>,
    pub vault_version: Nonce,
    pub mail_action: Nonce,
    pub vault: &'a [u8],
    pub mail: &'a [u8],
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
        self.mail.extend(encode_len(content.len()));
        self.mail.extend_from_slice(content);
    }
}

impl<'a> From<&'a Profile> for BorrowedProfile<'a> {
    fn from(profile: &'a Profile) -> Self {
        Self {
            sign: profile.sign,
            enc: profile.enc,
            last_sig: profile.last_sig,
            vault_version: profile.vault_version,
            mail_action: profile.mail_action,
            vault: profile.vault.as_slice(),
            mail: profile.mail.as_slice(),
        }
    }
}

impl<'a> BorrowedProfile<'a> {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        Proof {
            pk: self.sign,
            signature: self.last_sig,
            nonce: self.vault_version,
            context: self.vault,
        }
        .verify()
    }
}

impl<'a> From<BorrowedProfile<'a>> for Profile {
    fn from(profile: BorrowedProfile<'a>) -> Self {
        Self {
            sign: profile.sign,
            enc: profile.enc,
            last_sig: profile.last_sig,
            vault_version: profile.vault_version,
            mail_action: profile.mail_action,
            vault: profile.vault.to_vec(),
            mail: profile.mail.to_vec(),
        }
    }
}

impl From<&Profile> for FetchProfileResp {
    fn from(profile: &Profile) -> Self {
        Self { sign: profile.sign, enc: profile.enc }
    }
}

impl Topic for Identity {
    type Event<'a> = ProfileEvent<'a>;
    type Record = Profile;
}

type ProfileEvent<'a> = Reminder<'a>;

#[derive(Codec)]
pub struct FetchProfileResp {
    pub sign: Serialized<sign::PublicKey>,
    pub enc: Serialized<enc::PublicKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum FetchProfileError {
    #[error("account not found")]
    NotFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum CreateAccountError {
    #[error("invalid proof")]
    InvalidProof,
    #[error("account already exists")]
    AlreadyExists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum SetVaultError {
    #[error("account not found")]
    NotFound,
    #[error("invalid proof")]
    InvalidProof,
    #[error("invalid action")]
    InvalidAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum FetchVaultError {
    #[error("account not found")]
    NotFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum ReadMailError {
    #[error("account not found")]
    NotFound,
    #[error("invalid proof")]
    InvalidProof,
    #[error("invalid action")]
    InvalidAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Codec, thiserror::Error)]
pub enum SendMailError {
    #[error("account not found")]
    NotFound,
    #[error("sent directly")]
    SentDirectly,
    #[error("sending to self is not allowed")]
    SendingToSelf,
    #[error("mailbox full (limit: {MAIL_BOX_CAP})")]
    MailboxFull,
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
