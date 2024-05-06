#![feature(slice_take)]
#![feature(btree_extract_if)]
#![feature(macro_metavar_expr)]
#![feature(let_chains)]
#![feature(never_type)]
#![feature(type_alias_impl_trait)]

#[macro_export]
macro_rules! wasm_if {
    ($wasm:stmt $(, $not_wasm:stmt)?) => {
        #[cfg(target_arch = "wasm32")]
        $wasm;
        $(
            #[cfg(not(target_arch = "wasm32"))]
            $not_wasm;
        )?
    };
}

use {
    anyhow::Context,
    argon2::Argon2,
    chain_api::{Nonce, Profile},
    chat_spec::*,
    codec::{DecodeOwned, Encode},
    crypto::{enc, sign},
    libp2p::futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    rand::{CryptoRng, RngCore},
    std::{future::Future, io, sync::Arc, time::Duration},
    storage_spec::ClientError,
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

mod api;

mod chain;
mod node;
mod requests;
mod vault;

pub fn encode_direct_chat_name(name: UserName) -> String {
    format!("{}{}", name, " ".repeat(32))
}

pub trait RecoverMail {
    fn recover_mail(self) -> anyhow::Result<()>;
}

impl RecoverMail for anyhow::Result<()> {
    fn recover_mail(self) -> anyhow::Result<()> {
        self.or_else(|e| {
            e.root_cause()
                .downcast_ref::<ChatError>()
                .and_then(|e| match e {
                    ChatError::SentDirectly => Some(Ok(())),
                    _ => None,
                })
                .ok_or(e)?
        })
    }
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
    pub hot_wallet: crypto::SharedSecret,
    pub chain_url: Arc<str>,
}

impl UserKeys {
    pub fn new(name: UserName, password: &str, chain_url: &str) -> Self {
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
        const HOT: usize = 32;
        let mut bytes = [0; VALUT + ENC + SIGN + HOT];
        Argon2::default()
            .hash_password_into(password.as_bytes(), &username_to_raw(name), &mut bytes)
            .unwrap();

        let mut entropy = Entropy(&bytes);

        let sign = sign::Keypair::new(&mut entropy);
        let enc = enc::Keypair::new(&mut entropy);
        let vault = crypto::new_secret(&mut entropy);
        let hot_wallet = crypto::new_secret(&mut entropy);
        Self { name, sign, enc, vault, hot_wallet, chain_url: chain_url.into() }
    }

    pub fn identity(&self) -> Identity {
        self.sign.identity()
    }

    pub fn to_identity(&self) -> Profile {
        Profile { sign: self.sign.identity(), enc: crypto::hash::new(self.enc.public_key()) }
    }

    pub fn chain_client(&self) -> impl Future<Output = chain_api::Result<chain::Client>> {
        let kp = chain_api::Keypair::from_seed(self.vault).expect("right amount of seed");
        let chain_url = self.chain_url.clone();
        async move { chain_api::Client::with_signer(&chain_url, kp).await }
    }

    pub async fn register(&self) -> anyhow::Result<()> {
        let client = self.chain_client().await.context("connecting to chain")?;

        let username = username_to_raw(self.name);

        if client.user_exists(username).await.context("checking if username is free")? {
            anyhow::bail!("user with this name already exists");
        }

        let nonce = client.get_nonce().await.context("fetching nonce")?;
        client.register(username, self.to_identity(), nonce).await.context("registering")?;

        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn timeout<F: Future>(duration: Duration, f: F) -> Option<F::Output> {
    use {
        std::task::Poll,
        web_sys::{
            wasm_bindgen::{closure::Closure, JsCast},
            window,
        },
    };

    let mut fut = std::pin::pin!(f);
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

#[cfg(not(target_arch = "wasm32"))]
pub async fn timeout<F: Future>(duration: Duration, f: F) -> Option<F::Output> {
    tokio::time::timeout(duration, f).await.ok()
}

pub async fn send_request<R: DecodeOwned>(
    stream: &mut (impl AsyncRead + AsyncWrite + Unpin),
    id: Prefix,
    topic: impl Into<Topic>,
    req: impl Encode,
) -> anyhow::Result<R> {
    timeout(
        Duration::from_secs(5),
        send_request_low::<Result<R, ChatError>>(stream, id, topic, req),
    )
    .await
    .context("timeout")??
    .map_err(Into::into)
}

pub async fn send_request_low<R: DecodeOwned>(
    stream: &mut (impl AsyncRead + AsyncWrite + Unpin),
    id: Prefix,
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
    stream.write_all(&req.to_bytes()).await?;
    stream.flush().await?;

    let mut header = [0; std::mem::size_of::<ResponseHeader>()];
    stream.read_exact(&mut header).await?;
    let header = ResponseHeader::from_array(header);
    let mut buf = vec![0; header.get_len()];
    stream.read_exact(&mut buf).await?;
    R::decode_exact(&buf).ok_or(io::ErrorKind::InvalidData.into())
}

pub fn spawn(fut: impl Future + Send + 'static) {
    let task = async move { _ = fut.await };
    wasm_if!(wasm_bindgen_futures::spawn_local(task), tokio::spawn(task));
}
