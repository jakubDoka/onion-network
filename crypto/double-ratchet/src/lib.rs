#![feature(slice_take)]
use {
    arrayvec::ArrayVec,
    codec::Codec,
    hkdf::Hkdf,
    kyber::{CIPHERTEXTBYTES, ENC_SEEDBYTES},
    rand_core::CryptoRngCore,
    sha2::Sha256,
    std::array,
};

const MAX_KEPT_MISSING_MESSAGES: usize = 5;
const SUB_CONSTANT: SharedSecret = [0u8; 32];
const RATCHET_SEED_SIZE: usize = 32 + 64;

type SharedSecret = [u8; 32];

#[derive(Debug, Clone, Copy, Codec)]
struct RatchetKey {
    seed: [u8; RATCHET_SEED_SIZE],
}

#[derive(Debug, Clone, Copy, Codec)]
pub struct PublicKey {
    #[codec(with = codec::unsafe_as_raw_bytes)]
    x: x25519_dalek::PublicKey,
    #[codec(with = codec::unsafe_as_raw_bytes)]
    k: kyber::PublicKey,
}

impl RatchetKey {
    fn random_from_rng(mut rng: impl CryptoRngCore) -> Self {
        let mut seed = [0u8; RATCHET_SEED_SIZE];
        rng.fill_bytes(&mut seed);
        Self { seed }
    }

    fn expand(&self) -> (kyber::Keypair, x25519_dalek::StaticSecret) {
        let (kyber_seed, x) = unsafe { std::mem::transmute(self.seed) };
        (kyber::Keypair::new(&kyber_seed), x)
    }

    fn expand_x25519(&self) -> x25519_dalek::StaticSecret {
        let (kyber_seed, x) = unsafe { std::mem::transmute(self.seed) };
        if false {
            _ = kyber::Keypair::new(&kyber_seed);
        }
        x
    }

    fn encapsulate(
        &self,
        pk: PublicKey,
        mut rng: impl CryptoRngCore,
    ) -> (SharedSecret, Ciphertext) {
        let x = self.expand_x25519();
        let mut enc_seed_bytes = [0u8; ENC_SEEDBYTES];
        rng.fill_bytes(&mut enc_seed_bytes);
        let (kyber_shared, k) = pk.k.enc(&enc_seed_bytes);
        let x = x.diffie_hellman(&pk.x).to_bytes();
        (array::from_fn(|i| k[i] ^ x[i]), Ciphertext(kyber_shared))
    }

    fn decapsulate(&self, pk: PublicKey, cp: Ciphertext) -> Option<SharedSecret> {
        let (kyber_shared, x) = self.expand();
        let k = kyber_shared.dec(&cp.0)?;
        let x = x.diffie_hellman(&pk.x).to_bytes();
        Some(array::from_fn(|i| k[i] ^ x[i]))
    }

    fn public_key(&self) -> PublicKey {
        let (kyber, x) = self.expand();
        PublicKey { x: x25519_dalek::PublicKey::from(&x), k: kyber.publickey() }
    }
}

impl Default for RatchetKey {
    fn default() -> Self {
        Self { seed: [0u8; RATCHET_SEED_SIZE] }
    }
}

#[derive(Debug, Clone, Copy, Codec)]
struct Ciphertext([u8; CIPHERTEXTBYTES]);

impl Default for Ciphertext {
    fn default() -> Self {
        Self([0u8; CIPHERTEXTBYTES])
    }
}

#[derive(Debug, Clone, Copy, Codec, PartialEq, Eq)]
struct MessageId {
    root_index: u32,
    sub_index: u32,
}

#[derive(Debug, Clone, Copy, Codec)]
pub struct MessageHeader {
    root_index: u32,
    sub_index: u32,
    prev_sub_count: u32,
    current_key: PublicKey,
    cp: Ciphertext,
}

// TODO: encrypt headers
#[derive(Codec, Default, Clone)]
pub struct DoubleRatchet {
    index: u32,
    root_chain: SharedSecret,
    sender_chain_index: u32,
    sender_chain: SharedSecret,
    receiver_chain_index: u32,
    receiver_chain: SharedSecret,
    prev_sub_count: u32,
    rachet_key: RatchetKey,
    cp: Ciphertext,
    missing_messages: ArrayVec<(MessageId, SharedSecret), MAX_KEPT_MISSING_MESSAGES>,
}

impl DoubleRatchet {
    pub fn recipient(
        recip: PublicKey,
        mut root_chain: SharedSecret,
        mut rng: impl CryptoRngCore,
    ) -> Self {
        let rachet_key = RatchetKey::random_from_rng(&mut rng);
        let (root_input, cp) = rachet_key.encapsulate(recip, rng);
        let sender_chain = kdf_rk(&mut root_chain, root_input);
        Self { root_chain, sender_chain, rachet_key, cp, ..Default::default() }
    }

    pub fn sender(root_chain: SharedSecret, rng: impl CryptoRngCore) -> (Self, PublicKey) {
        let rachet_key = RatchetKey::random_from_rng(rng);
        (Self { root_chain, rachet_key, ..Default::default() }, rachet_key.public_key())
    }

    pub fn recv_message(
        &mut self,
        message: MessageHeader,
        mut rng: impl CryptoRngCore,
    ) -> Option<SharedSecret> {
        if message.root_index < self.index {
            return self.find_missing_message(message);
        }

        if message.root_index == self.index {
            if message.sub_index < self.receiver_chain_index {
                return self.find_missing_message(message);
            }
        } else {
            self.add_missing_message(message.prev_sub_count)?;

            let reciever_rachet_input =
                self.rachet_key.decapsulate(message.current_key, message.cp)?;
            self.rachet_key = RatchetKey::random_from_rng(&mut rng);
            let (sender_rachet_input, cp) = self.rachet_key.encapsulate(message.current_key, rng);
            self.cp = cp;

            self.prev_sub_count = std::mem::take(&mut self.sender_chain_index);
            self.receiver_chain = kdf_rk(&mut self.root_chain, reciever_rachet_input);
            self.receiver_chain_index = 0;
            self.sender_chain = kdf_rk(&mut self.root_chain, sender_rachet_input);
            self.sender_chain_index = 0;
            self.index += 1;
        }

        if self.receiver_chain == SharedSecret::default() {
            return None;
        }

        self.add_missing_message(message.sub_index)?;
        Some(kdf_sc(&mut self.receiver_chain))
    }

    pub fn send_message(&mut self) -> Option<(MessageHeader, SharedSecret)> {
        if self.sender_chain == SharedSecret::default() {
            return None;
        }

        self.index += (self.sender_chain_index == 0) as u32;
        let header = MessageHeader {
            root_index: self.index,
            sub_index: self.sender_chain_index,
            prev_sub_count: self.prev_sub_count,
            current_key: self.rachet_key.public_key(),
            cp: self.cp,
        };
        let secret = kdf_sc(&mut self.sender_chain);
        self.sender_chain_index += 1;
        Some((header, secret))
    }

    fn find_missing_message(&mut self, message: MessageHeader) -> Option<SharedSecret> {
        self.missing_messages
            .iter()
            .position(|&(header, _)| {
                header == MessageId { root_index: message.root_index, sub_index: message.sub_index }
            })
            .map(|i| self.missing_messages.remove(i).1)
    }

    #[must_use]
    fn add_missing_message(&mut self, latest_index: u32) -> Option<()> {
        if latest_index < self.receiver_chain_index {
            return None;
        }

        for sub_index in self.receiver_chain_index..latest_index {
            if let Err(val) = self.missing_messages.try_push((
                MessageId { root_index: self.index, sub_index },
                kdf_sc(&mut self.receiver_chain),
            )) {
                self.missing_messages.remove(0);
                self.missing_messages.push(val.element());
            };
        }

        self.receiver_chain_index = latest_index + 1;

        Some(())
    }
}

fn kdf_sc(root: &mut SharedSecret) -> SharedSecret {
    let (next_root, message_key) = kdf(*root, SUB_CONSTANT, "sub chain");
    *root = next_root;
    message_key
}

fn kdf_rk(root: &mut SharedSecret, rachet_input: SharedSecret) -> SharedSecret {
    let (next_root, message_key) = kdf(*root, rachet_input, "root chain");
    *root = next_root;
    message_key
}

fn kdf(
    root: SharedSecret,
    input: SharedSecret,
    info: &'static str,
) -> (SharedSecret, SharedSecret) {
    let gen = Hkdf::<Sha256>::new(Some(&root), &input);
    let mut output = [0u8; 64];
    gen.expand(info.as_bytes(), &mut output).unwrap();
    unsafe { std::mem::transmute(output) }
}

#[cfg(test)]
mod test {
    use {super::*, rand_core::OsRng, x25519_dalek::StaticSecret};

    #[test]
    fn test_sanity() {
        let rng = OsRng;

        let shared_secret = StaticSecret::random_from_rng(rng).to_bytes();
        let (mut alice, alice_pk) = DoubleRatchet::sender(shared_secret, rng);
        let mut bob = DoubleRatchet::recipient(alice_pk, shared_secret, rng);

        let mut messages = Vec::new();
        for i in 1..10 {
            for _ in 0..i {
                messages.push(bob.send_message().unwrap());
            }

            for (msg, secret) in messages.drain(..) {
                let secret2 = alice.recv_message(msg, rng).unwrap();
                assert_eq!(secret, secret2);
            }
        }

        for i in 1..10 {
            for _ in 0..i {
                messages.push(alice.send_message().unwrap());
            }

            for (msg, secret) in messages.drain(..) {
                let secret2 = bob.recv_message(msg, rng).unwrap();
                assert_eq!(secret, secret2);
            }

            std::mem::swap(&mut alice, &mut bob);
        }

        for i in 1..10 {
            for _ in 0..i {
                messages.push(alice.send_message().unwrap());
            }

            for (i, (msg, secret)) in messages.drain(..).rev().enumerate() {
                if i > MAX_KEPT_MISSING_MESSAGES {
                    break;
                }

                let secret2 = bob.recv_message(msg, rng).unwrap();
                assert_eq!(secret, secret2);
            }

            std::mem::swap(&mut alice, &mut bob);
        }
    }
}
