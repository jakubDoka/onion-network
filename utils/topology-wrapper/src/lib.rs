#![feature(impl_trait_in_assoc_type)]
#![feature(assert_matches)]
#![feature(macro_metavar_expr)]

pub use impls::{channel, new, Behaviour, EventReceiver, EventSender};
use {
    component_utils::{Buffer, Codec, WritableBuffer},
    libp2p::{multihash::Multihash, PeerId},
};

pub enum ExtraEvent {
    Stream(String),
    Disconnected,
}

pub enum PacketKind {
    Sent,
    Closed,
}

pub type ExtraEventAndMeta = (ExtraEvent, PeerIdWrapper, libp2p::swarm::ConnectionId);
pub type PacketMeta = (PeerIdWrapper, usize, String);

const INIT_TAG: [u8; 32] = [0xff; 32];

#[cfg(feature = "disabled")]
pub mod report {
    use crate::EventReceiver;
    pub type Behaviour = libp2p::swarm::dummy::Behaviour;

    pub fn new(_: EventReceiver) -> Behaviour {
        libp2p::swarm::dummy::Behaviour
    }
}

#[cfg(feature = "disabled")]
mod impls {
    pub type EventSender = ();
    pub type EventReceiver = ();

    pub type Behaviour<T> = T;
    pub fn new<T>(inner: T, _: EventSender) -> T {
        inner
    }
    pub fn channel() -> (EventSender, EventReceiver) {
        ((), ())
    }
}

#[cfg(not(feature = "disabled"))]
pub mod collector {
    use {
        crate::report::Update,
        component_utils::Codec,
        libp2p::{
            futures::{stream::SelectAll, StreamExt},
            swarm::NetworkBehaviour,
            PeerId,
        },
        std::{convert::Infallible, io},
    };

    pub trait World: 'static {
        fn handle_update(&mut self, peer: PeerId, update: Update);
    }

    struct UpdateStream {
        peer: libp2p::PeerId,
        inner: libp2p::Stream,
        reader: component_utils::stream::PacketReader,
    }

    impl libp2p::futures::Stream for UpdateStream {
        type Item = (io::Result<Vec<u8>>, PeerId);

        fn poll_next(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            let s = self.get_mut();
            s.reader.poll_packet(cx, &mut s.inner).map(|r| Some((r.map(|r| r.to_vec()), s.peer)))
        }
    }

    pub struct Behaviour<W: World> {
        peer_id: PeerId,
        world: W,
        listeners: SelectAll<UpdateStream>,
        pending_connections: Vec<PeerId>,
    }

    impl<W: World> Behaviour<W> {
        pub fn new(peer_id: PeerId, world: W) -> Self {
            Self {
                peer_id,
                world,
                listeners: Default::default(),
                pending_connections: Default::default(),
            }
        }

        pub fn world_mut(&mut self) -> &mut W {
            &mut self.world
        }

        pub fn add_peer(&mut self, addr: PeerId) {
            self.pending_connections.push(addr);
        }
    }

    impl<W: World> NetworkBehaviour for Behaviour<W> {
        type ConnectionHandler = crate::report::Handler;
        type ToSwarm = Infallible;

        fn handle_established_inbound_connection(
            &mut self,
            _connection_id: libp2p::swarm::ConnectionId,
            _peer: PeerId,
            _local_addr: &libp2p::Multiaddr,
            _remote_addr: &libp2p::Multiaddr,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            Ok(crate::report::Handler::connecting())
        }

        fn handle_established_outbound_connection(
            &mut self,
            _connection_id: libp2p::swarm::ConnectionId,
            _peer: PeerId,
            _addr: &libp2p::Multiaddr,
            _role_override: libp2p::core::Endpoint,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            Ok(crate::report::Handler::connecting())
        }

        fn on_swarm_event(&mut self, _event: libp2p::swarm::FromSwarm) {}

        fn on_connection_handler_event(
            &mut self,
            peer_id: PeerId,
            _connection_id: libp2p::swarm::ConnectionId,
            event: libp2p::swarm::THandlerOutEvent<Self>,
        ) {
            if self.listeners.iter().any(|l| l.peer == peer_id) {
                return;
            }
            self.listeners.push(UpdateStream {
                peer: peer_id,
                inner: event,
                reader: component_utils::stream::PacketReader::default(),
            });
        }

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<
            libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>,
        > {
            if let Some(peer) = self.pending_connections.pop() {
                return std::task::Poll::Ready(libp2p::swarm::ToSwarm::Dial { opts: peer.into() });
            }

            loop {
                let Some((packet, peer)) =
                    libp2p::futures::ready!(self.listeners.poll_next_unpin(cx))
                else {
                    return std::task::Poll::Pending;
                };

                let Ok(packet) =
                    packet.inspect_err(|e| log::info!("Error while reading update: {}", e))
                else {
                    return std::task::Poll::Pending;
                };

                let Some(update) = Update::decode(&mut packet.as_slice()) else {
                    log::info!("Invalid update received, {:?}", packet);
                    return std::task::Poll::Pending;
                };

                if update.peer.0 != self.peer_id {
                    self.world.handle_update(peer, update);
                }
            }
        }
    }
}

#[cfg(not(feature = "disabled"))]
pub mod muxer {
    use {
        crate::{PacketKind, PacketMeta},
        libp2p::{
            core::StreamMuxer,
            futures::{AsyncRead, AsyncWrite, SinkExt},
        },
        std::pin::Pin,
    };

    pub struct Muxer<T> {
        inner: T,
        sender: crate::EventSender,
    }

    impl<T> Muxer<T> {
        pub fn new(inner: T, sender: crate::EventSender) -> Self {
            Self { inner, sender }
        }
    }

    impl<T: StreamMuxer + Unpin> StreamMuxer for Muxer<T>
    where
        T::Substream: Unpin,
    {
        type Error = T::Error;
        type Substream = Substream<T::Substream>;

        fn poll_inbound(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<Self::Substream, Self::Error>> {
            let stream = libp2p::futures::ready!(Pin::new(&mut self.inner).poll_inbound(cx))?;
            let sender = self.sender.clone();
            std::task::Poll::Ready(Ok(Substream {
                inner: stream,
                sender,
                meta: None,
                sending: false,
                closing_res: None,
            }))
        }

        fn poll_outbound(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<Self::Substream, Self::Error>> {
            let stream = libp2p::futures::ready!(Pin::new(&mut self.inner).poll_outbound(cx))?;
            let sender = self.sender.clone();
            std::task::Poll::Ready(Ok(Substream {
                inner: stream,
                sender,
                meta: None,
                sending: false,
                closing_res: None,
            }))
        }

        fn poll_close(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            Pin::new(&mut self.inner).poll_close(cx)
        }

        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<libp2p::core::muxing::StreamMuxerEvent, Self::Error>> {
            Pin::new(&mut self.inner).poll(cx)
        }
    }

    pub struct Substream<T> {
        inner: T,
        sender: crate::EventSender,
        meta: Option<crate::PacketMeta>,
        sending: bool,
        closing_res: Option<std::io::Result<()>>,
    }

    impl<T: AsyncRead + Unpin> AsyncRead for Substream<T> {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl<T: AsyncWrite + Unpin> AsyncWrite for Substream<T> {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            if self.meta.is_none() {
                use component_utils::Codec;
                if let Some((sig, meta)) = <([u8; 32], PacketMeta)>::decode(&mut &*buf) {
                    if sig == crate::INIT_TAG {
                        self.meta = Some(meta);
                        return std::task::Poll::Ready(Ok(buf.len()));
                    }
                }
            }

            if !self.sending
                && buf.len() > 4
                && libp2p::futures::ready!(self.sender.packets.poll_ready(cx)).is_ok()
            {
                if let Some(meta) = self.meta.clone() {
                    let _ = self.sender.packets.start_send((meta, PacketKind::Sent));
                }
            }
            _ = self.sender.packets.poll_flush_unpin(cx);

            self.sending = true;
            let res = libp2p::futures::ready!(Pin::new(&mut self.inner).poll_write(cx, buf));
            self.sending = false;
            std::task::Poll::Ready(res)
        }

        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            _ = self.sender.packets.poll_flush_unpin(cx);
            Pin::new(&mut self.inner).poll_flush(cx)
        }

        fn poll_close(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if self.closing_res.is_none() {
                self.closing_res =
                    Some(libp2p::futures::ready!(Pin::new(&mut self.inner).poll_close(cx)));
            }

            if libp2p::futures::ready!(self.sender.packets.poll_ready(cx)).is_ok() {
                if let Some(meta) = self.meta.take() {
                    _ = self.sender.packets.start_send((meta, PacketKind::Closed));
                }
            }
            _ = libp2p::futures::ready!(self.sender.packets.poll_flush_unpin(cx));

            std::task::Poll::Ready(self.closing_res.take().unwrap())
        }
    }
}

#[cfg(not(feature = "disabled"))]
pub mod report {
    use {
        crate::{EventReceiver, ExtraEvent, PeerIdWrapper},
        component_utils::{Codec, PacketWriter},
        libp2p::{
            core::UpgradeInfo,
            futures::{stream::FuturesUnordered, StreamExt},
            swarm::{ConnectionHandler, NetworkBehaviour},
            PeerId, StreamProtocol,
        },
        std::{
            collections::{HashMap, HashSet},
            convert::Infallible,
            io,
        },
    };

    #[must_use]
    pub fn new(recv: EventReceiver) -> Behaviour {
        Behaviour { listeners: Default::default(), topology: Default::default(), recv }
    }

    #[derive(Codec)]
    pub struct Update<'a> {
        pub event: Event<'a>,
        pub peer: PeerIdWrapper,
        pub connection: usize,
    }

    #[derive(Codec)]
    pub enum Event<'a> {
        Stream(&'a str),
        Packet(&'a str),
        Closed(&'a str),
        Disconnected,
    }

    struct UpdateStream {
        peer: PeerId,
        inner: libp2p::Stream,
        writer: component_utils::stream::PacketWriter,
    }

    impl std::future::Future for UpdateStream {
        type Output = io::Result<()>;

        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            let s = &mut *self;
            libp2p::futures::ready!(s.writer.poll(cx, &mut s.inner))?;
            std::task::Poll::Pending
        }
    }

    pub struct Behaviour {
        listeners: FuturesUnordered<UpdateStream>,
        topology: HashMap<PeerIdWrapper, HashMap<usize, HashSet<String>>>,
        recv: EventReceiver,
    }

    impl NetworkBehaviour for Behaviour {
        type ConnectionHandler = Handler;
        type ToSwarm = Infallible;

        fn handle_established_inbound_connection(
            &mut self,
            _connection_id: libp2p::swarm::ConnectionId,
            _peer: PeerId,
            _local_addr: &libp2p::Multiaddr,
            _remote_addr: &libp2p::Multiaddr,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            Ok(Handler::default())
        }

        fn handle_established_outbound_connection(
            &mut self,
            _connection_id: libp2p::swarm::ConnectionId,
            _peer: PeerId,
            _addr: &libp2p::Multiaddr,
            _role_override: libp2p::core::Endpoint,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            Ok(Handler::default())
        }

        fn on_swarm_event(&mut self, _event: libp2p::swarm::FromSwarm) {}

        fn on_connection_handler_event(
            &mut self,
            peer_id: PeerId,
            _connection_id: libp2p::swarm::ConnectionId,
            event: libp2p::swarm::THandlerOutEvent<Self>,
        ) {
            if self.listeners.iter().any(|l| l.peer == peer_id) {
                return;
            }
            let mut stream =
                UpdateStream { peer: peer_id, inner: event, writer: PacketWriter::new(1 << 13) };

            for (peer, connections) in &self.topology {
                for (connection, protocols) in connections {
                    for protocol in protocols {
                        let update = Update {
                            event: Event::Stream(protocol.as_str()),
                            peer: *peer,
                            connection: *connection,
                        };
                        stream.writer.write_packet(&update);
                    }
                }
            }

            self.listeners.push(stream);
        }

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<
            libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>,
        > {
            if let std::task::Poll::Ready(Some(Err(e))) = self.listeners.poll_next_unpin(cx) {
                log::info!("Error while writing update: {}", e);
            }

            while let std::task::Poll::Ready(Some((extra, peer, connection))) =
                self.recv.events.poll_next_unpin(cx)
            {
                let event = match &extra {
                    ExtraEvent::Stream(i) => Event::Stream(i.as_ref()),
                    ExtraEvent::Disconnected => Event::Disconnected,
                };

                let connection = unsafe { std::mem::transmute(connection) };
                let update = Update { event, peer, connection };
                for l in &mut self.listeners {
                    l.writer.write_packet(&update);
                }

                match extra {
                    ExtraEvent::Stream(p) => {
                        self.topology
                            .entry(peer)
                            .or_default()
                            .entry(connection)
                            .or_default()
                            .insert(p);
                    }
                    ExtraEvent::Disconnected => {
                        let peer_state = self.topology.entry(peer).or_default();
                        peer_state.remove(&update.connection);
                        if peer_state.is_empty() {
                            self.topology.remove(&peer);
                        }
                    }
                };
            }

            while let std::task::Poll::Ready(Some(((peer, connection, proto), kind))) =
                self.recv.packets.poll_next_unpin(cx)
            {
                let event = match kind {
                    crate::PacketKind::Sent => Event::Packet(proto.as_str()),
                    crate::PacketKind::Closed => Event::Closed(proto.as_str()),
                };

                let update = Update { event, peer, connection };
                for l in &mut self.listeners {
                    l.writer.write_packet(&update);
                }

                if matches!(kind, crate::PacketKind::Closed) {
                    self.topology
                        .entry(peer)
                        .or_default()
                        .entry(connection)
                        .or_default()
                        .remove(proto.as_str());
                }
            }

            std::task::Poll::Pending
        }
    }

    #[derive(Default)]
    pub struct Handler {
        connected: Option<libp2p::Stream>,
        connect: bool,
    }

    impl Handler {
        #[must_use]
        pub fn connecting() -> Self {
            Self { connected: None, connect: true }
        }
    }

    impl ConnectionHandler for Handler {
        type FromBehaviour = Infallible;
        type InboundOpenInfo = ();
        type InboundProtocol = Protocol;
        type OutboundOpenInfo = ();
        type OutboundProtocol = Protocol;
        type ToBehaviour = libp2p::Stream;

        fn listen_protocol(
            &self,
        ) -> libp2p::swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo>
        {
            libp2p::swarm::SubstreamProtocol::new(Protocol, ())
        }

        fn poll(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<
            libp2p::swarm::ConnectionHandlerEvent<
                Self::OutboundProtocol,
                Self::OutboundOpenInfo,
                Self::ToBehaviour,
            >,
        > {
            if self.connect {
                self.connect = false;
                return std::task::Poll::Ready(
                    libp2p::swarm::ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: libp2p::swarm::SubstreamProtocol::new(Protocol, ()),
                    },
                );
            }

            if let Some(stream) = self.connected.take() {
                return std::task::Poll::Ready(
                    libp2p::swarm::ConnectionHandlerEvent::NotifyBehaviour(stream),
                );
            }

            std::task::Poll::Pending
        }

        fn on_behaviour_event(&mut self, _event: Self::FromBehaviour) {}

        fn on_connection_event(
            &mut self,
            event: libp2p::swarm::handler::ConnectionEvent<
                Self::InboundProtocol,
                Self::OutboundProtocol,
                Self::InboundOpenInfo,
                Self::OutboundOpenInfo,
            >,
        ) {
            match event {
                libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedInbound(i) => {
                    self.connected = Some(i.protocol);
                }
                libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(o) => {
                    self.connected = Some(o.protocol);
                }
                _ => {}
            }
        }
    }

    component_utils::decl_stream_protocol!(ROUTING_PROTOCOL = "updt");

    pub struct Protocol;

    impl UpgradeInfo for Protocol {
        type Info = StreamProtocol;
        type InfoIter = std::iter::Once<Self::Info>;

        fn protocol_info(&self) -> Self::InfoIter {
            std::iter::once(ROUTING_PROTOCOL)
        }
    }

    impl libp2p::core::upgrade::InboundUpgrade<libp2p::Stream> for Protocol {
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Output, Self::Error>>;
        type Output = libp2p::Stream;

        fn upgrade_inbound(self, socket: libp2p::Stream, _: Self::Info) -> Self::Future {
            std::future::ready(Ok(socket))
        }
    }

    impl libp2p::core::upgrade::OutboundUpgrade<libp2p::Stream> for Protocol {
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Output, Self::Error>>;
        type Output = libp2p::Stream;

        fn upgrade_outbound(self, socket: libp2p::Stream, _: Self::Info) -> Self::Future {
            std::future::ready(Ok(socket))
        }
    }
}

#[cfg(not(feature = "disabled"))]
mod impls {
    use {
        crate::{ExtraEvent, ExtraEventAndMeta, PacketKind, PacketMeta, PeerIdWrapper},
        component_utils::Codec,
        libp2p::{
            futures::{AsyncWrite, SinkExt},
            swarm::{
                handler::{
                    DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
                    InboundUpgradeSend, ListenUpgradeError, OutboundUpgradeSend, UpgradeInfoSend,
                },
                ConnectionHandler, ConnectionId, NetworkBehaviour,
            },
        },
        std::{
            assert_matches::assert_matches,
            collections::VecDeque,
            mem,
            ops::{Deref, DerefMut},
            pin::Pin,
            task::Poll,
        },
    };

    #[derive(Clone)]
    pub struct EventSender {
        pub(crate) events: libp2p::futures::channel::mpsc::Sender<ExtraEventAndMeta>,
        pub(crate) packets: libp2p::futures::channel::mpsc::Sender<(PacketMeta, PacketKind)>,
    }

    pub struct EventReceiver {
        pub(crate) events: libp2p::futures::channel::mpsc::Receiver<ExtraEventAndMeta>,
        pub(crate) packets: libp2p::futures::channel::mpsc::Receiver<(PacketMeta, PacketKind)>,
    }

    #[must_use]
    pub fn channel() -> (EventSender, EventReceiver) {
        let (events_sender, events_receiver) = libp2p::futures::channel::mpsc::channel(5);
        let (packets_sender, packets_receiver) = libp2p::futures::channel::mpsc::channel(5);
        (EventSender { events: events_sender, packets: packets_sender }, EventReceiver {
            events: events_receiver,
            packets: packets_receiver,
        })
    }

    pub fn new<T: NetworkBehaviour>(inner: T, sender: EventSender) -> Behaviour<T> {
        Behaviour::new(inner, sender)
    }

    pub struct Behaviour<T: NetworkBehaviour> {
        inner: T,
        extra_events: VecDeque<ExtraEventAndMeta>,
        sender: EventSender,
        waker: Option<std::task::Waker>,
    }

    impl<T: NetworkBehaviour> Behaviour<T> {
        fn new(inner: T, sender: EventSender) -> Self {
            Self { inner, sender, extra_events: VecDeque::new(), waker: None }
        }

        fn add_event(&mut self, event: ExtraEvent, peer: libp2p::PeerId, connection: ConnectionId) {
            self.extra_events.push_back((event, PeerIdWrapper(peer), connection));
            if let Some(waker) = mem::take(&mut self.waker) {
                waker.wake();
            }
        }
    }

    impl<T: NetworkBehaviour> NetworkBehaviour for Behaviour<T> {
        type ConnectionHandler = Handler<T::ConnectionHandler>;
        type ToSwarm = T::ToSwarm;

        fn handle_established_inbound_connection(
            &mut self,
            connection_id: libp2p::swarm::ConnectionId,
            peer: libp2p::PeerId,
            local_addr: &libp2p::Multiaddr,
            remote_addr: &libp2p::Multiaddr,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            self.inner
                .handle_established_inbound_connection(connection_id, peer, local_addr, remote_addr)
                .map(|h| Handler::new(h, peer, connection_id))
        }

        fn handle_established_outbound_connection(
            &mut self,
            connection_id: libp2p::swarm::ConnectionId,
            peer: libp2p::PeerId,
            addr: &libp2p::Multiaddr,
            role_override: libp2p::core::Endpoint,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            self.inner
                .handle_established_outbound_connection(connection_id, peer, addr, role_override)
                .map(|h| Handler::new(h, peer, connection_id))
        }

        fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
            if let libp2p::swarm::FromSwarm::ConnectionClosed(c) = &event {
                self.add_event(ExtraEvent::Disconnected, c.peer_id, c.connection_id);
            }

            self.inner.on_swarm_event(event);
        }

        fn on_connection_handler_event(
            &mut self,
            peer_id: libp2p::PeerId,
            connection_id: libp2p::swarm::ConnectionId,
            event: libp2p::swarm::THandlerOutEvent<Self>,
        ) {
            let event = match event {
                ToBehavior::Inner(i) => i,
                ToBehavior::Extra(e) => {
                    self.add_event(e, peer_id, connection_id);
                    return;
                }
            };
            self.inner.on_connection_handler_event(peer_id, connection_id, event);
        }

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<
            libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>,
        > {
            self.waker = Some(cx.waker().clone());
            while let Some(event) = self.extra_events.pop_front() {
                if let Poll::Pending | Poll::Ready(Err(_)) = self.sender.events.poll_ready(cx) {
                    self.extra_events.push_front(event);
                    break;
                }
                _ = self.sender.events.start_send(event);
            }
            _ = self.sender.events.poll_flush_unpin(cx);
            self.inner.poll(cx)
        }

        fn handle_pending_inbound_connection(
            &mut self,
            _connection_id: libp2p::swarm::ConnectionId,
            _local_addr: &libp2p::Multiaddr,
            _remote_addr: &libp2p::Multiaddr,
        ) -> Result<(), libp2p::swarm::ConnectionDenied> {
            self.inner.handle_pending_inbound_connection(_connection_id, _local_addr, _remote_addr)
        }

        fn handle_pending_outbound_connection(
            &mut self,
            _connection_id: libp2p::swarm::ConnectionId,
            _maybe_peer: Option<libp2p::PeerId>,
            _addresses: &[libp2p::Multiaddr],
            _effective_role: libp2p::core::Endpoint,
        ) -> Result<Vec<libp2p::Multiaddr>, libp2p::swarm::ConnectionDenied> {
            self.inner.handle_pending_outbound_connection(
                _connection_id,
                _maybe_peer,
                _addresses,
                _effective_role,
            )
        }
    }

    impl<T: NetworkBehaviour> Deref for Behaviour<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    impl<T: NetworkBehaviour> DerefMut for Behaviour<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner
        }
    }

    pub struct Handler<T: ConnectionHandler> {
        inner: T,
        extra_events: VecDeque<ExtraEvent>,
        opened: bool,
        peer: libp2p::PeerId,
        connection: ConnectionId,
    }

    impl<T: ConnectionHandler> Handler<T> {
        pub fn new(inner: T, peer: libp2p::PeerId, connection: ConnectionId) -> Self {
            Self { inner, extra_events: VecDeque::new(), opened: true, peer, connection }
        }
    }

    impl<T: ConnectionHandler> ConnectionHandler for Handler<T> {
        type FromBehaviour = T::FromBehaviour;
        type InboundOpenInfo = T::InboundOpenInfo;
        type InboundProtocol = Protocol<T::InboundProtocol>;
        type OutboundOpenInfo = T::OutboundOpenInfo;
        type OutboundProtocol = Protocol<T::OutboundProtocol>;
        type ToBehaviour = ToBehavior<T>;

        fn listen_protocol(
            &self,
        ) -> libp2p::swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo>
        {
            self.inner
                .listen_protocol()
                .map_upgrade(|p| Protocol::new(p, self.peer, self.connection))
        }

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<
            libp2p::swarm::ConnectionHandlerEvent<
                Self::OutboundProtocol,
                Self::OutboundOpenInfo,
                Self::ToBehaviour,
            >,
        > {
            if let Some(event) = self.extra_events.pop_front() {
                return std::task::Poll::Ready(
                    libp2p::swarm::ConnectionHandlerEvent::NotifyBehaviour(ToBehavior::Extra(
                        event,
                    )),
                );
            }

            self.inner.poll(cx).map(|e| {
                e.map_custom(ToBehavior::Inner)
                    .map_protocol(|p| Protocol::new(p, self.peer, self.connection))
            })
        }

        fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
            self.inner.on_behaviour_event(event);
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
            use libp2p::swarm::handler::ConnectionEvent as CE;
            let event = match event {
                CE::FullyNegotiatedInbound(i) => {
                    self.extra_events
                        .push_back(ExtraEvent::Stream(i.protocol.1.as_ref().to_string()));
                    libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedInbound(
                        FullyNegotiatedInbound { protocol: i.protocol.0, info: i.info },
                    )
                }
                CE::FullyNegotiatedOutbound(o) => {
                    self.extra_events
                        .push_back(ExtraEvent::Stream(o.protocol.1.as_ref().to_string()));
                    CE::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                        protocol: o.protocol.0,
                        info: o.info,
                    })
                }
                CE::AddressChange(a) => CE::AddressChange(a),
                CE::DialUpgradeError(d) => {
                    CE::DialUpgradeError(DialUpgradeError { error: d.error, info: d.info })
                }
                CE::ListenUpgradeError(l) => {
                    CE::ListenUpgradeError(ListenUpgradeError { error: l.error, info: l.info })
                }
                CE::LocalProtocolsChange(l) => CE::LocalProtocolsChange(l),
                CE::RemoteProtocolsChange(r) => CE::RemoteProtocolsChange(r),
                _ => return,
            };
            self.inner.on_connection_event(event);
        }

        fn connection_keep_alive(&self) -> bool {
            self.inner.connection_keep_alive()
        }

        fn poll_close(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::ToBehaviour>> {
            if self.opened {
                self.opened = false;
                return std::task::Poll::Ready(Some(ToBehavior::Extra(ExtraEvent::Disconnected)));
            }
            self.inner.poll_close(cx).map(|opt| opt.map(ToBehavior::Inner))
        }
    }

    pub enum ToBehavior<C: ConnectionHandler> {
        Inner(C::ToBehaviour),
        Extra(ExtraEvent),
    }

    impl<C: ConnectionHandler> std::fmt::Debug for ToBehavior<C> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Inner(_) => write!(f, "ToBehavior::Inner"),
                Self::Extra(_) => write!(f, "ToBehavior::Extra"),
            }
        }
    }

    pub struct Protocol<T> {
        inner: T,
        peer: libp2p::PeerId,
        connection: usize,
    }

    impl<T> Protocol<T> {
        pub fn new(inner: T, peer: libp2p::PeerId, connection: ConnectionId) -> Self {
            Self { inner, peer, connection: unsafe { mem::transmute(connection) } }
        }
    }

    impl<T: UpgradeInfoSend> UpgradeInfoSend for Protocol<T> {
        type Info = T::Info;
        type InfoIter = T::InfoIter;

        fn protocol_info(&self) -> Self::InfoIter {
            self.inner.protocol_info()
        }
    }

    impl<T: InboundUpgradeSend> InboundUpgradeSend for Protocol<T> {
        type Error = T::Error;
        type Output = (T::Output, Self::Info);

        type Future = impl std::future::Future<Output = Result<Self::Output, Self::Error>>;

        fn upgrade_inbound(self, mut socket: libp2p::Stream, info: Self::Info) -> Self::Future {
            async move {
                let bytes = (
                    crate::INIT_TAG,
                    (PeerIdWrapper(self.peer), self.connection, info.as_ref().to_owned()),
                )
                    .to_bytes();
                assert_matches!(
                    libp2p::futures::poll!(std::future::poll_fn(
                        |cx| Pin::new(&mut socket).poll_write(cx, &bytes)
                    )),
                    Poll::Ready(Ok(_))
                );
                self.inner.upgrade_inbound(socket, info.clone()).await.map(|o| (o, info))
            }
        }
    }

    impl<T: OutboundUpgradeSend> OutboundUpgradeSend for Protocol<T> {
        type Error = T::Error;
        type Output = (T::Output, Self::Info);

        type Future = impl std::future::Future<Output = Result<Self::Output, Self::Error>>;

        fn upgrade_outbound(self, mut socket: libp2p::Stream, info: Self::Info) -> Self::Future {
            async move {
                let bytes = (
                    crate::INIT_TAG,
                    (PeerIdWrapper(self.peer), self.connection, info.as_ref().to_owned()),
                )
                    .to_bytes();
                assert_matches!(
                    libp2p::futures::poll!(std::future::poll_fn(
                        |cx| Pin::new(&mut socket).poll_write(cx, &bytes)
                    )),
                    Poll::Ready(Ok(_))
                );
                self.inner.upgrade_outbound(socket, info.clone()).await.map(|o| (o, info))
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerIdWrapper(pub PeerId);

impl<'a> Codec<'a> for PeerIdWrapper {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        let mh = Multihash::from(self.0);
        mh.write(WritableBuffer { buffer }).ok()?;
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let read = Multihash::<64>::read(buffer);
        read.ok().and_then(|mh| PeerId::from_multihash(mh).ok()).map(Self)
    }
}
