#![feature(extract_if)]
#![feature(hash_extract_if)]
#![feature(let_chains)]
#![feature(iter_collect_into)]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(macro_metavar_expr)]
#![feature(if_let_guard)]

use {
    libp2p::{
        core::upgrade::ReadyUpgrade,
        swarm::{
            dial_opts::{DialOpts, PeerCondition},
            ConnectionHandler, ConnectionId, DialError, DialFailure, NetworkBehaviour,
            NotifyHandler, StreamUpgradeError, SubstreamProtocol,
        },
        PeerId, StreamProtocol,
    },
    std::{
        collections::{hash_map::Entry, HashMap},
        io,
        task::Poll,
    },
};

component_utils::decl_stream_protocol!(PROTOCOL_NAME = "streaming");

#[derive(Default, Debug)]
pub struct Connection {
    id: Option<ConnectionId>,
    pending_requests: usize,
    new_requests: usize,
}
impl Connection {
    fn total_requests(&self) -> usize {
        self.pending_requests + self.new_requests
    }
}

#[derive(Default)]
pub struct Behaviour {
    connections: HashMap<PeerId, Connection>,
    events: Vec<Event>,
    waker: Option<std::task::Waker>,
}

impl Behaviour {
    pub fn new() -> Self {
        Self { ..Default::default() }
    }

    pub fn is_resolving_stream_for(&self, peer: PeerId) -> bool {
        self.connections.get(&peer).is_some_and(|c| c.pending_requests > 0 || c.new_requests > 0)
    }

    pub fn create_stream(&mut self, with: PeerId) {
        self.connections.entry(with).or_default().new_requests += 1;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    pub fn new_handler(&mut self, peer: PeerId, proto: fn() -> StreamProtocol) -> Handler {
        let conn = self.connections.entry(peer).or_default();
        conn.pending_requests += conn.new_requests;
        conn.new_requests = 0;
        Handler::new(conn.pending_requests, proto)
    }
}

fn new_dial_opts(peer: PeerId, id: &mut Option<ConnectionId>) -> DialOpts {
    let opts = DialOpts::peer_id(peer).condition(PeerCondition::DisconnectedAndNotDialing).build();
    *id = Some(opts.connection_id());
    opts
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        peer_id: PeerId,
        _: &libp2p::Multiaddr,
        _: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(self.new_handler(peer_id, || PROTOCOL_NAME))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        peer_id: PeerId,
        _: &libp2p::Multiaddr,
        _: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(self.new_handler(peer_id, || PROTOCOL_NAME))
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        match event {
            libp2p::swarm::FromSwarm::DialFailure(DialFailure {
                error,
                peer_id: Some(peer_id),
                connection_id,
                ..
            }) if let Entry::Occupied(conn) = self.connections.entry(peer_id)
                && conn.get().id == Some(connection_id) =>
            {
                if !matches!(error, DialError::DialPeerConditionFalse(_)) {
                    for _ in 0..=conn.get().total_requests() {
                        self.events.push(Event::OutgoingStream(
                            peer_id,
                            Err(StreamUpgradeError::Io(io::Error::other(error.to_string()))),
                        ));
                    }
                    conn.remove();
                }
            }
            libp2p::swarm::FromSwarm::ConnectionClosed(c) => {
                if let Entry::Occupied(conn) = self.connections.entry(c.peer_id) {
                    for _ in 0..=conn.get().total_requests() {
                        self.events.push(Event::OutgoingStream(
                            c.peer_id,
                            Err(StreamUpgradeError::Io(io::ErrorKind::ConnectionReset.into())),
                        ));
                    }
                    conn.remove();
                }
            }
            libp2p::swarm::FromSwarm::ConnectionEstablished(c) => {
                let conn = self.connections.entry(c.peer_id).or_default();
                conn.id = Some(c.connection_id);
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _connection_id: ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.events.push(match event {
            HEvent::Incoming(stream) => Event::IncomingStream(peer_id, stream),
            HEvent::Outgoing(e) => Event::OutgoingStream(peer_id, e),
        });
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        if let Some((peer, conn)) = self.connections.iter_mut().find(|(_, c)| c.id.is_none()) {
            return Poll::Ready(libp2p::swarm::ToSwarm::Dial {
                opts: new_dial_opts(*peer, &mut conn.id),
            });
        }

        if let Some((peer, id, conn)) = self
            .connections
            .iter_mut()
            .filter(|(_, c)| c.new_requests > 0)
            .find_map(|(p, c)| c.id.map(|id| (p, id, c)))
        {
            conn.pending_requests += conn.new_requests;
            return Poll::Ready(libp2p::swarm::ToSwarm::NotifyHandler {
                peer_id: *peer,
                handler: NotifyHandler::One(id),
                event: std::mem::take(&mut conn.new_requests),
            });
        }

        if let Some(event) = self.events.pop() {
            return Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(event));
        }

        component_utils::set_waker(&mut self.waker, cx.waker());
        Poll::Pending
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
#[allow(clippy::type_complexity)]
pub enum Event {
    IncomingStream(PeerId, libp2p::Stream),
    OutgoingStream(PeerId, Result<libp2p::Stream, Error>),
}

pub type Error = StreamUpgradeError<void::Void>;

pub struct Handler {
    to_request: usize,
    stream: Vec<HEvent>,
    waker: Option<std::task::Waker>,
    proto: fn() -> StreamProtocol,
}

impl Handler {
    fn new(to_request: usize, proto: fn() -> StreamProtocol) -> Self {
        Self { to_request, stream: Vec::new(), waker: None, proto }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = usize;
    type InboundOpenInfo = ();
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundOpenInfo = ();
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type ToBehaviour = HEvent;

    fn listen_protocol(
        &self,
    ) -> libp2p::swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        libp2p::swarm::SubstreamProtocol::new(ReadyUpgrade::new((self.proto)()), ())
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<
        libp2p::swarm::ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
        >,
    > {
        if let Some(stream) = self.stream.pop() {
            return Poll::Ready(libp2p::swarm::ConnectionHandlerEvent::NotifyBehaviour(stream));
        }

        if let Some(next_requested) = self.to_request.checked_sub(1) {
            self.to_request = next_requested;
            return Poll::Ready(libp2p::swarm::ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new((self.proto)()), ()),
            });
        }

        component_utils::set_waker(&mut self.waker, cx.waker());
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, glass: Self::FromBehaviour) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
        self.to_request = glass;
    }

    fn on_connection_event(
        &mut self,
        event: libp2p::swarm::handler::ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        use libp2p::swarm::handler::ConnectionEvent as E;
        let ev = match event {
            E::FullyNegotiatedInbound(i) => HEvent::Incoming(i.protocol),
            E::FullyNegotiatedOutbound(o) => HEvent::Outgoing(Ok(o.protocol)),
            E::DialUpgradeError(e) => HEvent::Outgoing(Err(e.error)),
            _ => return,
        };
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
        self.stream.push(ev);
    }
}

#[derive(Debug)]
pub enum HEvent {
    Incoming(libp2p::Stream),
    Outgoing(Result<libp2p::Stream, Error>),
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crypto::rand_core::OsRng,
        dht::Route,
        libp2p::{
            futures::{self, stream::FuturesUnordered, StreamExt},
            multiaddr::Protocol,
            Multiaddr, Transport,
        },
        opfusk::ToPeerId as _,
        std::{future::Future, time::Duration},
    };

    #[derive(NetworkBehaviour, Default)]
    struct TestBehatiour {
        rpc: Behaviour,
        dht: dht::Behaviour,
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_random_rpc_calls() {
        env_logger::init();

        let pks = (0..10).map(|_| crypto::sign::Keypair::new(OsRng)).collect::<Vec<_>>();
        let public_keys = pks.iter().map(|kp| kp.public_key()).collect::<Vec<_>>();
        let peer_ids = pks.iter().map(|kp| kp.to_peer_id()).collect::<Vec<_>>();
        let servers = pks.into_iter().enumerate().map(|(i, kp)| {
            let i = i + 1;
            let beh = TestBehatiour::default();
            let transport = LatentTransport::new(
                libp2p::core::transport::memory::MemoryTransport::default(),
                Duration::from_millis(10),
            )
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(opfusk::Config::new(OsRng, kp))
            .multiplex(libp2p::yamux::Config::default())
            .boxed();
            let mut swarm = libp2p::Swarm::new(
                transport,
                beh,
                kp.to_peer_id(),
                libp2p::swarm::Config::with_tokio_executor()
                    .with_idle_connection_timeout(Duration::from_secs(10)),
            );

            swarm.listen_on(Multiaddr::empty().with(Protocol::Memory(i as u64))).unwrap();

            for (j, pk) in public_keys.iter().enumerate() {
                let j = j + 1;
                swarm.behaviour_mut().dht.table.write().insert(Route::new(
                    pk.identity(),
                    Multiaddr::empty().with(Protocol::Memory(j as u64)),
                ));
            }

            swarm
        });

        async fn run_server(swarm: &mut libp2p::Swarm<TestBehatiour>, mut all_peers: Vec<PeerId>) {
            all_peers.retain(|p| p != swarm.local_peer_id());
            let max_pending_requests = 10;
            let mut pending_request_count = 0;
            let mut iteration = 0;
            let mut total_requests = 0;
            let mut connections = vec![];
            while total_requests < 3000 {
                if max_pending_requests > pending_request_count {
                    let peer_id = all_peers[iteration % all_peers.len()];
                    swarm.behaviour_mut().rpc.create_stream(peer_id);
                    pending_request_count += 1;
                    total_requests += 1;
                }

                let e = libp2p::futures::select! {
                    e = swarm.select_next_some() => e,
                };

                if total_requests % 500 == 0 {
                    log::info!("total requests: {}", total_requests);
                }

                match e {
                    libp2p::swarm::SwarmEvent::Behaviour(TestBehatiourEvent::Rpc(
                        Event::OutgoingStream(p, e),
                    )) => {
                        if let Err(e) = e {
                            log::error!("outgoing stream error: {:?} {:?}", p, e);
                        }
                        pending_request_count -= 1;
                        if total_requests % 100 == 0 {
                            _ = swarm.disconnect_peer_id(p);
                        }
                    }
                    libp2p::swarm::SwarmEvent::Behaviour(TestBehatiourEvent::Rpc(
                        Event::IncomingStream(..),
                    )) => {
                        total_requests += 1;
                    }
                    libp2p::swarm::SwarmEvent::ConnectionEstablished {
                        peer_id,
                        connection_id,
                        endpoint,
                        num_established,
                        concurrent_dial_errors,
                        established_in,
                    } => {
                        log::info!(
                            "connection established: {:?} {:?} {:?} {:?} {:?} {:?}",
                            peer_id,
                            connection_id,
                            endpoint,
                            num_established,
                            concurrent_dial_errors,
                            established_in
                        );
                        connections.push(connection_id);
                    }
                    e => {
                        log::info!("event: {:?}", e);
                    }
                }

                iteration += 1;
            }
        }

        servers
            .map(|mut s| {
                let pids = peer_ids.clone();
                async move {
                    if tokio::time::timeout(Duration::from_secs(10), run_server(&mut s, pids))
                        .await
                        .is_err()
                    {
                        log::error!("server timed out {:?}", s.behaviour().rpc.connections);
                    }
                }
            })
            .collect::<FuturesUnordered<_>>()
            .next()
            .await;
    }

    pub struct LatentTransport<T> {
        inner: T,
        latency: Duration,
    }

    impl<T> LatentTransport<T> {
        pub fn new(inner: T, latency: Duration) -> Self {
            Self { inner, latency }
        }
    }

    impl<T: Transport> Transport for LatentTransport<T> {
        type Dial = LatentFuture<T::Dial>;
        type Error = T::Error;
        type ListenerUpgrade = LatentFuture<T::ListenerUpgrade>;
        type Output = LatentStream<T::Output>;

        fn listen_on(
            &mut self,
            id: libp2p::core::transport::ListenerId,
            addr: Multiaddr,
        ) -> Result<(), libp2p::TransportError<Self::Error>> {
            self.inner.listen_on(id, addr)
        }

        fn remove_listener(&mut self, id: libp2p::core::transport::ListenerId) -> bool {
            self.inner.remove_listener(id)
        }

        fn dial(
            &mut self,
            addr: Multiaddr,
        ) -> Result<Self::Dial, libp2p::TransportError<Self::Error>> {
            Ok(LatentFuture::new(self.inner.dial(addr)?, self.latency))
        }

        fn dial_as_listener(
            &mut self,
            addr: Multiaddr,
        ) -> Result<Self::Dial, libp2p::TransportError<Self::Error>> {
            Ok(LatentFuture::new(self.inner.dial_as_listener(addr)?, self.latency))
        }

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<libp2p::core::transport::TransportEvent<Self::ListenerUpgrade, Self::Error>>
        {
            let s = unsafe { self.get_unchecked_mut() };
            Transport::poll(unsafe { std::pin::Pin::new_unchecked(&mut s.inner) }, cx)
                .map(|e| e.map_upgrade(|u| LatentFuture::new(u, Duration::ZERO)))
        }

        fn address_translation(
            &self,
            listen: &Multiaddr,
            observed: &Multiaddr,
        ) -> Option<Multiaddr> {
            self.inner.address_translation(listen, observed)
        }
    }

    pub struct LatentFuture<F> {
        inner: F,
        delay: Option<tokio::time::Sleep>,
        latency: Duration,
    }

    impl<F> LatentFuture<F> {
        pub fn new(inner: F, latency: Duration) -> Self {
            Self { inner, delay: Some(tokio::time::sleep(latency)), latency }
        }
    }

    impl<O, E, F: futures::Future> futures::Future for LatentFuture<F>
    where
        F: futures::Future<Output = Result<O, E>>,
    {
        type Output = Result<LatentStream<O>, E>;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<Self::Output> {
            let s = unsafe { self.get_unchecked_mut() };

            if let Some(ref mut sleep) = s.delay {
                futures::ready!(unsafe { std::pin::Pin::new_unchecked(sleep) }.poll(cx));
                s.delay = None;
            }

            unsafe { std::pin::Pin::new_unchecked(&mut s.inner) }
                .poll(cx)
                .map_ok(|inner| LatentStream::new(inner, s.latency))
        }
    }

    pub struct LatentStream<S> {
        inner: S,
        sleep: Box<tokio::time::Sleep>,
        latency: Duration,
    }

    impl<S> LatentStream<S> {
        pub fn new(inner: S, latency: Duration) -> Self {
            Self { inner, sleep: tokio::time::sleep(latency).into(), latency }
        }
    }

    impl<S: futures::AsyncRead> futures::AsyncRead for LatentStream<S> {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            // no delay, sender has delay on flush which is enough
            let s = unsafe { self.get_unchecked_mut() };
            unsafe { std::pin::Pin::new_unchecked(&mut s.inner) }.poll_read(cx, buf)
        }
    }

    impl<S: futures::AsyncWrite> futures::AsyncWrite for LatentStream<S> {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let s = unsafe { self.get_unchecked_mut() };
            futures::ready!(unsafe { std::pin::Pin::new_unchecked(&mut *s.sleep) }.poll(cx));
            let res = futures::ready!(
                unsafe { std::pin::Pin::new_unchecked(&mut s.inner) }.poll_write(cx, buf)
            );
            s.sleep = tokio::time::sleep(s.latency).into();
            Poll::Ready(res)
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<io::Result<()>> {
            let s = unsafe { self.get_unchecked_mut() };
            futures::ready!(unsafe { std::pin::Pin::new_unchecked(&mut *s.sleep) }.poll(cx));
            let res = futures::ready!(
                unsafe { std::pin::Pin::new_unchecked(&mut s.inner) }.poll_flush(cx)
            );
            s.sleep = tokio::time::sleep(s.latency).into();
            Poll::Ready(res)
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<io::Result<()>> {
            let s = unsafe { self.get_unchecked_mut() };
            futures::ready!(unsafe { std::pin::Pin::new_unchecked(&mut *s.sleep) }.poll(cx));
            let res = futures::ready!(
                unsafe { std::pin::Pin::new_unchecked(&mut s.inner) }.poll_close(cx)
            );
            s.sleep = tokio::time::sleep(s.latency).into();
            Poll::Ready(res)
        }
    }
}
