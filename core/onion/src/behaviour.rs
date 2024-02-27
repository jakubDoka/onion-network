use {
    crate::{
        handler::{self, Handler},
        packet::{self, ASOC_DATA, MISSING_PEER},
        IncomingOrRequest, IncomingOrResponse, IncomingStream, KeyPair, OUpgradeError, PublicKey,
        SharedSecret, StreamRequest,
    },
    aes_gcm::{
        aead::{generic_array::GenericArray, OsRng},
        AeadCore, AeadInPlace, Aes256Gcm, KeyInit,
    },
    component_utils::{
        encode_len, ClosingStream, Codec, FindAndRemove, PacketReader, PacketWriter, Reminder,
        PACKET_LEN_WIDTH,
    },
    core::fmt,
    futures::{
        stream::{FusedStream, FuturesUnordered},
        AsyncRead, AsyncWrite, StreamExt,
    },
    instant::Duration,
    libp2p::{
        identity::PeerId,
        swarm::{
            dial_opts::DialOpts, ConnectionId, DialError, NetworkBehaviour, NotifyHandler,
            StreamUpgradeError, ToSwarm as TS,
        },
    },
    std::{
        collections::VecDeque, convert::Infallible, io, mem, ops::DerefMut, pin::Pin, sync::Arc,
        task::Poll,
    },
};

pub struct Behaviour {
    config: Config,
    router: FuturesUnordered<Channel>,
    peer_to_connection: component_utils::LinearMap<PeerId, ConnectionId>,
    dialing_peers: component_utils::LinearMap<ConnectionId, PeerId>,
    events: VecDeque<TS<Event, handler::FromBehaviour>>,
    pending_connections: Vec<IncomingStream>,
    pending_requests: Vec<Arc<StreamRequest>>,
    error_streams: FuturesUnordered<component_utils::ClosingStream<libp2p::swarm::Stream>>,
    buffer: Arc<spin::Mutex<[u8; 1 << 16]>>,
}

impl Behaviour {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            config,
            router: Default::default(),
            peer_to_connection: Default::default(),
            dialing_peers: Default::default(),
            events: Default::default(),
            pending_connections: Default::default(),
            pending_requests: Default::default(),
            error_streams: Default::default(),
            buffer: Arc::new(spin::Mutex::new([0; 1 << 16])),
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

        let sr = Arc::new(sr);
        self.pending_requests.push(sr.clone());
        let Some(&conn_id) = self.peer_to_connection.get(&sr.to) else {
            log::debug!("queueing connection to {} from {}", sr.to, self.config.current_peer_id);
            self.handle_missing_connection(sr.to);
            return;
        };

        self.events.push_back(TS::NotifyHandler {
            peer_id: sr.to,
            handler: NotifyHandler::One(conn_id),
            event: handler::FromBehaviour::InitPacket(IncomingOrRequest::Request(sr)),
        });
    }

    fn push_incoming_stream(&mut self, is: IncomingStream) {
        log::debug!("incoming stream from");
        let valid_stream_count = self
            .router
            .iter_mut()
            .filter_map(|c| c.is_valid(self.config.keep_alive_interval).then_some(()))
            .count();
        if valid_stream_count + self.pending_connections.len() > self.config.max_streams {
            log::info!("too many streams");
            if self.error_streams.len() > self.config.max_error_streams {
                log::warn!("too many erroring streams");
                return;
            }

            self.error_streams.push(ClosingStream::new(is.stream, packet::OCCUPIED_PEER));
            return;
        }

        if is.meta.to == self.config.current_peer_id {
            // most likely melisious since we check this in open_path
            log::warn!("melacious stream or self connection");
            return;
        }

        let meta = is.meta.clone();
        self.pending_connections.push(is);
        let Some(&conn_id) = self.peer_to_connection.get(&meta.to) else {
            log::debug!("queueing connection to {} from {}", meta.to, self.config.current_peer_id);
            self.handle_missing_connection(meta.to);
            return;
        };

        self.events.push_back(TS::NotifyHandler {
            peer_id: meta.to,
            handler: NotifyHandler::One(conn_id),
            event: handler::FromBehaviour::InitPacket(IncomingOrRequest::Incoming(meta)),
        });
    }

    fn add_connection(&mut self, to: PeerId, to_id: ConnectionId) {
        if self.peer_to_connection.get(&to).is_some() {
            log::debug!("connection to {} already exists", to);
            return;
        }

        self.peer_to_connection.insert(to, to_id);

        let incoming = self
            .pending_connections
            .iter()
            .filter(|p| p.meta.to == to)
            .map(|p| p.meta.clone())
            .map(IncomingOrRequest::Incoming);
        let requests = self
            .pending_requests
            .iter()
            .filter(|p| p.to == to)
            .cloned()
            .map(IncomingOrRequest::Request);

        log::debug!("adding connection to {to} from {}", self.config.current_peer_id);

        for p in incoming.chain(requests) {
            log::debug!("sending pending stream to {to}");
            self.events.push_back(TS::NotifyHandler {
                peer_id: to,
                handler: NotifyHandler::One(to_id),
                event: handler::FromBehaviour::InitPacket(p),
            });
        }
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

    fn handle_missing_connection(&mut self, to: PeerId) {
        if self.config.dial {
            self.dial(to);
        } else {
            self.events.push_back(TS::GenerateEvent(Event::ConnectRequest(to)));
        }
    }

    fn dial(&mut self, peer: PeerId) {
        let opts = DialOpts::from(peer);
        self.dialing_peers.insert(opts.connection_id(), peer);
        self.events.push_back(TS::Dial { opts });
    }

    fn create_handler(&mut self, peer: PeerId, connection_id: ConnectionId) -> Handler {
        self.add_connection(peer, connection_id);
        Handler::new(self.config.secret.clone(), self.config.buffer_cap)
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: libp2p::identity::PeerId,
        _local_addr: &libp2p::core::Multiaddr,
        _remote_addr: &libp2p::core::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(self.create_handler(peer, connection_id))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: libp2p::identity::PeerId,
        _addr: &libp2p::core::Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(self.create_handler(peer, connection_id))
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        if let libp2p::swarm::FromSwarm::ConnectionClosed(c) = event {
            if self.pending_requests.iter().any(|p| p.to == c.peer_id)
                || self.pending_connections.iter().any(|p| p.meta.to == c.peer_id)
            {
                self.events.push_back(TS::GenerateEvent(Event::ConnectRequest(c.peer_id)));
            }
            log::debug!("connection closed to {}", c.peer_id);
            self.peer_to_connection.remove(&c.peer_id);
        }

        if let libp2p::swarm::FromSwarm::DialFailure(e) = event
            && let Some(peer) = self.dialing_peers.remove(&e.connection_id)
            && !matches!(e.error, DialError::DialPeerConditionFalse(_))
        {
            self.report_unreachable(peer);
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: libp2p::identity::PeerId,
        _connection_id: ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        use crate::handler::ToBehaviour as HTB;
        match event {
            HTB::NewChannel(to, path_id) => {
                let Some(from) =
                    self.pending_connections.find_and_remove(|p| p.meta.path_id == path_id)
                else {
                    log::error!("no pending connection for path id {}", peer_id);
                    return;
                };
                self.router.push(Channel::new(
                    from.stream,
                    to,
                    self.config.buffer_cap,
                    self.buffer.clone(),
                ));
            }
            HTB::IncomingStream(IncomingOrResponse::Incoming(s)) => self.push_incoming_stream(s),
            HTB::IncomingStream(IncomingOrResponse::Response(s)) => {
                self.events.push_back(TS::GenerateEvent(Event::InboundStream(s, PathId::new())));
            }
            HTB::OutboundStream { to, id, from } => {
                if self.pending_requests.find_and_remove(|p| p.path_id == id).is_none() {
                    log::error!("no pending request for path id {:?}", id);
                    return;
                }
                self.events.push_back(TS::GenerateEvent(Event::OutboundStream(
                    to.map(|to| (to, from)),
                    id,
                )));
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<TS<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        if let Poll::Ready(Some(Err(e))) = self.router.poll_next_unpin(cx) {
            log::debug!("router error: {}", e);
        }

        if let Poll::Ready(Some(Err(e))) = self.error_streams.poll_next_unpin(cx) {
            log::debug!("error stream error: {}", e);
        }

        Poll::Pending
    }
}

component_utils::gen_config! {
    /// The secret key used as identiry for the node. In case you pass None, client mode is
    /// enabled and new secret created for each connection.
    secret: Option<KeyPair>,
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
    ConnectRequest(PeerId),
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
        let reserved = writer.write([0u8; PACKET_LEN_WIDTH])?;
        let raw = writer.write(data)?;
        let tag = aes.encrypt_in_place_detached(&nonce, ASOC_DATA, raw).expect("no");
        let full_len = raw.len() + tag.len() + nonce.len();
        writer.write_bytes(&tag)?;
        writer.write_bytes(&nonce)?;
        reserved.copy_from_slice(&encode_len(full_len));

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
