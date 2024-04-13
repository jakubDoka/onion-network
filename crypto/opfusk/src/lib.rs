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
    std::{collections::VecDeque, io, ops::DerefMut, pin::Pin, task::Poll},
};

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
        std::iter::once(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
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

            Ok((peer_id, Output {
                stream: socket,
                rng: self.rng,
                sender_ss,
                receiver_ss,
                sender_cursor: Cursor::new(self.buffer_size),
                reciever_cursor: Cursor::new(self.buffer_size),
            }))
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

            Ok((peer_id, Output {
                stream: socket,
                rng: self.rng,
                sender_ss,
                receiver_ss,
                sender_cursor: Cursor::new(self.buffer_size),
                reciever_cursor: Cursor::new(self.buffer_size),
            }))
        }
    }
}

pub struct Output<R, T> {
    stream: T,
    rng: R,
    sender_ss: SharedSecret,
    receiver_ss: SharedSecret,
    sender_cursor: Cursor,
    reciever_cursor: Cursor,
}

impl<R, T> AsyncRead for Output<R, T>
where
    R: CryptoRngCore + Unpin,
    T: AsyncWrite + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        todo!()
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
        let prev_len = buf.len();
        let s = self.deref_mut();

        let res = s.sender_cursor.write_to(cx, Pin::new(&mut s.stream));
        // never pending or error
        _ = s.sender_cursor.read_from(&s.sender_ss, &mut s.rng, cx, Pin::new(&mut buf));
        futures::ready!(res)?;

        if prev_len == buf.len() {
            return Poll::Pending;
        }

        Poll::Ready(Ok(prev_len - buf.len()))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        let s = self.deref_mut();
        if s.sender_cursor.can_flush() {
            s.sender_cursor.flush(&s.sender_ss, &mut s.rng);
        }
        futures::ready!(s.sender_cursor.write_to(cx, Pin::new(&mut s.stream)))?;
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

struct Cursor {
    buffer: Box<[u8]>,
    start: usize,
    end: usize,
    cursor: usize,
}

const TAG_SIZE: usize = crypto::TAG_SIZE + 2;

impl Cursor {
    fn new(cap: usize) -> Self {
        Self {
            buffer: vec![0; cap].into_boxed_slice(),
            start: 0,
            end: TAG_SIZE * 2,
            cursor: TAG_SIZE,
        }
    }

    fn write_to(
        &mut self,
        cx: &mut std::task::Context<'_>,
        stream: Pin<&mut impl AsyncWrite>,
    ) -> Poll<io::Result<()>> {
        let to_write = &self.buffer[self.start + TAG_SIZE..self.cursor];

        if !to_write.is_empty() {
            let n = futures::ready!(stream.poll_write(cx, to_write))?;
            self.start += n;

            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
        }

        if self.start + TAG_SIZE == self.cursor {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn read_from(
        &mut self,
        ss: &SharedSecret,
        rng: impl CryptoRngCore,
        cx: &mut std::task::Context<'_>,
        mut stream: Pin<&mut impl AsyncRead>,
    ) -> Poll<io::Result<()>> {
        let cap = self.buffer.len();
        let (read_a, write_b) = if self.end < self.start {
            (&mut self.buffer[self.end..self.start], &mut [][..])
        } else {
            let (left, right) = self.buffer.split_at_mut(self.end);
            (&mut left[..self.start], right)
        };

        let mut written = false;

        if !read_a.is_empty() {
            written = true;
            let n = futures::ready!(stream.as_mut().poll_read(cx, read_a))?;
            self.end += n;

            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
        }

        if !write_b.is_empty() && self.end == cap {
            written = true;
            let n = futures::ready!(stream.poll_read(cx, read_a))?;
            self.end = n;

            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
        }

        if self.end * (self.end == self.buffer.len()) as usize == self.start {
            self.flush(ss, rng)
        }

        if written {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn can_flush(&self) -> bool {
        self.cursor + TAG_SIZE != self.end
    }

    fn flush(&mut self, ss: &SharedSecret, rng: impl CryptoRngCore) {
        if self.start > self.end {
            let rotate_by = self.start;
            self.start = 0;
            self.cursor -= rotate_by;
            self.end += self.buffer.len() - rotate_by;
            self.buffer.rotate_left(rotate_by);
        }

        let tag =
            crypto::encrypt(&mut self.buffer[self.cursor + crypto::TAG_SIZE..self.end], *ss, rng);
        self.buffer[self.cursor..self.cursor].copy_from_slice(&tag);

        self.start = self.cursor - TAG_SIZE;
        self.cursor = wrap(self.end + TAG_SIZE, self.buffer.len());
        self.end = wrap(self.end + TAG_SIZE * 2, self.buffer.len());
    }
}

fn wrap(num: usize, around: usize) -> usize {
    if num > around {
        num - around
    } else {
        num
    }
}

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
