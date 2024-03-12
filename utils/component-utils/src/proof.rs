use {
    crate::{self as component_utils, Codec, Reminder},
    arrayvec::ArrayString,
    crypto::*,
    rand_core::*,
};

pub type Nonce = u64;

#[derive(Clone, Copy, Codec, Debug)]
pub struct Proof<T> {
    pub pk: Serialized<sign::PublicKey>,
    pub nonce: Nonce,
    pub signature: Serialized<sign::Signature>,
    pub context: T,
}

const PAYLOAD_SIZE: usize = std::mem::size_of::<Nonce>() + hash::SIZE;

impl<T: ToProofContext> Proof<T> {
    pub fn new(kp: &sign::Keypair, nonce: &mut Nonce, context: T, rng: impl CryptoRngCore) -> Self {
        let signature = kp.sign(&Self::pack_payload(*nonce, context.to_proof_context()), rng);
        *nonce += 1;
        Self {
            pk: kp.public_key().into_bytes(),
            nonce: *nonce - 1,
            signature: signature.into_bytes(),
            context,
        }
    }

    fn pack_payload(nonce: Nonce, context: Hash) -> [u8; PAYLOAD_SIZE] {
        let mut buf = [0; PAYLOAD_SIZE];
        buf[..hash::SIZE].copy_from_slice(&context);
        buf[hash::SIZE..].copy_from_slice(&nonce.to_be_bytes());
        buf
    }

    pub fn verify(&self) -> bool {
        let bytes = Self::pack_payload(self.nonce, self.context.to_proof_context());
        let pk = sign::PublicKey::from_ref(&self.pk);
        let signature = sign::Signature::from_ref(&self.signature);
        pk.verify(&bytes, signature).is_ok()
    }
}

pub trait ToProofContext: Copy {
    fn to_proof_context(self) -> Hash;
}

impl<'a> ToProofContext for Reminder<'a> {
    fn to_proof_context(self) -> Hash {
        crypto::hash::from_slice(self.0)
    }
}

impl<'a> ToProofContext for &'a [u8] {
    fn to_proof_context(self) -> Hash {
        crypto::hash::from_slice(self)
    }
}

impl<const SIZE: usize> ToProofContext for [u8; SIZE] {
    fn to_proof_context(self) -> Hash {
        crypto::hash::from_slice(&self)
    }
}

impl<const SIZE: usize> ToProofContext for ArrayString<SIZE> {
    fn to_proof_context(self) -> Hash {
        crypto::hash::from_slice(self.as_bytes())
    }
}
