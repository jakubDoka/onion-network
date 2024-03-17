#![feature(slice_take)]
pub use x25519_dalek::PublicKey;
use {
    arrayvec::ArrayVec, codec::Codec, hkdf::Hkdf, rand_core::CryptoRngCore, sha2::Sha256,
    x25519_dalek::StaticSecret,
};

const MAX_KEPT_MISSING_MESSAGES: usize = 5;
const SUB_CONSTANT: SharedSecret = [0u8; 32];

type SharedSecret = [u8; 32];

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
    #[codec(with = codec::unsafe_as_raw_bytes)]
    current_key: PublicKey,
}

// TODO: encrypt headers
#[derive(Codec, Default)]
pub struct DoubleRatchet {
    index: u32,
    root_chain: SharedSecret,
    sender_chain_index: u32,
    sender_chain: SharedSecret,
    receiver_chain_index: u32,
    receiver_chain: SharedSecret,
    prev_sub_count: u32,
    rachet_key: SharedSecret,
    missing_messages: ArrayVec<(MessageId, SharedSecret), MAX_KEPT_MISSING_MESSAGES>,
}

impl DoubleRatchet {
    pub fn recipient(
        recip: PublicKey,
        mut root_chain: SharedSecret,
        rng: impl CryptoRngCore,
    ) -> Self {
        let rachet_key = StaticSecret::random_from_rng(rng);
        let root_input = rachet_key.diffie_hellman(&recip).to_bytes();
        let sender_chain = kdf_rk(&mut root_chain, root_input);
        Self { root_chain, sender_chain, rachet_key: rachet_key.to_bytes(), ..Default::default() }
    }

    pub fn sender(root_chain: SharedSecret, rng: impl CryptoRngCore) -> (Self, PublicKey) {
        let rachet_key = StaticSecret::random_from_rng(rng);
        (
            Self { root_chain, rachet_key: rachet_key.to_bytes(), ..Default::default() },
            PublicKey::from(&rachet_key),
        )
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
            self.add_missing_message(message.prev_sub_count);

            let reciever_rachet_input =
                StaticSecret::from(self.rachet_key).diffie_hellman(&message.current_key).to_bytes();
            self.rachet_key = StaticSecret::random_from_rng(&mut rng).to_bytes();
            let sender_rachet_input =
                StaticSecret::from(self.rachet_key).diffie_hellman(&message.current_key).to_bytes();

            self.prev_sub_count = std::mem::take(&mut self.sender_chain_index);
            self.receiver_chain = kdf_rk(&mut self.root_chain, reciever_rachet_input);
            self.receiver_chain_index = 0;
            self.sender_chain = kdf_rk(&mut self.root_chain, sender_rachet_input);
            self.sender_chain_index = 0;
            self.index += 1;
        }

        assert_ne!(self.receiver_chain, SharedSecret::default());

        self.add_missing_message(message.sub_index);
        Some(kdf_sc(&mut self.receiver_chain))
    }

    pub fn send_message(&mut self) -> (MessageHeader, SharedSecret) {
        assert_ne!(self.sender_chain, SharedSecret::default());

        self.index += (self.sender_chain_index == 0) as u32;
        let header = MessageHeader {
            root_index: self.index,
            sub_index: self.sender_chain_index,
            prev_sub_count: self.prev_sub_count,
            current_key: PublicKey::from(&StaticSecret::from(self.rachet_key)),
        };
        let secret = kdf_sc(&mut self.sender_chain);
        self.sender_chain_index += 1;
        (header, secret)
    }

    fn find_missing_message(&mut self, message: MessageHeader) -> Option<SharedSecret> {
        self.missing_messages
            .iter()
            .position(|&(header, _)| {
                header == MessageId { root_index: message.root_index, sub_index: message.sub_index }
            })
            .map(|i| self.missing_messages.swap_remove(i).1)
    }

    fn add_missing_message(&mut self, latest_index: u32) {
        assert!(latest_index >= self.receiver_chain_index);

        if let Some(to_truncate) = (self.missing_messages.len() + latest_index as usize
            - self.receiver_chain_index as usize)
            .checked_sub(MAX_KEPT_MISSING_MESSAGES)
        {
            self.missing_messages.drain(..to_truncate);
        }

        for sub_index in self.receiver_chain_index..latest_index {
            self.missing_messages.push((
                MessageId { root_index: self.index, sub_index },
                kdf_sc(&mut self.receiver_chain),
            ));
        }

        self.receiver_chain_index = latest_index + 1;
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
    use {super::*, rand_core::OsRng};

    #[test]
    fn test_sanity() {
        let rng = OsRng;

        let shared_secret = StaticSecret::random_from_rng(rng).to_bytes();
        let (mut alice, alice_pk) = DoubleRatchet::sender(shared_secret, rng);
        let mut bob = DoubleRatchet::recipient(alice_pk, shared_secret, rng);

        let mut messages = Vec::new();
        for i in 1..10 {
            for _ in 0..i {
                messages.push(bob.send_message());
            }

            for (msg, secret) in messages.drain(..) {
                let secret2 = alice.recv_message(msg, rng).unwrap();
                assert_eq!(secret, secret2);
            }
        }

        for i in 1..10 {
            for _ in 0..i {
                messages.push(alice.send_message());
            }

            for (msg, secret) in messages.drain(..) {
                let secret2 = bob.recv_message(msg, rng).unwrap();
                assert_eq!(secret, secret2);
            }

            std::mem::swap(&mut alice, &mut bob);
        }

        for i in 1..10 {
            dbg!(i);
            for _ in 0..i {
                messages.push(alice.send_message());
            }

            for (msg, secret) in messages.drain(..).rev() {
                let secret2 = bob.recv_message(msg, rng).unwrap();
                assert_eq!(secret, secret2);
            }

            //std::mem::swap(&mut alice, &mut bob);
        }
    }
}
