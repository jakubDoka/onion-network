#![feature(let_chains)]
#![feature(slice_take)]
#![feature(iter_next_chunk)]
#![feature(impl_trait_in_assoc_type)]
use {
    crypto::{enc, sign, SharedSecret},
    futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, Future},
    libp2p_core::{
        upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade},
        UpgradeInfo,
    },
    libp2p_identity::PeerId,
    multihash::Multihash,
    rand_core::CryptoRngCore,
    std::{io, ops::DerefMut, pin::Pin, task::Poll, u16, usize},
};

#[derive(Clone)]
pub struct Config<R> {
    kp: sign::Keypair,
    rng: R,
    buffer_size: usize,
}

impl<R> Config<R> {
    pub fn new(rng: R, kp: sign::Keypair) -> Self {
        Self { kp, rng, buffer_size: 1024 * 32 }
    }

    /// configure buffer size, must be greater than crypto::TAG_SIZE
    /// if not set, default is 1024 * 32, the `Output` actually uses 2 of this sized buffers.
    pub fn with_buffer_size(self, buffer_size: usize) -> Self {
        assert!(
            buffer_size > crypto::TAG_SIZE,
            "buffer size must be greater than {}",
            crypto::TAG_SIZE
        );
        Self { buffer_size, ..self }
    }
}

impl<R> UpgradeInfo for Config<R> {
    type Info = &'static str;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once(concat!("/", env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
    }
}

struct InitRequest {
    challinge: SharedSecret,
    sign_pk: sign::PublicKey,
    enc_pk: enc::PublicKey,
}

struct InitResponse {
    challinge: SharedSecret,
    sign_pk: sign::PublicKey,
    enc_pk: enc::PublicKey,
    proof: sign::Signature,
    cp: enc::Ciphertext,
}

struct FinalRequest {
    cp: enc::Ciphertext,
    proof: sign::Signature,
}

const fn max(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

const MAX_BUFFER: usize = max(
    max(std::mem::size_of::<InitRequest>(), std::mem::size_of::<InitResponse>()),
    std::mem::size_of::<FinalRequest>(),
);

impl<R, T> InboundConnectionUpgrade<T> for Config<R>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    R: CryptoRngCore + Send + 'static,
{
    type Error = Error;
    type Output = (PeerId, Output<R, T>);

    type Future = impl Future<Output = Result<Self::Output, Self::Error>> + Send;

    fn upgrade_inbound(mut self, mut socket: T, _: Self::Info) -> Self::Future {
        // TODO: save as seed between awaits
        let temp_key = enc::Keypair::new(&mut self.rng);
        let challinge = crypto::new_secret(&mut self.rng);
        async move {
            let mut buf = [0; MAX_BUFFER];
            Pin::new(&mut socket)
                .read_exact(&mut buf[..std::mem::size_of::<InitRequest>()])
                .await?;

            let sender_sign_pk;
            let sender_ss;

            // we keep scopes to drop big variables so that future is not bloated
            {
                let cp;
                let proof;
                // we also keep scopes to discard unsafe references
                {
                    let init_req = unsafe { std::mem::transmute::<_, &InitRequest>(&buf) };
                    sender_sign_pk = init_req.sign_pk;
                    (cp, sender_ss) = temp_key.encapsulate(&init_req.enc_pk, &mut self.rng);
                    proof = self.kp.sign(&init_req.challinge, &mut self.rng);
                }

                let init_resp = unsafe { std::mem::transmute::<_, &mut InitResponse>(&mut buf) };
                *init_resp = InitResponse {
                    challinge,
                    sign_pk: self.kp.public_key(),
                    enc_pk: temp_key.public_key(),
                    proof,
                    cp,
                };
            };

            Pin::new(&mut socket).write_all(&buf[..std::mem::size_of::<InitResponse>()]).await?;
            Pin::new(&mut socket).flush().await?;
            Pin::new(&mut socket)
                .read_exact(&mut buf[..std::mem::size_of::<FinalRequest>()])
                .await?;

            let final_req = unsafe { std::mem::transmute::<_, &FinalRequest>(&buf) };
            sender_sign_pk.verify(&challinge, &final_req.proof)?;
            let receiver_ss = temp_key.decapsulate(&final_req.cp)?;
            let peer_id = hash_to_peer_id(sender_sign_pk.identity());

            Ok((peer_id, Output::new(socket, self.rng, sender_ss, receiver_ss, self.buffer_size)))
        }
    }
}

impl<R, T> OutboundConnectionUpgrade<T> for Config<R>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    R: CryptoRngCore + Send + 'static,
{
    type Error = Error;
    type Output = (PeerId, Output<R, T>);

    type Future = impl Future<Output = Result<Self::Output, Self::Error>> + Send;

    fn upgrade_outbound(mut self, mut socket: T, _: Self::Info) -> Self::Future {
        let temp_key = enc::Keypair::new(&mut self.rng);
        let challinge = crypto::new_secret(&mut self.rng);
        async move {
            let mut buf = [0; MAX_BUFFER];
            {
                let init_req = unsafe { std::mem::transmute::<_, &mut InitRequest>(&mut buf) };
                *init_req = InitRequest {
                    challinge,
                    sign_pk: self.kp.public_key(),
                    enc_pk: temp_key.public_key(),
                };
            }
            Pin::new(&mut socket).write_all(&buf[..std::mem::size_of::<InitRequest>()]).await?;
            Pin::new(&mut socket).flush().await?;
            Pin::new(&mut socket)
                .read_exact(&mut buf[..std::mem::size_of::<InitResponse>()])
                .await?;

            let sender_ss;
            let receiver_ss;
            let identity;
            {
                let proof;
                let cp;
                {
                    let init_resp = unsafe { std::mem::transmute::<_, &InitResponse>(&buf) };
                    init_resp.sign_pk.verify(&challinge, &init_resp.proof)?;
                    receiver_ss = temp_key.decapsulate(&init_resp.cp)?;
                    proof = self.kp.sign(&init_resp.challinge, &mut self.rng);
                    (cp, sender_ss) = temp_key.encapsulate(&init_resp.enc_pk, &mut self.rng);
                    identity = init_resp.sign_pk.identity();
                }

                let final_req = unsafe { std::mem::transmute::<_, &mut FinalRequest>(&mut buf) };
                *final_req = FinalRequest { cp, proof };
            }

            Pin::new(&mut socket).write_all(&buf[..std::mem::size_of::<FinalRequest>()]).await?;
            Pin::new(&mut socket).flush().await?;

            let peer_id = hash_to_peer_id(identity);

            Ok((peer_id, Output::new(socket, self.rng, sender_ss, receiver_ss, self.buffer_size)))
        }
    }
}

pub struct Output<R, T> {
    stream: T,
    rng: R,
    sender_ss: SharedSecret,
    receiver_ss: SharedSecret,
    read_buffer: Vec<u8>,
    readable_end: usize,
    readable_start: usize,
    chunk_reminder: usize,
    write_buffer: Vec<u8>,
    writable_end: usize,
    writable_start: usize,
}

impl<R, T> Output<R, T> {
    pub fn new(
        stream: T,
        rng: R,
        sender_ss: SharedSecret,
        receiver_ss: SharedSecret,
        _buffer_cap: usize,
    ) -> Self {
        Self {
            stream,
            rng,
            sender_ss,
            receiver_ss,
            read_buffer: Vec::with_capacity(_buffer_cap),
            chunk_reminder: 0,
            readable_end: 0,
            readable_start: 0,
            write_buffer: {
                let mut vec = Vec::with_capacity(_buffer_cap);
                vec.extend([0; TAG_SIZE]);
                vec
            },
            writable_end: 0,
            writable_start: 0,
        }
    }

    pub fn encrypt_in_progress(&mut self)
    where
        R: CryptoRngCore,
    {
        let in_progress = &mut self.write_buffer[self.writable_end..];
        let (tag_part, message) = in_progress.split_at_mut(TAG_SIZE);
        let tag = crypto::encrypt(message, self.sender_ss, &mut self.rng);
        tag_part[2..].copy_from_slice(&tag);
        let body_len = message.len() as u16;
        tag_part[..2].copy_from_slice(&body_len.to_be_bytes());
        self.writable_end = self.write_buffer.len();
        debug_assert!(self.write_buffer.capacity() - self.write_buffer.len() >= TAG_SIZE);
        self.write_buffer.extend([0; TAG_SIZE]);
    }
}

impl<R, T> AsyncWrite for Output<R, T>
where
    R: CryptoRngCore + Unpin,
    T: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        mut buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let s = self.deref_mut();

        let prev_len = buf.len();

        let mut updated = true;
        while std::mem::take(&mut updated) {
            'encrypt: {
                if s.write_buffer.len() - s.writable_end < s.write_buffer.capacity() / 2 {
                    break 'encrypt;
                }

                s.encrypt_in_progress();
                updated = true;
            }

            'write_inner: {
                let available = &s.write_buffer[s.writable_start..s.writable_end];
                if available.is_empty() {
                    break 'write_inner;
                }

                let Poll::Ready(n) = Pin::new(&mut s.stream).poll_write(cx, available)? else {
                    break 'write_inner;
                };

                if n == 0 {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }

                s.writable_start += n;
                updated = true;
            }

            'gc: {
                if s.write_buffer.len() < s.write_buffer.capacity() - TAG_SIZE {
                    break 'gc;
                }

                s.write_buffer.drain(..s.writable_start);
                s.writable_end -= s.writable_start;
                s.writable_start = 0;
                updated = true;
            }

            'write_buf: {
                let buffer = get_buffer(&mut s.write_buffer);
                let to_write = (buffer.len().saturating_sub(TAG_SIZE)).min(buf.len());
                if to_write == 0 {
                    break 'write_buf;
                }

                buffer[..to_write].copy_from_slice((&mut buf).take(..to_write).unwrap());
                extend_used_buffer(&mut s.write_buffer, to_write);
                updated = true;
            }
        }

        let red = prev_len - buf.len();
        if red == 0 {
            Poll::Pending
        } else {
            Poll::Ready(Ok(red))
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        let s = self.deref_mut();

        if s.write_buffer.len().saturating_sub(s.writable_end) != TAG_SIZE {
            s.encrypt_in_progress();
        }

        let mut to_write = &s.write_buffer[s.writable_start..s.writable_end];
        while !to_write.is_empty() {
            let n = futures::ready!(Pin::new(&mut s.stream).poll_write(cx, to_write))?;
            to_write = &to_write[n..];
            s.writable_start += n;
        }

        Pin::new(&mut s.stream).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        futures::ready!(self.as_mut().poll_flush(cx))?;
        Pin::new(&mut self.stream).poll_close(cx)
    }
}

impl<R, T> AsyncRead for Output<R, T>
where
    R: CryptoRngCore + Unpin,
    T: AsyncRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        mut buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let s = self.get_mut();

        let prev_len = buf.len();

        let mut updated = true;
        while std::mem::take(&mut updated) {
            'gc: {
                if s.read_buffer.len() != s.read_buffer.capacity() {
                    break 'gc;
                }

                s.read_buffer.drain(..s.readable_start);
                s.readable_end -= s.readable_start;
                s.readable_start = 0;
                updated = true;
            }

            'read_inner: loop {
                let buffer = get_buffer(&mut s.read_buffer);
                if buffer.is_empty() {
                    break 'read_inner;
                }
                let Poll::Ready(n) = Pin::new(&mut s.stream).poll_read(cx, buffer)? else {
                    break 'read_inner;
                };
                extend_used_buffer(&mut s.read_buffer, n);

                if n == 0 {
                    return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                }

                updated = true;
            }

            'decyrpt: loop {
                let available = &mut s.read_buffer[s.readable_end..];

                let Some((&mut len, rest)) = available.split_first_chunk_mut::<2>() else {
                    break 'decyrpt;
                };

                let len = u16::from_be_bytes(len) as usize + crypto::TAG_SIZE;
                let Some(body) = rest.get_mut(..len) else {
                    break 'decyrpt;
                };

                let Some((&mut tag, body)) = body.split_first_chunk_mut::<{ crypto::TAG_SIZE }>()
                else {
                    break 'decyrpt;
                };

                if !crypto::decrypt_separate_tag(body, s.receiver_ss, tag) {
                    return Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()));
                }
                s.readable_end += len + 2;
                updated = true;
            }

            'write_buf: loop {
                'advacne_chunk: {
                    if s.chunk_reminder != 0 || s.readable_start == s.readable_end {
                        break 'advacne_chunk;
                    }

                    let len = &s.read_buffer[s.readable_start..][..2];
                    let len = u16::from_be_bytes(len.try_into().unwrap()) as usize;
                    s.readable_start += TAG_SIZE;
                    s.chunk_reminder = len;
                    updated = true;
                }
                let read_len = s.chunk_reminder.min(buf.len());
                if read_len == 0 {
                    break 'write_buf;
                }

                let available = &s.read_buffer[s.readable_start..][..read_len];
                buf.take_mut(..read_len).unwrap().copy_from_slice(available);
                s.chunk_reminder -= read_len;
                s.readable_start += read_len;
                updated = true;
            }
        }

        let red = prev_len - buf.len();
        if red == 0 {
            Poll::Pending
        } else {
            Poll::Ready(Ok(red))
        }
    }
}

const TAG_SIZE: usize = crypto::TAG_SIZE + 2;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("signature error: {0}")]
    Signature(#[from] sign::SignatureError),
    #[error("decapsulation error: {0}")]
    Decapsulation(#[from] enc::DecapsulationError),
}

fn hash_to_peer_id(hash: crypto::Hash) -> PeerId {
    PeerId::from_multihash(Multihash::<64>::wrap(0, &hash).unwrap()).unwrap()
}

fn peer_id_to_hash(peer_id: PeerId) -> Option<crypto::Hash> {
    let multihash = Multihash::from(peer_id);
    let digest = multihash.digest();
    digest.try_into().ok()
}

fn get_buffer(buffer: &mut Vec<u8>) -> &mut [u8] {
    let cap = buffer.capacity();
    let len = buffer.len();
    let ptr = buffer.as_mut_ptr();
    unsafe { std::slice::from_raw_parts_mut(ptr.add(len), cap - len) }
}

fn extend_used_buffer(buffer: &mut Vec<u8>, n: usize) {
    let len = buffer.len();
    unsafe { buffer.set_len(len + n) }
}

pub trait ToPeerId {
    fn to_peer_id(&self) -> PeerId;
}

impl ToPeerId for sign::PublicKey {
    fn to_peer_id(&self) -> PeerId {
        hash_to_peer_id(self.identity())
    }
}

impl ToPeerId for sign::Keypair {
    fn to_peer_id(&self) -> PeerId {
        hash_to_peer_id(self.identity())
    }
}

impl ToPeerId for crypto::Hash {
    fn to_peer_id(&self) -> PeerId {
        hash_to_peer_id(*self)
    }
}

pub trait PeerIdExt {
    fn to_hash(&self) -> crypto::Hash;
    fn try_to_hash(&self) -> Option<crypto::Hash>;
}

impl PeerIdExt for PeerId {
    fn to_hash(&self) -> crypto::Hash {
        self.try_to_hash().unwrap_or_default()
    }

    fn try_to_hash(&self) -> Option<crypto::Hash> {
        peer_id_to_hash(*self)
    }
}

#[cfg(test)]
mod tests {
    use {
        futures::StreamExt,
        libp2p::{Multiaddr, Transport},
        libp2p_core::upgrade::Version,
        rand_core::OsRng,
        std::time::Duration,
    };

    fn create_swarm() -> (libp2p::Swarm<libp2p::ping::Behaviour>, crypto::sign::Keypair) {
        let kp = crypto::sign::Keypair::new(OsRng);
        let transport = libp2p::tcp::tokio::Transport::default()
            .upgrade(Version::V1)
            .authenticate(super::Config::new(OsRng, kp).with_buffer_size(300))
            .multiplex(libp2p::yamux::Config::default())
            .boxed();
        let behaviour = libp2p::ping::Behaviour::new(
            libp2p::ping::Config::new().with_interval(Duration::from_millis(1)),
        );

        (
            libp2p::Swarm::new(
                transport,
                behaviour,
                super::hash_to_peer_id(kp.identity()),
                libp2p::swarm::Config::with_tokio_executor()
                    .with_idle_connection_timeout(Duration::MAX),
            ),
            kp,
        )
    }

    #[tokio::test]
    async fn test() {
        _ = env_logger::builder().is_test(true).try_init();

        let (mut swarm1, _kp1) = create_swarm();
        let (mut swarm2, _kp2) = create_swarm();

        swarm1.listen_on("/ip4/0.0.0.0/tcp/7000".parse().unwrap()).unwrap();
        swarm2.dial("/ip4/127.0.0.1/tcp/7000".parse::<Multiaddr>().unwrap()).unwrap();

        _ = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    e = swarm1.next() => log::info!("event1: {:?}", e),
                    e = swarm2.next() => log::info!("event2: {:?}", e),
                }
            }
        })
        .await;
    }
}
