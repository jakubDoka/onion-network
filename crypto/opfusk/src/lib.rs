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
    std::{collections::VecDeque, io, ops::DerefMut, pin::Pin, task::Poll, usize},
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
                    sender_ss = temp_key.decapsulate(&init_resp.cp)?;
                    proof = self.kp.sign(&init_resp.challinge, &mut self.rng);
                    (cp, receiver_ss) = temp_key.encapsulate(&init_resp.enc_pk, &mut self.rng);
                    identity = init_resp.sign_pk.identity();
                }

                let final_req = unsafe { std::mem::transmute::<_, &mut FinalRequest>(&mut buf) };
                *final_req = FinalRequest { cp, proof };
            }

            Pin::new(&mut socket).write_all(&buf[..std::mem::size_of::<FinalRequest>()]).await?;

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
    buffer_cap: usize,
    read_buffer: Vec<u8>,
    readable_end: usize,
    readable_start: usize,
    chunk_reminder: usize,
    write_buffer: Vec<u8>,
    in_progress_start: usize,
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
            buffer_cap: _buffer_cap,
            read_buffer: Vec::with_capacity(_buffer_cap),
            chunk_reminder: 0,
            readable_end: 0,
            readable_start: 0,
            write_buffer: {
                let mut vec = Vec::with_capacity(_buffer_cap);
                vec.extend([0; TAG_SIZE]);
                vec
            },
            in_progress_start: TAG_SIZE,
            writable_start: TAG_SIZE,
        }
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

        'o: loop {
            let buffer = get_buffer(&mut s.write_buffer);
            // make sure next tag fits
            let n = buffer.len().min(buf.len());
            buffer[..n].copy_from_slice((&mut buf).take(..n).unwrap());
            s.writable_start += n;
            extend_used_buffer(&mut s.write_buffer, n);

            if s.write_buffer.len() - s.in_progress_start > s.write_buffer.capacity() / 2 {
                let (rest, slice) = s.write_buffer.split_at_mut(s.in_progress_start);
                let tag = crypto::encrypt(slice, s.sender_ss, &mut s.rng);
                let tag_start = rest.len() - TAG_SIZE;
                rest[tag_start..][..crypto::TAG_SIZE].copy_from_slice(&tag);
                let body_len = slice.len() as u16;
                slice[tag_start + crypto::TAG_SIZE..].copy_from_slice(&body_len.to_be_bytes());
                s.in_progress_start += slice.len() + TAG_SIZE;
            }

            let written = 'try_write_inner: {
                let to_write = &s.write_buffer[s.writable_start..s.in_progress_start - TAG_SIZE];
                if to_write.is_empty() {
                    break 'try_write_inner false;
                }

                let Poll::Ready(n) = Pin::new(&mut s.stream).poll_write(cx, to_write)? else {
                    break 'o;
                };
                s.writable_start += n;

                if n == 0 {
                    break 'o;
                }

                true
            };

            if s.in_progress_start >= s.write_buffer.capacity() && s.writable_start > TAG_SIZE * 2 {
                s.write_buffer.drain(..s.writable_start);
                s.in_progress_start -= s.writable_start;
                s.writable_start = 0;
            }

            if !buf.is_empty() && !written {
                break;
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
        todo!()
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

        'o: loop {
            while !buf.is_empty() {
                if s.chunk_reminder == 0 {
                    if !s.read_buffer.len() >= TAG_SIZE {
                        break;
                    }

                    let body_len = u16::from_be_bytes(
                        s.read_buffer[s.readable_end + crypto::TAG_SIZE..][..TAG_SIZE]
                            .try_into()
                            .unwrap(),
                    ) as usize;
                    s.chunk_reminder = body_len;
                    s.readable_start += TAG_SIZE;
                }

                if s.chunk_reminder == 0 {
                    // thats useless and suspicious
                    return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
                }

                let readable = &s.read_buffer[s.readable_start..s.readable_end];
                let to_copy = buf.len().min(s.chunk_reminder);
                buf.take_mut(..to_copy).unwrap().copy_from_slice(&readable[..to_copy]);
                s.readable_start += to_copy;
                s.chunk_reminder -= to_copy;
            }

            let buffer_is_full = s.read_buffer.len() == s.read_buffer.capacity();
            if buffer_is_full {
                if s.readable_start < TAG_SIZE * 2 {
                    // not worth it garbage collect
                    break 'o;
                }

                s.read_buffer.drain(..s.readable_start);
                s.readable_end -= s.readable_start;
                s.readable_start = 0;
            }

            let prev_read_end = s.readable_end;

            let has_tag = s.read_buffer.len() >= TAG_SIZE;
            'try_read_inner_prefix: {
                if has_tag {
                    break 'try_read_inner_prefix;
                }

                let buffer = get_buffer(&mut s.read_buffer);
                let Poll::Ready(n) = Pin::new(&mut s.stream).poll_read(cx, buffer)? else {
                    break 'try_read_inner_prefix;
                };

                if n == 0 {
                    break 'o;
                }

                extend_used_buffer(&mut s.read_buffer, n);
            }

            let mut has_body = false;
            'try_read_inner_body: {
                if !has_tag {
                    break 'try_read_inner_body;
                }

                let body_len = u16::from_be_bytes(
                    s.read_buffer[s.readable_end + crypto::TAG_SIZE..][..TAG_SIZE]
                        .try_into()
                        .unwrap(),
                ) as usize;

                if body_len > s.read_buffer.capacity() {
                    return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
                }

                has_body = s.read_buffer.len() - s.readable_end >= TAG_SIZE + body_len;
                if has_body {
                    break 'try_read_inner_body;
                }

                let buffer = get_buffer(&mut s.read_buffer);
                let Poll::Ready(n) = Pin::new(&mut s.stream).poll_read(cx, buffer)? else {
                    break 'try_read_inner_body;
                };

                if n == 0 {
                    break 'o;
                }

                extend_used_buffer(&mut s.read_buffer, n);
            }

            if has_body {
                let tag: [u8; crypto::TAG_SIZE] =
                    s.read_buffer[..crypto::TAG_SIZE].try_into().unwrap();
                let body = &mut s.read_buffer[TAG_SIZE..];
                if !crypto::decrypt_separate_tag(body, s.receiver_ss, tag) {
                    return Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()));
                }
                s.readable_end = TAG_SIZE + body.len();
            }

            if prev_read_end == s.readable_end || buf.is_empty() {
                break;
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

fn extend_used_buffer(buffer: &mut Vec<u8>, n: usize) {
    let len = buffer.len();
    unsafe { buffer.set_len(len + n) }
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

pub fn hash_to_peer_id(hash: crypto::Hash) -> PeerId {
    PeerId::from_multihash(Multihash::<64>::wrap(0, &hash).unwrap()).unwrap()
}

pub fn peer_id_to_hash(peer_id: PeerId) -> Option<crypto::Hash> {
    let multihash = Multihash::from(peer_id);
    let digest = multihash.digest();
    if digest.len() != 32 {
        return None;
    }
    let mut hash = [0; 32];
    hash.copy_from_slice(digest);
    Some(hash)
}

fn get_buffer(buffer: &mut Vec<u8>) -> &mut [u8] {
    let cap = buffer.capacity();
    let len = buffer.len();
    let ptr = buffer.as_mut_ptr();
    unsafe { std::slice::from_raw_parts_mut(ptr.add(len), cap) }
}

#[cfg(test)]
mod tests {
    use {
        futures::StreamExt,
        libp2p::{Multiaddr, Transport},
        libp2p_core::{transport::MemoryTransport, upgrade::Version},
        rand_core::OsRng,
        std::time::Duration,
    };

    fn create_swarm() -> (libp2p::Swarm<libp2p::ping::Behaviour>, crypto::sign::Keypair) {
        let kp = crypto::sign::Keypair::new(OsRng);
        let transport = MemoryTransport::default()
            .upgrade(Version::V1)
            .authenticate(super::Config::new(OsRng, kp))
            .multiplex(libp2p::yamux::Config::default())
            .boxed();
        let behaviour = libp2p::ping::Behaviour::default();
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

        swarm1.listen_on("/memory/1".parse().unwrap()).unwrap();
        swarm2.dial("/memory/1".parse::<Multiaddr>().unwrap()).unwrap();

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
