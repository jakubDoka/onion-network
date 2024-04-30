#![feature(slice_take)]
#![feature(let_chains)]

pub use kyber::PublicKey;
use {
    arrayvec::ArrayVec,
    codec::Codec,
    hkdf::Hkdf,
    kyber::{CryptoRng, RngCore},
    rand_core::CryptoRngCore,
    sha2::Sha256,
    std::array,
    x25519_dalek::StaticSecret,
};

const MAX_KEPT_MISSING_MESSAGES: usize = 5;
const KYBER_PERIOD: u32 = 3;
const SUB_CONSTANT: SharedSecret = [0u8; 32];

type SharedSecret = [u8; 32];
type Ciphertext = [u8; kyber::KYBER_CIPHERTEXTBYTES];
type KyberSeed = [u8; 64];
type Hash = [u8; 32];

pub struct InitiatiorKey {
    x25519: x25519_dalek::PublicKey,
    kyber: kyber::PublicKey,
}

// TODO: encrypt headers
#[derive(Codec, Clone)]
pub struct DoubleRatchet {
    root: Chain,
    sender: Chain,
    receiver_hash: Hash,
    receiver: Chain,
    sender_hash: Hash,
    prev_sub_count: u32,
    kyber: KyberSeed,
    cp: Ciphertext,
    ss: SharedSecret,
    #[codec(with = codec::unsafe_as_raw_bytes)]
    x25519: StaticSecret,
    missing_messages: ArrayVec<(MessageId, SharedSecret), MAX_KEPT_MISSING_MESSAGES>,
}

impl DoubleRatchet {
    pub fn recipient(
        root: SharedSecret,
        init: InitiatiorKey,
        mut rng: impl CryptoRngCore,
    ) -> (Self, SharedSecret) {
        let kyber: KyberSeed = random_array(&mut rng);
        let x25519 = StaticSecret::random_from_rng(&mut rng);

        let mut root = Chain::from(root);
        let sender_hash = blake3::hash(&root.root).into();
        let id = root.next_sc();
        let receiver = Chain::new_init(root.next_sc());
        let receiver_hash = blake3::hash(&receiver.root).into();

        let dh = x25519.diffie_hellman(&init.x25519).to_bytes();
        let (cp, ss) = kyber::encapsulate(&init.kyber, &mut rng).unwrap();

        let sender = Chain::new_guarding(dh);

        let s = Self {
            receiver,
            sender,
            sender_hash,
            receiver_hash,
            root,
            kyber,
            cp,
            ss,
            x25519,
            ..unsafe { std::mem::zeroed() }
        };
        (s, id)
    }

    pub fn sender(
        root: SharedSecret,
        mut rng: impl CryptoRngCore,
    ) -> (Self, InitiatiorKey, SharedSecret) {
        let kyber: KyberSeed = random_array(&mut rng);
        let x25519 = StaticSecret::random_from_rng(&mut rng);
        let mut root = Chain::from(root);
        let receiver_hash = blake3::hash(&root.root).into();
        let id = root.next_sc();
        let sender = Chain::new_init(root.next_sc());
        let sender_hash = blake3::hash(&sender.root).into();
        let key = InitiatiorKey {
            x25519: (&x25519).into(),
            kyber: kyber::Keypair::generate(&mut SliceRng(&kyber)).unwrap().public,
        };
        let s = Self {
            sender,
            sender_hash,
            receiver_hash,
            root,
            kyber,
            x25519,
            ..unsafe { std::mem::zeroed() }
        };
        (s, key, id)
    }

    pub fn receiver_hash(&self) -> Hash {
        self.receiver_hash
    }

    pub fn sender_hash(&self) -> Hash {
        self.sender_hash
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

            let mut receiver_input = self.x25519.diffie_hellman(&message.x25519).to_bytes();
            self.x25519 = StaticSecret::random_from_rng(&mut rng);
            if let Some(kyb) = message.kyber {
                let sec = kyber::Keypair::generate(&mut SliceRng(&self.kyber)).unwrap().secret;
                let sec = kyber::decapsulate(&kyb.cp, &sec).ok().ok_or(RecvError::Decapsulation)?;
                receiver_input = xor(receiver_input, sec);
                self.kyber = random_array(&mut rng);
                let (cp, ss) = kyber::encapsulate(&kyb.public, &mut rng).unwrap();
                self.cp = cp;
                self.ss = ss;
            }

            self.prev_sub_count = std::mem::take(&mut self.sender.step);
            self.receiver = self.root.next_rk(receiver_input).into();
            self.receiver_hash = blake3::hash(&self.receiver.root).into();
            self.sender.guard(self.x25519.diffie_hellman(&message.x25519).to_bytes());
        }

        self.add_missing_message(message.id.sub_step)?;
        Ok(self.receiver.next_sc())
    }

    pub fn send(&mut self) -> (MessageHeader, SharedSecret) {
        if self.sender.step == u32::MAX {
            let mut seed = self.sender.root;
            if (self.root.step + 1) % KYBER_PERIOD == 0 {
                seed = xor(seed, self.ss);
            }
            self.sender = self.root.next_rk(seed).into();
            self.sender_hash = blake3::hash(&self.sender.root).into();
        }

        let header = MessageHeader {
            id: MessageId { step: self.root.step, sub_step: self.sender.step },
            prev_sub_count: self.prev_sub_count,
            // odd number to make participants alternate
            kyber: (self.root.step % KYBER_PERIOD == 0).then(|| Kyber {
                public: kyber::Keypair::generate(&mut SliceRng(&self.kyber)).unwrap().public,
                cp: {
                    debug_assert_ne!(self.cp, [0u8; kyber::KYBER_CIPHERTEXTBYTES]);
                    self.cp
                },
            }),
            x25519: (&self.x25519).into(),
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
    Decapsulation,
    InvalidState,
}

impl std::fmt::Display for RecvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecvError::MissingMessage => write!(f, "missing message"),
            RecvError::Decapsulation => write!(f, "decapsulation failed"),
            RecvError::InvalidState => write!(f, "invalid state"),
        }
    }
}

impl std::error::Error for RecvError {}

#[derive(Debug, Clone, Copy, Codec, PartialEq, Eq)]
struct MessageId {
    step: u32,
    sub_step: u32,
}

#[derive(Debug, Clone, Copy, Codec)]
pub struct MessageHeader {
    id: MessageId,
    prev_sub_count: u32,
    kyber: Option<Kyber>,
    #[codec(with = codec::unsafe_as_raw_bytes)]
    x25519: x25519_dalek::PublicKey,
}

#[derive(Debug, Clone, Copy, Codec)]
pub struct Kyber {
    #[codec(with = codec::unsafe_as_raw_bytes)]
    public: kyber::PublicKey,
    cp: Ciphertext,
}

impl MessageHeader {
    pub fn is_major(&self) -> bool {
        self.kyber.is_some()
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

    pub fn new_guarding(root: SharedSecret) -> Self {
        Self { step: u32::MAX, root }
    }

    pub fn next_sc(&mut self) -> SharedSecret {
        self.step += 1;
        kdf_sc(&mut self.root)
    }

    pub fn next_rk(&mut self, rachet_input: SharedSecret) -> SharedSecret {
        self.step += 1;
        kdf_rk(&mut self.root, rachet_input)
    }

    fn guard(&mut self, sender_input: SharedSecret) {
        self.root = sender_input;
        self.step = u32::MAX;
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

fn random_array<const N: usize>(rng: &mut impl CryptoRngCore) -> [u8; N] {
    let mut array = [0u8; N];
    rng.fill_bytes(&mut array);
    array
}

struct SliceRng<'a>(&'a [u8]);

impl RngCore for SliceRng<'_> {
    fn next_u32(&mut self) -> u32 {
        unimplemented!()
    }

    fn next_u64(&mut self) -> u64 {
        unimplemented!()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        dest.copy_from_slice(self.0.take(..dest.len()).unwrap());
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl<'a> Drop for SliceRng<'a> {
    fn drop(&mut self) {
        debug_assert!(self.0.is_empty());
    }
}

impl CryptoRng for SliceRng<'_> {}

#[cfg(test)]
mod test {
    use {super::*, rand_core::OsRng};

    fn match_rs(alice: &DoubleRatchet, bob: &DoubleRatchet) {
        assert_eq!(alice.sender_hash(), bob.receiver_hash());
        assert_eq!(alice.receiver_hash(), bob.sender_hash());
    }

    #[test]
    fn differned_deliver_orders() {
        let rng = OsRng;

        let shared_secret = StaticSecret::random_from_rng(rng).to_bytes();
        let (mut alice, init, aid) = DoubleRatchet::sender(shared_secret, rng);
        let (mut bob, bid) = DoubleRatchet::recipient(shared_secret, init, rng);
        assert_eq!(aid, bid);

        match_rs(&alice, &bob);

        let (msg, secret) = alice.send();
        let secret2 = bob.recv(msg, rng).unwrap();
        assert_eq!(secret, secret2);

        match_rs(&alice, &bob);

        let (msg, secret) = bob.send();
        let secret2 = alice.recv(msg, rng).unwrap();
        assert_eq!(secret, secret2);

        match_rs(&alice, &bob);

        let (msg, secret) = alice.send();
        let secret2 = bob.recv(msg, rng).unwrap();
        assert_eq!(secret, secret2);

        match_rs(&alice, &bob);
    }

    #[test]
    fn test_sanity() {
        let rng = OsRng;

        let shared_secret = StaticSecret::random_from_rng(rng).to_bytes();
        let (mut alice, init, _) = DoubleRatchet::sender(shared_secret, rng);
        let (mut bob, _) = DoubleRatchet::recipient(shared_secret, init, rng);

        let mut messages = Vec::new();
        for i in 1..10 {
            for _ in 0..i {
                messages.push(bob.send());
            }

            for (msg, secret) in messages.drain(..) {
                let secret2 = alice.recv(msg, rng).unwrap();
                assert_eq!(secret, secret2);
                match_rs(&alice, &bob);
            }
        }

        for i in 1..10 {
            for _ in 0..i {
                messages.push(alice.send());
            }

            for (msg, secret) in messages.drain(..) {
                let secret2 = bob.recv(msg, rng).unwrap();
                assert_eq!(secret, secret2, "{:?} ======= {:?}", bob.cp, alice.cp);
                match_rs(&alice, &bob);
            }

            std::mem::swap(&mut alice, &mut bob);
        }

        for i in 1..10 {
            for _ in 0..i {
                messages.push(alice.send());
            }

            for (i, (msg, secret)) in messages.drain(..).rev().enumerate() {
                if i > MAX_KEPT_MISSING_MESSAGES {
                    break;
                }

                let secret2 = bob.recv(msg, rng).unwrap();
                assert_eq!(secret, secret2);
                match_rs(&alice, &bob);
            }

            std::mem::swap(&mut alice, &mut bob);
        }
    }
}
