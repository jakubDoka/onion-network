use {
    crate::{
        packet::{self, ASOC_DATA, CONFIRM_PACKET_SIZE, MISSING_PEER},
        Keypair, PublicKey, SharedSecret,
    },
    aes_gcm::{
        aead::{generic_array::GenericArray, OsRng},
        AeadCore, AeadInPlace, Aes256Gcm, KeyInit,
    },
    codec::{Codec, Reminder},
    component_utils::{ClosingStream, FindAndRemove, PacketReader, PacketWriter},
    core::{fmt, slice},
    futures::{
        stream::{FusedStream, FuturesUnordered},
        AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, Future, FutureExt, StreamExt,
    },
    instant::Duration,
    libp2p::{
        identity::PeerId,
        swarm::{ConnectionId, NetworkBehaviour, StreamUpgradeError, ToSwarm as TS},
    },
    std::{
        collections::VecDeque, convert::Infallible, io, mem, ops::DerefMut, pin::Pin, sync::Arc,
        task::Poll,
    },
    thiserror::Error,
};

pub struct Behaviour {
    config: Config,
    router: FuturesUnordered<Channel>,
    error_streams: FuturesUnordered<component_utils::ClosingStream<libp2p::swarm::Stream>>,
    inbound: FuturesUnordered<IUpgrade>,
    outbound: FuturesUnordered<OUpgrade>,
    streams: streaming::Behaviour,
    events: VecDeque<TS<Event, ()>>,
    pending_connections: Vec<IncomingStream>,
    pending_requests: Vec<StreamRequest>,
    buffer: Arc<spin::Mutex<[u8; 1 << 16]>>,
}

impl Behaviour {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            router: Default::default(),
            streams: streaming::Behaviour::new(!config.dial),
            inbound: Default::default(),
            outbound: Default::default(),

            events: Default::default(),
            pending_connections: Default::default(),
            pending_requests: Default::default(),
            error_streams: Default::default(),
            buffer: Arc::new(spin::Mutex::new([0; 1 << 16])),

            config,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// !!! Path is in reverse order of relay jumps (last element denotes entry node, first element
    /// denotes destination) !!!
    /// # Panics
    ///
    /// Panics if path contains two equal elements in a row.
    pub fn open_path(
        &mut self,
        [mut path @ .., (mut recipient, to)]: [(PublicKey, PeerId); crate::packet::PATH_LEN + 1],
    ) -> PathId {
        assert!(path.array_windows().all(|[a, b]| a.1 != b.1));

        path.iter_mut().rev().for_each(|(k, _)| mem::swap(k, &mut recipient));

        let path_id = PathId::new();

        log::debug!("opening path to {}", to);

        self.push_stream_request(StreamRequest { to, path_id, recipient, path });

        path_id
    }

    fn push_stream_request(&mut self, sr: StreamRequest) {
        if sr.to == self.config.current_peer_id {
            // most likely melsrious since we check this in open_path
            log::warn!("melacious stream or self connection");
            return;
        }

        self.streams.create_stream(sr.to);
        self.pending_requests.push(sr);
    }

    /// Must be called when a peer cannot be found, otherwise a pending connection information is
    /// leaked for each `ConnectionRequest`.
    pub fn report_unreachable(&mut self, peer: PeerId) {
        for p in self.pending_connections.extract_if(|p| p.meta.to == peer) {
            self.error_streams.push(ClosingStream::new(p.stream, MISSING_PEER));
        }

        for r in self.pending_requests.extract_if(|p| p.to == peer) {
            self.events.push_back(TS::GenerateEvent(Event::OutboundStream(
                Err(StreamUpgradeError::Apply(OUpgradeError::MissingPeer)),
                r.path_id,
            )));
        }
    }

    pub fn poll_inbound(&mut self, cx: &mut std::task::Context<'_>) {
        while let Poll::Ready(Some((_, res))) = self.inbound.poll_next_unpin(cx) {
            let res = match res {
                Ok(r) => r,
                Err(e) => {
                    log::debug!("inbound error: {}", e);
                    continue;
                }
            };

            match res {
                IncomingOrResponse::Incoming(is) => {
                    log::debug!("creating new stream for route continuation");
                    self.streams.create_stream(is.meta.to);
                    self.pending_connections.push(is);
                    self.poll_streams(cx);
                }
                IncomingOrResponse::Response(stream) => {
                    self.events
                        .push_back(TS::GenerateEvent(Event::InboundStream(stream, PathId::new())));
                }
            }
        }
    }

    pub fn poll_outbound(&mut self, cx: &mut std::task::Context<'_>) {
        while let Poll::Ready(Some(res)) = self.outbound.poll_next_unpin(cx) {
            let ChannelMeta { from, to } = match res {
                Ok(r) => r,
                Err(e) => {
                    log::debug!("outbound error: {}", e);
                    continue;
                }
            };

            match from {
                ChannelSource::Relay(from) => 'b: {
                    let valid_stream_count = self
                        .router
                        .iter_mut()
                        .filter_map(|c| c.is_valid(self.config.keep_alive_interval).then_some(()))
                        .count();
                    if valid_stream_count + self.pending_connections.len() > self.config.max_streams
                    {
                        log::info!("too many streams");
                        if self.error_streams.len() > self.config.max_error_streams {
                            log::warn!("too many erroring streams");
                            break 'b;
                        }

                        self.error_streams.push(ClosingStream::new(from, packet::OCCUPIED_PEER));
                        break 'b;
                    }

                    log::debug!("received valid relay channel");
                    self.router.push(Channel::new(
                        from,
                        to,
                        self.config.buffer_cap,
                        self.buffer.clone(),
                    ));
                }
                ChannelSource::ThisNode(secret, path_id, peer_id) => {
                    self.events.push_back(TS::GenerateEvent(Event::OutboundStream(
                        Ok((EncryptedStream::new(to, secret, self.config.buffer_cap), peer_id)),
                        path_id,
                    )));
                }
            }
        }
    }

    fn poll_streams(&mut self, cx: &mut std::task::Context<'_>) {
        while let Poll::Ready(event) = self.streams.poll(cx) {
            let e = match event {
                TS::GenerateEvent(e) => e,
                e => {
                    self.events.push_back(e.map_out(|_| unreachable!()));
                    return;
                }
            };

            match e {
                streaming::Event::IncomingStream(peer, stream)
                    if let Some(key_pair) = self.config.secret.as_ref() =>
                {
                    log::debug!("received valid incoming stream");
                    self.inbound.push(upgrade_inbound(
                        peer,
                        key_pair.clone(),
                        self.config.buffer_cap,
                        stream,
                    ));
                    self.poll_inbound(cx);
                }
                streaming::Event::OutgoingStream(peer, Ok(stream)) => 'b: {
                    let key_pair =
                        self.config.secret.as_ref().cloned().unwrap_or_else(|| Keypair::new(OsRng));

                    if let Some(request) = self.pending_requests.find_and_remove(|r| r.to == peer) {
                        log::debug!("received valid request stream");
                        self.outbound.push(upgrade_outbound(
                            key_pair,
                            IncomingOrRequest::Request(request),
                            stream,
                        ));
                        self.poll_outbound(cx);
                        break 'b;
                    };

                    if let Some(incoming) =
                        self.pending_connections.find_and_remove(|r| r.meta.to == peer)
                    {
                        log::debug!("received valid incoming continuation stream");
                        self.outbound.push(upgrade_outbound(
                            key_pair,
                            IncomingOrRequest::Incoming(incoming),
                            stream,
                        ));
                        self.poll_outbound(cx);
                        break 'b;
                    }

                    log::warn!("received unexpected valid stream");
                }
                streaming::Event::OutgoingStream(peer, Err(err)) => 'b: {
                    let Some(request) = self.pending_requests.find_and_remove(|r| r.to == peer)
                    else {
                        log::warn!("received unexpected invalid stream");
                        break 'b;
                    };
                    self.events.push_back(TS::GenerateEvent(Event::OutboundStream(
                        Err(err.map_upgrade_err(|v| void::unreachable(v))),
                        request.path_id,
                    )));
                }
                streaming::Event::SearchRequest(peer) => {
                    self.events.push_back(TS::GenerateEvent(Event::SearchRequest(peer)));
                }
                _ => {}
            }
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = streaming::Handler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: libp2p::identity::PeerId,
        _local_addr: &libp2p::core::Multiaddr,
        _remote_addr: &libp2p::core::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(streaming::Handler::new(|| crate::ROUTING_PROTOCOL))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: libp2p::identity::PeerId,
        _addr: &libp2p::core::Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(streaming::Handler::new(|| crate::ROUTING_PROTOCOL))
    }

    fn on_swarm_event(&mut self, se: libp2p::swarm::FromSwarm) {
        self.streams.on_swarm_event(se);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: libp2p::identity::PeerId,
        connection_id: ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.streams.on_connection_handler_event(peer_id, connection_id, event);
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<TS<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        self.poll_inbound(cx);
        self.poll_outbound(cx);
        self.poll_streams(cx);

        if let Poll::Ready(Some(Err(e))) = self.router.poll_next_unpin(cx) {
            log::debug!("router error: {}", e);
        }

        if let Poll::Ready(Some(Err(e))) = self.error_streams.poll_next_unpin(cx) {
            log::debug!("error stream error: {}", e);
        }

        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}

component_utils::gen_config! {
    /// The secret key used as identiry for the node. In case you pass None, client mode is
    /// enabled and new secret created for each connection.
    secret: Option<Keypair>,
    /// The peer id of the node.
    current_peer_id: PeerId,
    ;;
    /// The maximum number of streams that can be opened at the same time.
    max_streams: usize = 20,
    /// The maximum number of streams to which the error response is sent.
    max_error_streams: usize = 20,
    /// The maximum interval between two packets before the connection is considered dead.
    keep_alive_interval: std::time::Duration = std::time::Duration::from_secs(30),
    /// size of the buffer for forwarding packets.
    buffer_cap: usize = 1 << 13,
    /// Dial instead of emmiting a connection request.
    dial: bool = true,
}

#[derive(Debug)]
pub enum Event {
    SearchRequest(PeerId),
    InboundStream(EncryptedStream, PathId),
    OutboundStream(Result<(EncryptedStream, PeerId), StreamUpgradeError<OUpgradeError>>, PathId),
}

component_utils::gen_unique_id!(pub PathId);

#[derive(Debug)]
pub struct EncryptedStream {
    inner: Option<libp2p::Stream>,
    key: SharedSecret,
    reader: PacketReader,
    writer: PacketWriter,
}

impl EncryptedStream {
    pub(crate) fn new(inner: libp2p::Stream, key: SharedSecret, cap: usize) -> Self {
        Self { inner: Some(inner), key, reader: Default::default(), writer: PacketWriter::new(cap) }
    }

    #[must_use = "write could have failed"]
    pub fn write_bytes(&mut self, data: &[u8]) -> Option<()> {
        self.write(Reminder(data))
    }

    #[must_use = "write could have failed"]
    pub fn write<'a>(&mut self, data: impl Codec<'a>) -> Option<()> {
        let aes = Aes256Gcm::new(GenericArray::from_slice(&self.key));
        let nonce = Aes256Gcm::generate_nonce(OsRng);

        let mut writer = self.writer.guard();
        let reserved = writer.write([0u8; 2])?;
        let raw = writer.write(data)?;
        let tag = aes.encrypt_in_place_detached(&nonce, ASOC_DATA, raw).expect("no");
        let full_len = raw.len() + tag.len() + nonce.len();
        writer.write_bytes(&tag)?;
        writer.write_bytes(&nonce)?;
        reserved.copy_from_slice(&(full_len as u16).to_be_bytes());

        Some(())
    }

    pub fn poll(&mut self, cx: &mut std::task::Context<'_>) -> Poll<io::Result<&mut [u8]>> {
        let Some(stream) = self.inner.as_mut() else {
            return Poll::Pending;
        };

        if let Poll::Ready(Err(e)) = self.writer.poll(cx, stream) {
            self.inner.take();
            return Poll::Ready(Err(e));
        }

        let read = match futures::ready!(self.reader.poll_packet(cx, stream)) {
            Ok(r) => r,
            Err(err) => {
                self.inner.take();
                return Poll::Ready(Err(err));
            }
        };

        let Some(len) = packet::peel_wih_key(&self.key, read) else {
            self.inner.take();
            return Poll::Ready(Err(io::ErrorKind::InvalidData.into()));
        };

        Poll::Ready(Ok(&mut read[..len]))
    }

    pub fn close(&mut self) {
        _ = self.inner.take();
    }
}

impl futures::Stream for EncryptedStream {
    type Item = io::Result<Vec<u8>>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if self.is_terminated() {
            return Poll::Ready(None);
        }
        self.poll(cx).map_ok(|v| v.to_vec()).map(Some)
    }
}

impl futures::stream::FusedStream for EncryptedStream {
    fn is_terminated(&self) -> bool {
        self.inner.is_none()
    }
}

#[derive(Debug)]
pub struct Stream {
    pub(crate) inner: libp2p::swarm::Stream,
    pub(crate) poll_cache: Vec<u8>,
    pub(crate) written: usize,
}

impl Stream {
    pub(crate) fn new(inner: libp2p::swarm::Stream, cap: usize) -> Self {
        Self { inner, poll_cache: Vec::with_capacity(cap), written: 0 }
    }

    fn forward_from(
        &mut self,
        from: &mut libp2p::swarm::Stream,
        temp: &mut [u8],
        last_packet: &mut instant::Instant,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<Infallible, io::Error>> {
        loop {
            while self.written < self.poll_cache.len() {
                let w = futures::ready!(
                    Pin::new(&mut self.inner).poll_write(cx, &self.poll_cache[self.written..])
                )?;
                if w == 0 {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                self.written += w;
            }
            self.poll_cache.clear();

            loop {
                let n = futures::ready!(Pin::new(&mut *from).poll_read(cx, temp))?;
                if n == 0 {
                    return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                }
                *last_packet = instant::Instant::now();
                let w = match Pin::new(&mut self.inner).poll_write(cx, &temp[..n])? {
                    Poll::Ready(w) => w,
                    Poll::Pending => 0,
                };
                if w == 0 {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                if w != n {
                    self.poll_cache.extend_from_slice(&temp[w..n]);
                    break;
                }
            }
        }
    }
}

pub struct Channel {
    from: Stream,
    to: Stream,
    waker: Option<std::task::Waker>,
    invalid: bool,
    buffer: Arc<spin::Mutex<[u8; 1 << 16]>>,
    last_packet: instant::Instant,
}

impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel").finish()
    }
}

impl Channel {
    pub fn new(
        from: libp2p::Stream,
        to: libp2p::Stream,
        buffer_cap: usize,
        buffer: Arc<spin::Mutex<[u8; 1 << 16]>>,
    ) -> Self {
        Self {
            from: Stream::new(from, buffer_cap),
            to: Stream::new(to, buffer_cap),
            waker: None,
            invalid: false,
            buffer,
            last_packet: instant::Instant::now(),
        }
    }

    pub fn poll(&mut self, cx: &mut std::task::Context<'_>) -> Poll<io::Result<Infallible>> {
        if self.invalid {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }
        component_utils::set_waker(&mut self.waker, cx.waker());
        let temp = &mut self.buffer.lock()[..];
        if let Poll::Ready(e) =
            self.from.forward_from(&mut self.to.inner, temp, &mut self.last_packet, cx)
        {
            return Poll::Ready(e);
        }
        self.to.forward_from(&mut self.from.inner, temp, &mut self.last_packet, cx)
    }

    fn is_valid(&mut self, timeout: Duration) -> bool {
        if self.invalid {
            return false;
        }

        if self.last_packet + timeout > instant::Instant::now() {
            return true;
        }

        self.invalid = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }

        false
    }
}

impl std::future::Future for Channel {
    type Output = io::Result<Infallible>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.deref_mut().poll(cx)
    }
}

type OUpgrade = impl Future<Output = Result<ChannelMeta, OUpgradeError>>;

fn upgrade_outbound(
    keypair: Keypair,
    incoming: IncomingOrRequest,
    stream: libp2p::swarm::Stream,
) -> OUpgrade {
    upgrade_outbound_low(keypair, incoming, stream)
}

async fn upgrade_outbound_low(
    keypair: Keypair,
    incoming: IncomingOrRequest,
    mut stream: libp2p::swarm::Stream,
) -> Result<ChannelMeta, OUpgradeError> {
    let mut written_packet = vec![];
    let mut ss = [0; 32];
    let (buffer, peer_id) = match &incoming {
        IncomingOrRequest::Request(r) => {
            ss = packet::new_initial(&r.recipient, r.path, &keypair, &mut written_packet);
            (&written_packet, r.path[0].1)
        }
        IncomingOrRequest::Incoming(i) => (&i.meta.buffer, i.meta.to), // the peer id is arbitrary in
                                                                       // this case
    };

    stream
        .write_all(&(buffer.len() as u16).to_be_bytes())
        .await
        .map_err(OUpgradeError::WritePacketLength)?;
    log::debug!("wrote packet length: {}", buffer.len());
    stream.write_all(buffer).await.map_err(OUpgradeError::WritePacket)?;
    log::debug!("wrote packet");

    let request = match incoming {
        IncomingOrRequest::Incoming(i) => {
            log::debug!("received incoming routable stream");
            return Ok(ChannelMeta { from: ChannelSource::Relay(i.stream), to: stream });
        }
        IncomingOrRequest::Request(r) => r,
    };

    let mut kind = 0;
    stream.read_exact(slice::from_mut(&mut kind)).await.map_err(OUpgradeError::ReadPacketKind)?;
    log::debug!("read packet kind: {}", kind);

    match kind {
        crate::packet::OK => {
            log::debug!("received auth packet");
            let mut buffer = written_packet;
            buffer.resize(CONFIRM_PACKET_SIZE, 0);
            stream.read(&mut buffer).await.map_err(OUpgradeError::ReadPacket)?;

            if !packet::verify_confirm(&ss, &mut buffer) {
                Err(OUpgradeError::AuthenticationFailed)
            } else {
                Ok(ChannelMeta {
                    from: ChannelSource::ThisNode(ss, request.path_id, peer_id),
                    to: stream,
                })
            }
        }
        crate::packet::MISSING_PEER => Err(OUpgradeError::MissingPeer),
        crate::packet::OCCUPIED_PEER => Err(OUpgradeError::OccupiedPeer),
        _ => Err(OUpgradeError::UnknownPacketKind(kind)),
    }
}

type IUpgrade = impl Future<Output = (PeerId, Result<IncomingOrResponse, IUpgradeError>)>;

fn upgrade_inbound(
    peer: PeerId,
    keypair: Keypair,
    buffer_cap: usize,
    stream: libp2p::swarm::Stream,
) -> IUpgrade {
    upgrade_inbound_low(keypair, buffer_cap, stream).map(move |r| (peer, r))
}

async fn upgrade_inbound_low(
    keypair: Keypair,
    buffer_cap: usize,
    mut stream: libp2p::swarm::Stream,
) -> Result<IncomingOrResponse, IUpgradeError> {
    log::debug!("received inbound stream");
    let mut len = [0; 2];
    stream.read_exact(&mut len).await.map_err(IUpgradeError::ReadPacketLength)?;

    let len = u16::from_be_bytes(len) as usize;
    let mut buffer = vec![0; len];

    stream.read_exact(&mut buffer).await.map_err(IUpgradeError::ReadPacket)?;

    log::debug!("peeling packet: {}", len);
    let (to, ss, new_len) =
        crate::packet::peel_initial(&keypair, &mut buffer).ok_or(IUpgradeError::MalformedPacket)?;

    log::debug!("peeled packet to: {:?}", to);

    log::debug!("received init packet");
    let Some(to) = to else {
        log::debug!("received incoming stream");
        buffer.resize(CONFIRM_PACKET_SIZE + 1, 0);
        packet::write_confirm(&ss, &mut buffer[1..]);
        buffer[0] = packet::OK;
        stream.write_all(&buffer).await.map_err(IUpgradeError::WriteAuthPacket)?;

        return Ok(IncomingOrResponse::Response(EncryptedStream::new(stream, ss, buffer_cap)));
    };

    Ok(IncomingOrResponse::Incoming(IncomingStream {
        stream,
        meta: IncomingStreamMeta { to, buffer: buffer[..new_len].to_vec() },
    }))
}

component_utils::decl_stream_protocol!(ROUTING_PROTOCOL = "rot");

#[derive(Debug)]
pub struct IncomingStream {
    stream: libp2p::Stream,
    meta: IncomingStreamMeta,
}

#[derive(Debug, Clone)]
pub struct IncomingStreamMeta {
    to: PeerId,
    buffer: Vec<u8>,
}

#[derive(Debug)]
pub struct StreamRequest {
    to: PeerId,
    path_id: PathId,
    recipient: PublicKey,
    path: [(PublicKey, PeerId); crate::packet::PATH_LEN],
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum IncomingOrRequest {
    Incoming(IncomingStream),
    Request(StreamRequest),
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum IncomingOrResponse {
    Incoming(IncomingStream),
    Response(EncryptedStream),
}

#[derive(Debug, Error)]
pub enum IUpgradeError {
    #[error("malformed init packet")]
    MalformedPacket,
    #[error("failed to write packet: {0}")]
    WriteKeyPacket(io::Error),
    #[error("failed to read packet length: {0}")]
    ReadPacketLength(io::Error),
    #[error("failed to read packet: {0}")]
    ReadPacket(io::Error),
    #[error("failed to write auth packet: {0}")]
    WriteAuthPacket(io::Error),
}

#[derive(Debug)]
pub enum ChannelSource {
    Relay(libp2p::Stream),
    ThisNode(SharedSecret, PathId, PeerId),
}

#[derive(Debug)]
pub struct ChannelMeta {
    from: ChannelSource,
    to: libp2p::Stream,
}

#[derive(Debug, Error)]
pub enum OUpgradeError {
    #[error("missing peer")]
    MissingPeer,
    #[error("occupied peer")]
    OccupiedPeer,
    #[error("malformed init packet")]
    MalformedPacket,
    #[error("failed to authenticate")]
    AuthenticationFailed,
    #[error("paket kind not recognized: {0}")]
    UnknownPacketKind(u8),
    #[error("failed to write packet length: {0}")]
    WritePacketLength(io::Error),
    #[error("failed to write packet: {0}")]
    WritePacket(io::Error),
    #[error("failed to read packet kind: {0}")]
    ReadPacketKind(io::Error),
    #[error("failed to read packet: {0}")]
    ReadPacket(io::Error),
}
