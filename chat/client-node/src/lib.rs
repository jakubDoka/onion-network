#![feature(slice_take)]
#![feature(never_type)]
#![feature(type_alias_impl_trait)]

use {
    anyhow::Context,
    argon2::Argon2,
    chain_api::{Nonce, Profile},
    chat_spec::*,
    codec::{DecodeOwned, Encode},
    crypto::{enc, sign},
    libp2p::futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    rand::{CryptoRng, RngCore},
    std::{future::Future, io, pin, task::Poll, time::Duration},
    storage_spec::ClientError,
    web_sys::{
        wasm_bindgen::{closure::Closure, JsCast},
        window,
    },
};
pub use {
    chain::*,
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

pub fn encode_direct_chat_name(name: UserName) -> String {
    format!("{}{}", name, " ".repeat(32))
}

pub trait DowncastNonce<O> {
    fn downcast_nonce(self) -> anyhow::Result<Result<O, Nonce>>;
}

impl<O> DowncastNonce<O> for anyhow::Result<O> {
    fn downcast_nonce(self) -> anyhow::Result<Result<O, Nonce>> {
        self.map(Ok).or_else(|e| {
            e.root_cause()
                .downcast_ref::<ClientError>()
                .and_then(|e| match e {
                    ClientError::InvalidNonce(n) => Some(Err(*n)),
                    _ => None,
                })
                .ok_or(e)
        })
    }
}

//pub struct RawStorageRequest {
//    pub prefix: u8,
//    pub payload: Vec<u8>,
//    pub identity: NodeIdentity,
//    pub addr: SocketAddr,
//    pub response: Result<oneshot::Sender<libp2p::Stream>, oneshot::Sender<RawResponse>>,
//}
//
//pub struct RawSateliteRequest {
//    pub prefix: u8,
//    pub payload: Vec<u8>,
//    pub identity: NodeIdentity,
//    pub response: oneshot::Sender<RawResponse>,
//}

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
                let data = (&mut self.0).take(..dest.len()).expect("not enough entropy");
                dest.copy_from_slice(data);
            }

            fn try_fill_bytes(&mut self, bytes: &mut [u8]) -> std::result::Result<(), rand::Error> {
                self.fill_bytes(bytes);
                Ok(())
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

    pub fn identity(&self) -> Identity {
        crypto::hash::new(self.sign.public_key())
    }

    pub fn to_identity(&self) -> Profile {
        Profile {
            sign: crypto::hash::new(self.sign.public_key()),
            enc: crypto::hash::new(self.enc.public_key()),
        }
    }
}

pub async fn timeout<F: Future>(f: F, duration: Duration) -> Option<F::Output> {
    let mut fut = pin::pin!(f);
    let mut callback = None::<(Closure<dyn FnMut()>, i32)>;
    let until = instant::Instant::now() + duration;
    std::future::poll_fn(|cx| {
        if let Poll::Ready(v) = fut.as_mut().poll(cx) {
            if let Some((_cl, handle)) = callback.take() {
                window().unwrap().clear_timeout_with_handle(handle);
            }

            return Poll::Ready(Some(v));
        }

        if until < instant::Instant::now() {
            return Poll::Ready(None);
        }

        if callback.is_none() {
            let waker = cx.waker().clone();
            let handler = Closure::once(move || waker.wake());
            let handle = window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    handler.as_ref().unchecked_ref(),
                    duration.as_millis() as i32,
                )
                .unwrap();
            callback = Some((handler, handle));
        }

        Poll::Pending
    })
    .await
}

pub async fn send_request<R: DecodeOwned>(
    stream: &mut (impl AsyncRead + AsyncWrite + Unpin),
    id: u8,
    topic: impl Into<Topic>,
    req: impl Encode,
) -> anyhow::Result<R> {
    timeout(
        send_request_low::<Result<R, ChatError>>(stream, id, topic, req),
        Duration::from_secs(5),
    )
    .await
    .context("timeout")??
    .map_err(Into::into)
}

pub async fn send_request_low<R: DecodeOwned>(
    stream: &mut (impl AsyncRead + AsyncWrite + Unpin),
    id: u8,
    topic: impl Into<Topic>,
    req: impl Encode,
) -> io::Result<R> {
    let topic = topic.into();
    let len = req.encoded_len();
    let header = RequestHeader {
        prefix: id,
        call_id: [0; 4],
        topic: topic.compress(),
        len: (len as u32).to_be_bytes(),
    };
    stream.write_all(header.as_bytes()).await?;
    log::debug!("");
    stream.write_all(&req.to_bytes()).await?;
    log::debug!("");
    stream.flush().await?;
    log::debug!("");

    let mut header = [0; std::mem::size_of::<ResponseHeader>()];
    stream.read_exact(&mut header).await?;
    log::debug!("header: {:?}", header);
    let header = ResponseHeader::from_array(header);
    let mut buf = vec![0; header.get_len()];
    stream.read_exact(&mut buf).await?;
    R::decode(&mut buf.as_slice()).ok_or(io::ErrorKind::InvalidData.into())
}
