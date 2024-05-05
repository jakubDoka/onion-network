use {
    crate::{
        packet::{self, OuterPacket, MISSING_PEER},
        Keypair, PublicKey, SharedSecret,
    },
    aes_gcm::aead::OsRng,
    codec::Encode,
    component_utils::FindAndRemove,
    core::slice,
    futures::{
        stream::FuturesUnordered, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, Future,
        FutureExt, StreamExt,
    },
    instant::Duration,
    libp2p::{
        identity::PeerId,
        swarm::{ConnectionId, NetworkBehaviour, StreamUpgradeError, ToSwarm as TS},
    },
    std::{collections::VecDeque, io, pin::Pin, task::Poll},
    thiserror::Error,
};

type ClosingStream = impl Future<Output = io::Result<()>>;

struct Route {
    last_packet: instant::Instant,
    forward: Channel,
    backward: Channel,
    from: libp2p::Stream,
    to: libp2p::Stream,
    close_waker: Option<std::task::Waker>,
    closed: bool,
}

impl Route {
    fn new(from: libp2p::Stream, to: libp2p::Stream) -> Self {
        Self {
            forward: Channel::new(),
            backward: Channel::new(),
            from,
            to,
            last_packet: instant::Instant::now(),
            close_waker: None,
            closed: false,
        }
    }

    fn try_close(&mut self, interval: Duration) -> bool {
        if self.last_packet.elapsed() > interval {
            if let Some(waker) = self.close_waker.take() {
                waker.wake();
            }
            self.closed = true;
            false
        } else {
            true
        }
    }
}

impl Future for Route {
    type Output = io::Result<!>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        if self.closed {
            return Poll::Ready(Err(io::ErrorKind::ConnectionAborted.into()));
        }

        let s = self.get_mut();
        s.close_waker = Some(cx.waker().clone());
        _ = s.forward.poll(cx, &mut s.from, &mut s.to)?;
        _ = s.backward.poll(cx, &mut s.to, &mut s.from)?;
        s.last_packet = instant::Instant::now();
        Poll::Pending
    }
}

struct Channel {
    buf: [u8; 1 << 14],
    readable_start: usize,
    written_end: usize,
}

impl Channel {
    fn new() -> Channel {
        Channel { buf: [0; 1 << 14], readable_start: 0, written_end: 0 }
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
        from: &mut libp2p::Stream,
        to: &mut libp2p::Stream,
    ) -> Poll<io::Result<!>> {
        let mut updated = true;
        while std::mem::take(&mut updated) {
            if self.written_end == self.buf.len() && self.readable_start != 0 {
                self.buf.copy_within(self.readable_start..self.written_end, 0);
                self.written_end -= self.readable_start;
                self.readable_start = 0;
            }

            let read_buf = &mut self.buf[self.written_end..];
            if !read_buf.is_empty() {
                match Pin::new(&mut *from).poll_read(cx, read_buf)? {
                    Poll::Ready(0) => {
                        if self.readable_start == self.written_end {
                            std::task::ready!(Pin::new(&mut *to).poll_flush(cx))?;
                            std::task::ready!(Pin::new(&mut *to).poll_close(cx))?;
                            return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                        }
                    }
                    Poll::Ready(n) => {
                        self.written_end += n;
                        updated = true;
                    }
                    Poll::Pending => {}
                }
            }

            let write_buf = &self.buf[self.readable_start..self.written_end];
            if !write_buf.is_empty() {
                match Pin::new(&mut *to).poll_write(cx, write_buf)? {
                    Poll::Ready(0) => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
                    Poll::Ready(n) => {
                        self.readable_start += n;
                        updated = true;
                    }
                    Poll::Pending => {}
                }
            }
        }

        Poll::Pending
    }
}

fn close_stream(mut stream: libp2p::Stream, code: u8) -> ClosingStream {
    async move { stream.write_all(&[code]).await }
}

pub struct Behaviour {
    config: Config,
    router: FuturesUnordered<Route>,
    error_streams: FuturesUnordered<ClosingStream>,
    inbound: FuturesUnordered<IUpgrade>,
    outbound: FuturesUnordered<OUpgrade>,
    streaming: streaming::Behaviour,
    events: VecDeque<TS<Event, usize>>,
    pending_connections: Vec<IncomingStream>,
    pending_requests: Vec<StreamRequest>,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl Behaviour {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            router: Default::default(),
            streaming: streaming::Behaviour::new(|| ROUTING_PROTOCOL),
            inbound: Default::default(),
            outbound: Default::default(),

            events: Default::default(),
            pending_connections: Default::default(),
            pending_requests: Default::default(),
            error_streams: Default::default(),

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
        [path @ .., (.., to)]: [(PublicKey, PeerId); crate::packet::PATH_LEN + 1],
    ) -> PathId {
        assert!(path.array_windows().all(|[a, b]| a.1 != b.1));

        let path_id = PathId::new();

        log::debug!("opening path to {}", to);

        self.push_stream_request(StreamRequest {
            to,
            path_id,
            recipient: path[0].0,
            recipient_id: path[0].1,
            mid_recipient: path[1].0,
            mid_id: path[1].1,
        });

        path_id
    }

    fn push_stream_request(&mut self, sr: StreamRequest) {
        if sr.to == self.config.current_peer_id {
            // most likely melsrious since we check this in open_path
            log::warn!("melacious stream or self connection");
            return;
        }

        self.streaming.create_stream(sr.to);
        self.pending_requests.push(sr);
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
                    self.streaming.create_stream(is.meta.to);
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
        while let Poll::Ready(Some(ChannelMeta { from, to })) = self.outbound.poll_next_unpin(cx) {
            match from {
                ChannelSource::Relay(Ok(from)) => 'b: {
                    let valid_stream_count = self
                        .router
                        .iter_mut()
                        .filter_map(|c| c.try_close(self.config.keep_alive_interval).then_some(()))
                        .count();
                    if valid_stream_count + self.pending_connections.len() > self.config.max_streams
                    {
                        log::debug!("too many streams");
                        if self.error_streams.len() > self.config.max_error_streams {
                            log::warn!("too many erroring streams");
                            break 'b;
                        }

                        self.error_streams.push(close_stream(from, packet::OCCUPIED_PEER));
                        break 'b;
                    }

                    log::debug!("received valid relay channel");
                    self.router.push(Route::new(from, to));
                }
                ChannelSource::Relay(Err(e)) => {
                    log::warn!("received invalid relay channel: {}", e);
                }
                ChannelSource::ThisNode(secret, path_id, peer_id) => {
                    self.events.push_back(TS::GenerateEvent(Event::OutboundStream(
                        secret
                            .map(|s| EncryptedStream::new(to, OsRng, s, self.config.buffer_cap))
                            .map_err(StreamUpgradeError::Apply),
                        peer_id,
                        path_id,
                    )));
                }
            }
        }
    }

    fn poll_streams(&mut self, cx: &mut std::task::Context<'_>) {
        while let Poll::Ready(event) = self.streaming.poll(cx) {
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
                        if let Some(conn) =
                            self.pending_connections.find_and_remove(|r| r.meta.to == peer)
                        {
                            self.error_streams.push(close_stream(conn.stream, MISSING_PEER));
                            break 'b;
                        }

                        log::warn!("received unexpected invalid stream: {err}");
                        break 'b;
                    };

                    self.events.push_back(TS::GenerateEvent(Event::OutboundStream(
                        Err(err.map_upgrade_err(|v| void::unreachable(v))),
                        peer,
                        request.path_id,
                    )));
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
        cid: ConnectionId,
        peer_id: PeerId,
        _: &libp2p::Multiaddr,
        _: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        self.streaming.new_handler(peer_id, cid)
    }

    fn handle_established_outbound_connection(
        &mut self,
        cid: ConnectionId,
        peer_id: PeerId,
        _: &libp2p::Multiaddr,
        _: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        self.streaming.new_handler(peer_id, cid)
    }

    fn on_swarm_event(&mut self, se: libp2p::swarm::FromSwarm) {
        self.streaming.on_swarm_event(se);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: libp2p::identity::PeerId,
        connection_id: ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.streaming.on_connection_handler_event(peer_id, connection_id, event);
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
    buffer_cap: usize = 1 << 16,
}

impl Default for Config {
    fn default() -> Self {
        Self::new(None, PeerId::random())
    }
}

impl Config {
    pub fn build(self) -> Behaviour {
        Behaviour::new(self)
    }
}

#[derive(Debug)]
pub enum Event {
    InboundStream(EncryptedStream, PathId),
    OutboundStream(Result<EncryptedStream, StreamUpgradeError<OUpgradeError>>, PeerId, PathId),
}

component_utils::gen_unique_id!(pub PathId);

pub type EncryptedStream = opfusk::Output<OsRng, libp2p::Stream>;

type OUpgrade = impl Future<Output = ChannelMeta>;

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
) -> ChannelMeta {
    let mut written_packet = vec![];
    let mut ss = [0; 32];
    let (buffer, peer_id) = match &incoming {
        IncomingOrRequest::Request(r) => {
            let (pck, new_ss) =
                OuterPacket::new(r.recipient, r.recipient_id, r.mid_recipient, r.mid_id, &keypair);
            ss = new_ss;
            pck.encode(&mut written_packet).unwrap();
            (&written_packet, r.recipient_id)
        }
        IncomingOrRequest::Incoming(i) => (&i.meta.buffer, i.meta.to), // the peer id is arbitrary in
                                                                       // this case
    };

    let write = async {
        stream
            .write_all(&(buffer.len() as u16).to_be_bytes())
            .await
            .map_err(OUpgradeError::WritePacketLength)?;
        log::debug!("wrote packet length: {}", buffer.len());
        stream.write_all(buffer).await.map_err(OUpgradeError::WritePacket)?;
        log::debug!("wrote packet");
        Ok(())
    }
    .await;

    let request = match incoming {
        IncomingOrRequest::Incoming(i) => {
            let s = write.map(|_| i.stream);
            log::debug!("received incoming routable stream");
            return ChannelMeta { from: ChannelSource::Relay(s), to: stream };
        }
        IncomingOrRequest::Request(r) => r,
    };

    let read = async {
        write?;
        let mut kind = 0;
        stream
            .read_exact(slice::from_mut(&mut kind))
            .await
            .map_err(OUpgradeError::ReadPacketKind)?;
        log::debug!("read packet kind: {}", kind);

        match kind {
            crate::packet::OK => {
                log::debug!("received auth packet");
                let mut buffer = written_packet;
                buffer.resize(crypto::TAG_SIZE, 0);
                stream.read(&mut buffer).await.map_err(OUpgradeError::ReadPacket)?;

                if crypto::decrypt(&mut buffer, ss).is_none() {
                    Err(OUpgradeError::AuthenticationFailed)
                } else {
                    Ok(ss)
                }
            }
            crate::packet::MISSING_PEER => Err(OUpgradeError::MissingPeer),
            crate::packet::OCCUPIED_PEER => Err(OUpgradeError::OccupiedPeer),
            _ => Err(OUpgradeError::UnknownPacketKind(kind)),
        }
    }
    .await;

    ChannelMeta { from: ChannelSource::ThisNode(read, request.path_id, peer_id), to: stream }
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
    let (res, new_len) =
        crate::packet::peel(&keypair, &mut buffer).ok_or(IUpgradeError::MalformedPacket)?;

    log::debug!("peeled packet to: {:?}", res);

    log::debug!("received init packet");
    match res {
        Ok(to) => Ok(IncomingOrResponse::Incoming(IncomingStream {
            stream,
            meta: IncomingStreamMeta { to, buffer: buffer[..new_len].to_vec() },
        })),
        Err(ss) => {
            log::debug!("received incoming stream");
            buffer.resize(crypto::TAG_SIZE + 1, 0);
            let tag = crypto::encrypt(&mut [], ss, OsRng);
            buffer[1..].copy_from_slice(&tag);
            buffer[0] = packet::OK;
            stream.write_all(&buffer).await.map_err(IUpgradeError::WriteAuthPacket)?;

            Ok(IncomingOrResponse::Response(EncryptedStream::new(stream, OsRng, ss, buffer_cap)))
        }
    }
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
    recipient_id: PeerId,
    mid_recipient: PublicKey,
    mid_id: PeerId,
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
enum ChannelSource {
    Relay(Result<libp2p::Stream, OUpgradeError>),
    ThisNode(Result<SharedSecret, OUpgradeError>, PathId, PeerId),
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
