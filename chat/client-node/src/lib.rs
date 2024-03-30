#![feature(slice_take)]

use {
    crate::requests::Result,
    argon2::Argon2,
    chain_api::Profile,
    chat_spec::*,
    codec::Codec,
    crypto::{decrypt, enc, sign},
    libp2p::futures::channel::{mpsc, oneshot},
    onion::SharedSecret,
    rand::{rngs::OsRng, CryptoRng, RngCore},
    std::marker::PhantomData,
};
pub use {
    node::*,
    requests::*,
    vault::{Vault, *},
};

pub type SubscriptionMessage = Vec<u8>;
pub type RawResponse = Vec<u8>;
pub type MessageContent = String;

#[cfg(feature = "api")]
mod api;

mod chain;
mod node;
mod requests;
mod vault;

fn encrypt(mut data: Vec<u8>, secret: SharedSecret) -> Vec<u8> {
    let tag = crypto::encrypt(&mut data, secret, OsRng);
    data.extend(tag);
    data
}

fn vault_chat_404(name: ChatName) -> impl FnOnce() -> String {
    move || format!("chat {name} not found in vault")
}

pub type RequestStream = mpsc::Receiver<RequestInit>;

pub enum RequestInit {
    Request(RawRequest),
    Subscription(SubscriptionInit),
    EndSubscription(Topic),
}

impl RequestInit {
    pub fn topic(&self) -> Topic {
        match self {
            Self::Request(r) => r.topic.unwrap(),
            Self::Subscription(s) => s.topic,
            Self::EndSubscription(t) => *t,
        }
    }
}

pub struct SubscriptionInit {
    pub id: CallId,
    pub topic: Topic,
    pub channel: mpsc::Sender<SubscriptionMessage>,
}

pub struct RawRequest {
    pub id: CallId,
    pub topic: Option<Topic>,
    pub prefix: u8,
    pub payload: Vec<u8>,
    pub channel: oneshot::Sender<RawResponse>,
}

#[derive(Clone)]
pub struct UserKeys {
    pub name: UserName,
    pub sign: sign::Keypair,
    pub enc: enc::Keypair,
    pub vault: crypto::SharedSecret,
}

impl UserKeys {
    pub fn new(name: UserName, password: &str) -> Self {
        struct Entropy<'a>(&'a [u8]);

        impl RngCore for Entropy<'_> {
            fn next_u32(&mut self) -> u32 {
                unimplemented!()
            }

            fn next_u64(&mut self) -> u64 {
                unimplemented!()
            }

            fn fill_bytes(&mut self, dest: &mut [u8]) {
                let data = self.0.take(..dest.len()).expect("not enough entropy");
                dest.copy_from_slice(data);
            }

            fn try_fill_bytes(&mut self, _: &mut [u8]) -> Result<(), rand::Error> {
                unimplemented!()
            }
        }

        impl CryptoRng for Entropy<'_> {}

        const VALUT: usize = 32;
        const ENC: usize = 64 + 32;
        const SIGN: usize = 32 + 48;
        let mut bytes = [0; VALUT + ENC + SIGN];
        Argon2::default()
            .hash_password_into(password.as_bytes(), &username_to_raw(name), &mut bytes)
            .unwrap();

        let mut entropy = Entropy(&bytes);

        let sign = sign::Keypair::new(&mut entropy);
        let enc = enc::Keypair::new(&mut entropy);
        let vault = crypto::new_secret(&mut entropy);
        Self { name, sign, enc, vault }
    }

    pub fn identity_hash(&self) -> Identity {
        crypto::hash::new(self.sign.public_key())
    }

    pub fn to_identity(&self) -> Profile {
        Profile {
            sign: crypto::hash::new(self.sign.public_key()),
            enc: crypto::hash::new(self.enc.public_key()),
        }
    }
}

#[derive(Codec)]
pub struct Encrypted<T>(Vec<u8>, PhantomData<T>);

impl<T> Encrypted<T> {
    pub fn new(data: T, secret: SharedSecret) -> Self
    where
        T: for<'a> Codec<'a>,
    {
        Self(encrypt(data.to_bytes(), secret), PhantomData)
    }

    pub fn decrypt(&mut self, secret: SharedSecret) -> Option<T>
    where
        T: for<'a> Codec<'a>,
    {
        let data = decrypt(&mut self.0, secret)?;
        T::decode(&mut &data[..])
    }
}
