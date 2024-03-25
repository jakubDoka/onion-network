#![feature(slice_take)]
#![feature(let_chains)]
use {
    arrayvec::ArrayVec,
    codec::Codec,
    hkdf::Hkdf,
    kyber::{CIPHERTEXTBYTES, ENC_SEEDBYTES},
    rand_core::CryptoRngCore,
    sha2::Sha256,
    std::array,
};
pub use {kyber::PublicKey, x25519_dalek::PublicKey as InitiatiorKey};

const MAX_KEPT_MISSING_MESSAGES: usize = 5;
const SUB_CONSTANT: SharedSecret = [0u8; 32];
const RATCHET_SEED_SIZE: usize = 32 + 64;

type SharedSecret = [u8; 32];

// TODO: encrypt headers
#[derive(Codec, Default, Clone)]
pub struct DoubleRatchet {
    root: Chain,
    sender: Chain,
    receiver: Chain,
    key: RatchetKey,
    prev_sub_count: u32,
    prev_key: SharedSecret,
    last_x25519: [u8; 32],
    missing_messages: ArrayVec<(MessageId, SharedSecret), MAX_KEPT_MISSING_MESSAGES>,
}

impl DoubleRatchet {
    pub fn recipient(root: SharedSecret, init: InitiatiorKey, mut rng: impl CryptoRngCore) -> Self {
        let mut root = Chain::from(root);
        let mut prev_key = [0u8; 32];
        rng.fill_bytes(&mut prev_key);
        Self {
            receiver: Chain::new_init(root.next_sc()),
            sender: root.next_rk(prev_key).into(),
            root,
            key: RatchetKey::random_from_rng(&mut rng),
            prev_key,
            last_x25519: init.to_bytes(),
            ..Default::default()
        }
    }

    pub fn sender(root: SharedSecret, rng: impl CryptoRngCore) -> (Self, InitiatiorKey) {
        let rachet_key = RatchetKey::random_from_rng(rng);
        let mut root = Chain::from(root);
        let sender = Chain::new_init(root.next_sc());
        (Self { sender, root, key: rachet_key, ..Default::default() }, rachet_key.x25519_pk())
    }

    pub fn public_key(&self) -> kyber::PublicKey {
        self.key.expand().0.publickey()
    }

    pub fn recv(
        &mut self,
        message: MessageHeader,
        mut rng: impl CryptoRngCore,
    ) -> Result<SharedSecret, RecvError> {
        if message.id.step < self.root.step {
            return self.find_missing_message(message);
        }

        if message.id.step == self.root.step {
            if message.id.sub_step < self.receiver.step {
                return self.find_missing_message(message);
            }
        } else {
            self.add_missing_message(message.prev_sub_count)?;

            let exchanged = match message.cp {
                Some(cp) => self.key.decapsulate(message.x25519, cp)?,
                None => self.key.expand_x25519().diffie_hellman(&message.x25519).to_bytes(),
            };
            let reciever_input = xor(exchanged, message.xored_secret);

            self.key = RatchetKey::random_from_rng(&mut rng);

            rng.fill_bytes(&mut self.prev_key);
            self.prev_sub_count = std::mem::take(&mut self.sender.step);
            self.last_x25519 = message.x25519.to_bytes();

            self.receiver = self.root.next_rk(reciever_input).into();
            self.sender = self.root.next_rk(self.prev_key).into();
            self.root.step += 1;
        }

        self.add_missing_message(message.id.sub_step)?;
        Ok(self.receiver.next_sc())
    }

    pub fn send(
        &mut self,
        recip_pk: PublicKey,
        rng: impl CryptoRngCore,
    ) -> (MessageHeader, SharedSecret) {
        self.root.step += (self.sender.step == 0) as u32;

        let (ss, cp) = if self.root.step % 64 < 2 {
            let (ss, cp) = self.key.encapsulate(recip_pk, self.last_x25519.into(), rng);
            (ss, Some(cp))
        } else {
            let ss = self.key.expand_x25519().diffie_hellman(&self.last_x25519.into());
            (ss.to_bytes(), None)
        };

        let header = MessageHeader {
            id: MessageId { step: self.root.step, sub_step: self.sender.step },
            prev_sub_count: self.prev_sub_count,
            cp,
            x25519: self.key.x25519_pk(),
            xored_secret: xor(ss, self.prev_key),
        };
        (header, self.sender.next_sc())
    }

    fn find_missing_message(&mut self, message: MessageHeader) -> Result<SharedSecret, RecvError> {
        self.missing_messages
            .iter()
            .position(|&(header, _)| header == message.id)
            .map(|i| self.missing_messages.remove(i).1)
            .ok_or(RecvError::MissingMessage)
    }

    fn add_missing_message(&mut self, latest_index: u32) -> Result<(), RecvError> {
        if latest_index < self.receiver.step {
            return Err(RecvError::InvalidState);
        }

        for sub_step in self.receiver.step..latest_index {
            if self.missing_messages.is_full() {
                self.missing_messages.remove(0);
            }

            self.missing_messages
                .push((MessageId { step: self.root.step, sub_step }, self.receiver.next_sc()));
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum RecvError {
    MissingMessage,
    Encapsulation,
    InvalidState,
}

#[derive(Debug, Clone, Copy, Codec)]
struct RatchetKey {
    seed: [u8; RATCHET_SEED_SIZE],
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

    fn x25519_pk(&self) -> x25519_dalek::PublicKey {
        (&self.expand_x25519()).into()
    }

    fn encapsulate(
        &self,
        kyber: kyber::PublicKey,
        x25519: x25519_dalek::PublicKey,
        mut rng: impl CryptoRngCore,
    ) -> (SharedSecret, Ciphertext) {
        let x = self.expand_x25519();
        let mut enc_seed_bytes = [0u8; ENC_SEEDBYTES];
        rng.fill_bytes(&mut enc_seed_bytes);
        let (kyber_shared, k) = kyber.enc(&enc_seed_bytes);
        let x = x.diffie_hellman(&x25519).to_bytes();
        (xor(x, k), Ciphertext(kyber_shared))
    }

    fn decapsulate(
        &self,
        x25519: x25519_dalek::PublicKey,
        cp: Ciphertext,
    ) -> Result<SharedSecret, RecvError> {
        let (kyber_shared, x) = self.expand();
        let k = kyber_shared.dec(&cp.0).ok_or(RecvError::Encapsulation)?;
        let x = x.diffie_hellman(&x25519).to_bytes();
        Ok(xor(k, x))
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
    step: u32,
    sub_step: u32,
}

#[derive(Debug, Clone, Copy, Codec)]
pub struct MessageHeader {
    id: MessageId,
    prev_sub_count: u32,
    cp: Option<Ciphertext>,
    #[codec(with = codec::unsafe_as_raw_bytes)]
    x25519: x25519_dalek::PublicKey,
    xored_secret: SharedSecret,
}

impl MessageHeader {
    pub fn is_major(&self) -> bool {
        self.cp.is_some()
    }
}

#[derive(Clone, Copy, Default, Codec)]
struct Chain {
    step: u32,
    root: SharedSecret,
}

impl From<SharedSecret> for Chain {
    fn from(root: SharedSecret) -> Self {
        Self { step: 0, root }
    }
}

impl Chain {
    pub fn new_init(root: SharedSecret) -> Self {
        Self { step: 1, root }
    }

    pub fn next_sc(&mut self) -> SharedSecret {
        self.step += 1;
        kdf_sc(&mut self.root)
    }

    pub fn next_rk(&mut self, rachet_input: SharedSecret) -> SharedSecret {
        kdf_rk(&mut self.root, rachet_input)
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

fn xor(a: SharedSecret, b: SharedSecret) -> SharedSecret {
    array::from_fn(|i| a[i] ^ b[i])
}

#[cfg(test)]
mod test {
    use {super::*, rand_core::OsRng, x25519_dalek::StaticSecret};

    #[test]
    fn differned_deliver_orders() {
        let rng = OsRng;

        let shared_secret = StaticSecret::random_from_rng(rng).to_bytes();
        let (mut alice, init) = DoubleRatchet::sender(shared_secret, rng);
        let mut bob = DoubleRatchet::recipient(shared_secret, init, rng);

        let (msg, secret) = alice.send(bob.public_key(), rng);
        let secret2 = bob.recv(msg, rng).unwrap();
        assert_eq!(secret, secret2);
    }

    #[test]
    fn test_sanity() {
        let rng = OsRng;

        let shared_secret = StaticSecret::random_from_rng(rng).to_bytes();
        let (mut alice, init) = DoubleRatchet::sender(shared_secret, rng);
        let mut bob = DoubleRatchet::recipient(shared_secret, init, rng);

        let mut messages = Vec::new();
        for i in 1..10 {
            for _ in 0..i {
                messages.push(bob.send(alice.public_key(), rng));
            }

            for (msg, secret) in messages.drain(..) {
                let secret2 = alice.recv(msg, rng).unwrap();
                assert_eq!(secret, secret2);
            }
        }

        for i in 1..10 {
            for _ in 0..i {
                messages.push(alice.send(bob.public_key(), rng));
            }

            for (msg, secret) in messages.drain(..) {
                let secret2 = bob.recv(msg, rng).unwrap();
                assert_eq!(secret, secret2);
            }

            std::mem::swap(&mut alice, &mut bob);
        }

        for i in 1..10 {
            for _ in 0..i {
                messages.push(alice.send(bob.public_key(), rng));
            }

            for (i, (msg, secret)) in messages.drain(..).rev().enumerate() {
                if i > MAX_KEPT_MISSING_MESSAGES {
                    break;
                }

                let secret2 = bob.recv(msg, rng).unwrap();
                assert_eq!(secret, secret2);
            }

            std::mem::swap(&mut alice, &mut bob);
        }
    }
}
