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

            Ok((peer_id, Output {
                stream: socket,
                rng: self.rng,
                sender_ss,
                receiver_ss,
                sender_cursor: WriteCursor::new(self.buffer_size),
                reciever_cursor: ReadCursor::new(self.buffer_size),
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
                sender_cursor: WriteCursor::new(self.buffer_size),
                reciever_cursor: ReadCursor::new(self.buffer_size),
            }))
        }
    }
}

pub struct Output<R, T> {
    stream: T,
    rng: R,
    sender_ss: SharedSecret,
    receiver_ss: SharedSecret,
    sender_cursor: WriteCursor,
    reciever_cursor: ReadCursor,
}

impl<R, T> AsyncRead for Output<R, T>
where
    R: CryptoRngCore + Unpin,
    T: AsyncRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let s = self.get_mut();
        s.reciever_cursor.read_from(cx, &s.receiver_ss, buf, &mut s.stream)
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
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        log::debug!("writing {} bytes", buf.len());
        let s = self.deref_mut();
        s.sender_cursor.write_to(cx, &s.sender_ss, &mut s.rng, buf, Pin::new(&mut s.stream))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        log::debug!("flushing");
        let s = self.deref_mut();
        futures::ready!(s.sender_cursor.force_flush(cx, &s.sender_ss, &mut s.rng, &mut s.stream))?;
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

struct ReadCursor {
    buffer: Box<[u8]>,
    cursor: usize,
    start: usize,
    end: usize,
    current_chunk_len: usize,
}

impl ReadCursor {
    fn new(cap: usize) -> Self {
        Self {
            buffer: vec![0; cap].into_boxed_slice(),
            cursor: 0,
            start: 0,
            end: 0,
            current_chunk_len: 0,
        }
    }

    fn as_slices_mut(buffer: &mut [u8], start: usize, end: usize) -> (&mut [u8], &mut [u8]) {
        if start >= end {
            let (left, right) = buffer.split_at_mut(start);
            (right, &mut left[..end])
        } else {
            (&mut buffer[start..end], &mut [][..])
        }
    }

    fn remove_first_n(start: &mut usize, cap: usize, end: usize, n: usize) {
        *start = wrap(*start + n, cap);
    }

    fn add_last_n(end: &mut usize, cap: usize, start: usize, n: usize) {
        *end = wrap(*end + n, cap);
    }

    fn distance(start: usize, end: usize, cap: usize) -> usize {
        if start < end {
            end - start
        } else {
            cap - start + end
        }
    }

    fn read_from(
        &mut self,
        cx: &mut std::task::Context<'_>,
        ss: &SharedSecret,
        mut bytes: &mut [u8],
        stream: &mut (impl AsyncRead + Unpin),
    ) -> Poll<io::Result<usize>> {
        let cap = self.buffer.len();
        let (to_read_left, to_read_right) =
            Self::as_slices_mut(&mut self.buffer, self.end, self.start);

        'b: {
            if to_read_left.is_empty() {
                break 'b;
            }

            let Poll::Ready(n) = Pin::new(&mut *stream).poll_read(cx, to_read_left)? else {
                break 'b;
            };
            Self::add_last_n(&mut self.end, cap, self.start, n);

            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }

            if to_read_right.is_empty() || to_read_left.len() != n {
                break 'b;
            }

            let Poll::Ready(n) = Pin::new(stream).poll_read(cx, to_read_right)? else {
                break 'b;
            };
            Self::add_last_n(&mut self.end, cap, self.start, n);

            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
        }

        if let Some(chunk_len) = self.extract_chunk_len(self.cursor) {
            if Self::distance(self.cursor, self.end, cap) >= chunk_len + TAG_SIZE {
                if self.cursor > self.end {
                    let rotate_by = cap - self.cursor;
                    self.buffer.rotate_right(rotate_by);
                    self.start += rotate_by;
                    self.end += rotate_by;
                    self.cursor = 0;
                }

                let tag = &self.buffer[self.cursor..][..crypto::TAG_SIZE];
                let tag: [u8; crypto::TAG_SIZE] = tag.try_into().unwrap();
                let to_decrypt = &mut self.buffer[self.cursor + TAG_SIZE..][..chunk_len];
                _ = crypto::decrypt_separate_tag(to_decrypt, *ss, tag);

                self.cursor = wrap(self.cursor + chunk_len + TAG_SIZE, cap);
            }
        }

        let prev_len = bytes.len();

        while !bytes.is_empty() {
            if self.current_chunk_len == 0
                && let Some(chunk_len) = self.extract_chunk_len(self.start)
            {
                self.start += TAG_SIZE;
                self.current_chunk_len = chunk_len;
            }

            let end = wrap(self.start + self.current_chunk_len, cap);
            let (mut left, mut right) = Self::as_slices_mut(&mut self.buffer, self.start, end);
            if left.len() + right.len() == 0 {
                log::debug!("no more data to read");
                break;
            }

            fn take<'a>(bytes: &mut &'a mut [u8], n: usize) -> &'a mut [u8] {
                bytes.take_mut(..n).unwrap()
            }

            let common_len = bytes.len().min(left.len());
            take(&mut bytes, common_len).copy_from_slice(take(&mut left, common_len));
            let common_len = bytes.len().min(right.len());
            take(&mut bytes, common_len).copy_from_slice(take(&mut right, common_len));
        }

        let red = prev_len - bytes.len();

        if red == 0 {
            return Poll::Pending;
        }

        log::debug!("read {} bytes", red);

        Self::remove_first_n(&mut self.start, cap, self.end, red);
        Poll::Ready(Ok(red))
    }

    fn extract_chunk_len(&self, offset: usize) -> Option<usize> {
        let chunk_len = self.buffer.iter().cycle().take(self.end).skip(offset + crypto::TAG_SIZE);
        let chunk_len = chunk_len.copied().next_chunk::<2>().ok()?;
        Some(u16::from_be_bytes(chunk_len) as usize)
    }
}

struct WriteCursor {
    buffer: VecDeque<u8>,
    cursor: usize,
}

const TAG_SIZE: usize = crypto::TAG_SIZE + 2;

impl WriteCursor {
    fn new(cap: usize) -> Self {
        let mut buffer = VecDeque::with_capacity(cap);
        buffer.extend([0; TAG_SIZE * 2]);
        Self { buffer, cursor: TAG_SIZE }
    }

    fn write_to(
        &mut self,
        cx: &mut std::task::Context<'_>,
        ss: &SharedSecret,
        rng: impl CryptoRngCore,
        bytes: &[u8],
        stream: Pin<&mut impl AsyncWrite>,
    ) -> Poll<io::Result<usize>> {
        let to_write = &self.buffer.as_slices().0[..self.cursor - TAG_SIZE];

        if !to_write.is_empty() {
            let n = futures::ready!(stream.poll_write(cx, to_write))?;
            self.buffer.drain(..n);
            self.cursor -= n;

            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
        }

        let free_cap = self.buffer.capacity() - self.buffer.len();
        if free_cap == 0 {
            if self.can_flush() {
                self.flush(ss, rng);
            }
            return Poll::Pending;
        }

        let read_len = free_cap.min(bytes.len());
        self.buffer.extend(&bytes[..read_len]);

        Poll::Ready(Ok(read_len))
    }

    fn can_flush(&self) -> bool {
        self.cursor + TAG_SIZE < self.buffer.len()
    }

    fn force_flush(
        &mut self,
        cx: &mut std::task::Context<'_>,
        ss: &SharedSecret,
        rng: impl CryptoRngCore,
        stream: &mut (impl AsyncWrite + Unpin),
    ) -> Poll<io::Result<()>> {
        self.flush(ss, rng);
        let to_write = &self.buffer.as_slices().0[..self.cursor - TAG_SIZE];
        if to_write.is_empty() {
            return Poll::Ready(Ok(()));
        }

        log::debug!("force flushing: {} bytes", to_write.len());

        let n = futures::ready!(Pin::new(stream).poll_write(cx, to_write))?;
        self.buffer.drain(..n);
        self.cursor -= n;

        if n == 0 {
            return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
        }

        if self.cursor == TAG_SIZE {
            log::debug!("flushed");
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn flush(&mut self, ss: &SharedSecret, rng: impl CryptoRngCore) {
        if !self.can_flush() {
            log::debug!("cannot flush");
            return;
        }

        log::debug!("flushing");

        let (left, right) = self.buffer.as_mut_slices();
        let slice = if self.cursor > left.len() {
            &mut right[self.cursor - left.len()..]
        } else {
            &mut self.buffer.make_contiguous()[self.cursor..]
        };

        let len = slice.len() - TAG_SIZE;
        let tag = crypto::encrypt(&mut slice[crypto::TAG_SIZE..], *ss, rng);
        slice[..crypto::TAG_SIZE].copy_from_slice(&tag);
        slice[crypto::TAG_SIZE..TAG_SIZE].copy_from_slice(&(len as u16).to_be_bytes());

        self.cursor += slice.len();
    }
}

fn wrap(num: usize, around: usize) -> usize {
    if num >= around {
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
