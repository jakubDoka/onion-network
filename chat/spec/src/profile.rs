use {
    crate::{Identity, Nonce},
    arrayvec::ArrayString,
    chain_api::{RawUserName, USER_NAME_CAP},
    codec::{Codec, DecodeOwned},
    crypto::{enc, hash, proof::Proof, sign, SharedSecret},
    merkle_tree::MerkleTree,
    std::{collections::BTreeMap, iter},
};

pub const MAIL_BOX_CAP: usize = 1024 * 1024;

pub type UserName = ArrayString<32>;

#[derive(Clone, Codec)]
pub struct Profile {
    pub sign: sign::PublicKey,
    pub enc: enc::PublicKey,
    pub vault: Vault,
    pub mail_action: Nonce,
    pub mail: Vec<u8>,
}

impl Profile {
    pub fn is_valid(&self) -> bool {
        self.vault.is_valid(self.sign)
    }
}

#[derive(Codec, Clone)]
pub struct Vault {
    pub version: Nonce,
    pub sig: sign::Signature,
    pub values: BTreeMap<crypto::Hash, Vec<u8>>,
    #[codec(skip)]
    pub merkle_tree: MerkleTree<crypto::Hash>,
}

impl Vault {
    pub fn prepare(mut self, pk: sign::PublicKey) -> Option<Self> {
        self.recompute();
        self.is_valid(pk).then_some(self)
    }

    pub fn is_valid(&self, pk: sign::PublicKey) -> bool {
        Proof { pk, context: *self.merkle_tree.root(), nonce: self.version, signature: self.sig }
            .verify()
    }

    pub fn recompute(&mut self) {
        self.merkle_tree =
            self.values.iter().map(|(&k, v)| hash::combine(k, hash::new(v))).collect();
    }

    pub fn try_remove(&mut self, key: crypto::Hash, proof: Proof<crypto::Hash>) -> bool {
        if !proof.verify() {
            return false;
        }

        if proof.nonce <= self.version {
            return false;
        }

        let Some(v) = self.values.remove(&key) else {
            return false;
        };

        // TODO: we might be able to avoid full recompute and update smartly
        self.recompute();

        if proof.context != *self.merkle_tree.root() {
            self.values.insert(key, v);
            self.recompute();
            return false;
        }

        self.version = proof.nonce;
        self.sig = proof.signature;

        true
    }

    pub fn try_insert_bulk(
        &mut self,
        changes: Vec<(Identity, Vec<u8>)>,
        proof: Proof<Identity>,
    ) -> bool {
        if !proof.verify() {
            return false;
        }

        if proof.nonce <= self.version {
            return false;
        }

        let prevs = changes
            .iter()
            .map(|(k, v)| (k, self.values.insert(*k, v.clone())))
            .collect::<BTreeMap<_, _>>();

        self.recompute();

        if proof.context != *self.merkle_tree.root() {
            for (k, v) in prevs {
                if let Some(v) = v {
                    self.values.insert(*k, v);
                } else {
                    self.values.remove(k);
                }
            }
            self.recompute();

            return false;
        }

        self.version = proof.nonce;
        self.sig = proof.signature;

        true
    }

    pub fn try_insert(
        &mut self,
        key: crypto::Hash,
        value: Vec<u8>,
        proof: Proof<crypto::Hash>,
    ) -> bool {
        self.try_insert_bulk(vec![(key, value)], proof)
    }

    pub fn get_ecrypted<T: DecodeOwned>(
        &self,
        key: crypto::Hash,
        encryption_key: SharedSecret,
    ) -> Option<T> {
        let mut value = self.values.get(&key)?.clone();
        let value = crypto::decrypt(&mut value, encryption_key)?;
        T::decode(&mut &*value)
    }

    pub fn get_plaintext<T: DecodeOwned>(&self, key: crypto::Hash) -> Option<T> {
        let value = self.values.get(&key)?;
        T::decode(&mut value.as_slice())
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
