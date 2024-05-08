#![feature(impl_trait_in_assoc_type)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![feature(assert_matches)]
#![feature(macro_metavar_expr)]
#![feature(slice_take)]

pub use impls::{channel, new, Behaviour, EventReceiver, EventSender};
use {
    codec::{Buffer, Codec, Decode, Encode, WritableBuffer},
    libp2p::{
        multihash::Multihash,
        swarm::{ConnectionId, NetworkBehaviour},
        PeerId, StreamProtocol,
    },
};

#[derive(Codec, Clone)]
pub struct PacketMeta {
    #[codec(with = peer_id_codec)]
    peer: PeerId,
    #[codec(with = connection_id_codec)]
    connection: ConnectionId,
    #[codec(with = stream_protocol_codec)]
    protocol: StreamProtocol,
}

pub trait World: 'static {
    fn handle_update(&mut self, peer: PeerId, update: Update);
}

pub trait BuildWrapped: NetworkBehaviour + Sized {
    fn include_in_vis(self, ev: EventSender) -> Behaviour<Self>;
}

impl<T: NetworkBehaviour> BuildWrapped for T {
    fn include_in_vis(self, ev: EventSender) -> Behaviour<Self> {
        new(self, ev)
    }
}

#[cfg(not(feature = "disabled"))]
const INIT_TAG: [u8; 32] = [0xff; 32];

#[derive(Codec, Clone, Debug)]
pub struct Update {
    pub event: Event,
    #[codec(with = peer_id_codec)]
    pub peer: PeerId,
    #[codec(with = connection_id_codec)]
    pub connection: ConnectionId,
    #[codec(with = stream_protocol_codec)]
    pub protocol: StreamProtocol,
}

#[derive(Codec, Clone, Debug)]
pub enum Event {
    Stream,
    Packet,
    Closed,
}

#[cfg(feature = "disabled")]
pub mod collector {
    use {
        crate::World,
        libp2p::{swarm::NetworkBehaviour, PeerId},
        std::{convert::Infallible, marker::PhantomData},
    };

    pub fn new<W: World>(_: W) -> Behaviour<W> {
        Behaviour { world: PhantomData }
    }

    pub struct Behaviour<T> {
        world: PhantomData<T>,
    }

    impl<W: World> Behaviour<W> {
        pub fn new(_: PeerId, _: W) -> Self {
            Self { world: PhantomData }
        }

        pub fn world_mut(&mut self) -> &mut W {
            unimplemented!()
        }

        pub fn add_peer(&mut self, _: PeerId) {
            unimplemented!()
        }
    }

    impl<W: World> NetworkBehaviour for Behaviour<W> {
        type ConnectionHandler = libp2p::swarm::dummy::ConnectionHandler;
        type ToSwarm = Infallible;

        fn handle_established_inbound_connection(
            &mut self,
            _: libp2p::swarm::ConnectionId,
            _: PeerId,
            _: &libp2p::Multiaddr,
            _: &libp2p::Multiaddr,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            Ok(libp2p::swarm::dummy::ConnectionHandler)
        }

        fn handle_established_outbound_connection(
            &mut self,
            _connection_id: libp2p::swarm::ConnectionId,
            _peer: PeerId,
            _addr: &libp2p::Multiaddr,
            _role_override: libp2p::core::Endpoint,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            Ok(libp2p::swarm::dummy::ConnectionHandler)
        }

        fn on_swarm_event(&mut self, _event: libp2p::swarm::FromSwarm) {}

        fn on_connection_handler_event(
            &mut self,
            _peer_id: PeerId,
            _connection_id: libp2p::swarm::ConnectionId,
            _event: libp2p::swarm::THandlerOutEvent<Self>,
        ) {
        }

        fn poll(
            &mut self,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<
            libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>,
        > {
            std::task::Poll::Pending
        }
    }
}

#[cfg(not(feature = "disabled"))]
pub mod collector {

    use {
        crate::{Update, World},
        codec::Decode,
        libp2p::{
            futures::{stream::SelectAll, AsyncReadExt, StreamExt},
            swarm::NetworkBehaviour,
            PeerId,
        },
        std::convert::Infallible,
    };

    pub fn new<W: World>(world: W) -> Behaviour<W> {
        Behaviour::new(world)
    }

    mod foo {
        use {super::*, std::io};

        pub type UpdateStream =
            impl libp2p::futures::Stream<Item = (io::Result<Update>, PeerId)> + Unpin;

        pub fn update_stream(stream: libp2p::Stream, peer_id: PeerId) -> UpdateStream {
            Box::pin(libp2p::futures::stream::unfold(Some(stream), move |mut stream| async move {
                let mut buffer = [0u8; 1024];
                let mut len = [0u8; 2];
                let str = stream.as_mut()?;
                let res = async {
                    str.read_exact(&mut len).await?;
                    let len = u16::from_be_bytes(len) as usize;
                    debug_assert!(len != 0);
                    str.read_exact(&mut buffer[..len]).await?;
                    Update::decode_exact(&buffer[..len]).ok_or(io::ErrorKind::InvalidData.into())
                };
                Some(((res.await.inspect_err(|_| _ = stream.take()), peer_id), stream))
            }))
        }
    }

    pub struct Behaviour<W: World> {
        world: W,
        listeners: SelectAll<foo::UpdateStream>,
        pending_connections: Vec<PeerId>,
    }

    impl<W: World> Behaviour<W> {
        pub fn new(world: W) -> Self {
            Self { world, listeners: Default::default(), pending_connections: Default::default() }
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
            Ok(crate::report::Handler::default())
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
            self.listeners.push(foo::update_stream(event, peer_id));
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
                let Some((update, peer)) =
                    libp2p::futures::ready!(self.listeners.poll_next_unpin(cx))
                else {
                    return std::task::Poll::Pending;
                };

                let Ok(update) =
                    update.inspect_err(|e| log::error!("Error while decoding update: {}", e))
                else {
                    continue;
                };

                self.world.handle_update(peer, update);
            }
        }
    }
}

#[cfg(feature = "disabled")]
pub mod muxer {
    pub fn new<T>(inner: T, _: crate::EventSender) -> T {
        inner
    }
}

#[cfg(not(feature = "disabled"))]
pub mod muxer {
    use {
        crate::{Event, PacketMeta, Update},
        libp2p::{
            core::StreamMuxer,
            futures::{AsyncRead, AsyncWrite, SinkExt},
        },
        std::pin::Pin,
    };

    pub fn new<T>(inner: T, sender: crate::EventSender) -> Muxer<T> {
        Muxer::new(inner, sender)
    }

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
                use codec::Decode;
                if let Some((sig, meta)) = <([u8; 32], PacketMeta)>::decode(&mut &*buf) {
                    if sig == crate::INIT_TAG {
                        self.meta = Some(meta);
                        return std::task::Poll::Ready(Ok(buf.len()));
                    }
                }
            }

            if !self.sending
                && buf.len() > 4
                && libp2p::futures::ready!(self.sender.events.poll_ready(cx)).is_ok()
            {
                if let Some(meta) = self.meta.clone() {
                    let _ = self.sender.events.start_send(Update {
                        event: Event::Packet,
                        peer: meta.peer,
                        connection: meta.connection,
                        protocol: meta.protocol,
                    });
                }
            }
            _ = self.sender.events.poll_flush_unpin(cx);

            self.sending = true;
            let res = libp2p::futures::ready!(Pin::new(&mut self.inner).poll_write(cx, buf));
            self.sending = false;
            std::task::Poll::Ready(res)
        }

        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::ready!(self.sender.events.poll_flush_unpin(cx)).unwrap();
            Pin::new(&mut self.inner).poll_flush(cx)
        }

        fn poll_close(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_close(cx)
        }
    }

    impl<T> Drop for Substream<T> {
        fn drop(&mut self) {
            if let Some(meta) = self.meta.take() {
                self.sender
                    .events
                    .try_send(Update {
                        event: Event::Closed,
                        peer: meta.peer,
                        connection: meta.connection,
                        protocol: meta.protocol,
                    })
                    .unwrap();
            }
        }
    }
}

#[cfg(feature = "disabled")]
pub mod report {
    use crate::EventReceiver;
    pub type Behaviour = libp2p::swarm::dummy::Behaviour;
    pub type Update<'a> = ();

    pub fn new(_: EventReceiver) -> Behaviour {
        libp2p::swarm::dummy::Behaviour
    }
}

#[cfg(not(feature = "disabled"))]
pub mod report {
    use {
        crate::{Event, EventReceiver, Update},
        codec::Encode,
        libp2p::{
            core::upgrade::ReadyUpgrade,
            futures::{channel::mpsc, stream::FuturesUnordered, AsyncWriteExt, StreamExt},
            swarm::{ConnectionHandler, ConnectionId, NetworkBehaviour},
            PeerId, StreamProtocol,
        },
        std::{
            collections::{hash_map::Entry, HashMap},
            convert::Infallible,
        },
    };

    #[must_use]
    pub fn new(recv: EventReceiver) -> Behaviour {
        Behaviour {
            listeners: Default::default(),
            topology: Default::default(),
            recv,
            listener_tasks: Default::default(),
        }
    }

    type StreamTask = impl std::future::Future<Output = ()>;
    type Topology = HashMap<PeerId, HashMap<ConnectionId, HashMap<StreamProtocol, isize>>>;

    fn update_topology(
        topology: &mut Topology,
        peer: PeerId,
        conn: ConnectionId,
        proto: StreamProtocol,
        value: isize,
    ) -> bool {
        let counter = topology
            .entry(peer)
            .or_default()
            .entry(conn)
            .or_default()
            .entry(proto.clone())
            .or_default();

        let was_there = *counter > 0;
        *counter += value;
        let is_there = *counter > 0;

        if *counter == 0
            && let Entry::Occupied(mut peer) = topology.entry(peer)
            && let Entry::Occupied(mut connection) = peer.get_mut().entry(conn)
            && let Entry::Occupied(proto) = connection.get_mut().entry(proto)
        {
            proto.remove();
            if connection.get().is_empty() {
                connection.remove();
            }
            if peer.get().is_empty() {
                peer.remove();
            }
        }

        was_there != is_there
    }

    fn stream_task(
        mut stream: libp2p::Stream,
        mut updates: mpsc::Receiver<Update>,
        topology: Topology,
    ) -> StreamTask {
        async move {
            let mut buf = [0u8; 1024];
            for (peer, conns) in topology {
                for (connection, streams) in conns {
                    for (protocol, count) in streams {
                        debug_assert!(count > 0);
                        let meta =
                            crate::Update { peer, connection, event: Event::Stream, protocol };
                        let len = meta.encoded_len();
                        meta.encode(&mut &mut buf[2..len + 2]).unwrap();
                        buf[..2].copy_from_slice(&(len as u16).to_be_bytes());
                        if let Err(e) = stream.write_all(&buf[..len + 2]).await {
                            log::error!("Error while writing update: {}", e);
                            break;
                        }
                    }
                }
            }

            while let Some(update) = updates.next().await {
                let len = update.encoded_len();
                update.encode(&mut &mut buf[2..len + 2]).unwrap();
                buf[..2].copy_from_slice(&(len as u16).to_be_bytes());
                if let Err(e) = stream.write_all(&buf[..len + 2]).await {
                    log::error!("Error while writing update: {}", e);
                    break;
                }
            }
        }
    }

    struct UpdateStream {
        peer: PeerId,
        sender: mpsc::Sender<Update>,
    }

    pub struct Behaviour {
        listeners: Vec<UpdateStream>,
        listener_tasks: FuturesUnordered<StreamTask>,
        topology: Topology,
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

        fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
            match event {
                libp2p::swarm::FromSwarm::ConnectionClosed(c) => {
                    self.listeners.retain(|l| l.peer != c.peer_id);
                }
                _ => {}
            }
        }

        fn on_connection_handler_event(
            &mut self,
            peer_id: PeerId,
            _connection_id: libp2p::swarm::ConnectionId,
            event: libp2p::swarm::THandlerOutEvent<Self>,
        ) {
            if self.listeners.iter().any(|l| l.peer == peer_id) {
                return;
            }
            let (sender, receiver) = mpsc::channel(100);
            let stream = UpdateStream { peer: peer_id, sender };
            self.listener_tasks.push(stream_task(event, receiver, self.topology.clone()));

            self.listeners.push(stream);
        }

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<
            libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>,
        > {
            while let std::task::Poll::Ready(Some(mut update)) =
                self.recv.events.poll_next_unpin(cx)
            {
                let should_update = match &update.event {
                    Event::Stream => update_topology(
                        &mut self.topology,
                        update.peer,
                        update.connection,
                        update.protocol.clone(),
                        1,
                    ),
                    Event::Packet => true,
                    Event::Closed => update_topology(
                        &mut self.topology,
                        update.peer,
                        update.connection,
                        update.protocol.clone(),
                        -1,
                    ),
                };

                if should_update {
                    self.listeners.retain_mut(|l| {
                        _ = l.sender.try_send(update.clone()).is_ok();
                        true
                    });

                    if let Some(peer) = self.topology.get(&update.peer)
                        && let Some(connection) = peer.get(&update.connection)
                        && connection.contains_key(&update.protocol)
                    {
                    } else {
                        update.event = Event::Closed;
                        self.listeners.retain_mut(|l| {
                            _ = l.sender.try_send(update.clone()).is_ok();
                            true
                        });
                    }
                }
            }

            while let std::task::Poll::Ready(Some(_)) = self.listener_tasks.poll_next_unpin(cx) {}

            std::task::Poll::Pending
        }
    }

    #[derive(Default)]
    pub struct Handler {
        connected: Option<libp2p::Stream>,
        connect: bool,
    }

    impl Handler {
        pub fn connecting() -> Self {
            Self { connected: None, connect: true }
        }
    }

    impl ConnectionHandler for Handler {
        type FromBehaviour = Infallible;
        type InboundOpenInfo = ();
        type InboundProtocol = ReadyUpgrade<StreamProtocol>;
        type OutboundOpenInfo = ();
        type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
        type ToBehaviour = libp2p::Stream;

        fn listen_protocol(
            &self,
        ) -> libp2p::swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo>
        {
            libp2p::swarm::SubstreamProtocol::new(ReadyUpgrade::new(ROUTING_PROTOCOL), ())
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
                        protocol: libp2p::swarm::SubstreamProtocol::new(
                            ReadyUpgrade::new(ROUTING_PROTOCOL),
                            (),
                        ),
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
}

#[cfg(feature = "disabled")]
mod impls {
    pub type Behaviour<T> = T;

    #[derive(Clone)]
    pub struct EventSender;
    #[derive(Clone)]
    pub struct EventReceiver;

    pub fn new<T>(inner: T, _: EventSender) -> T {
        inner
    }
    pub fn channel() -> (EventSender, EventReceiver) {
        (EventSender, EventReceiver)
    }
}

#[cfg(not(feature = "disabled"))]
mod impls {
    use {
        codec::Encode,
        libp2p::{
            futures::AsyncWrite,
            swarm::{
                handler::{
                    DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
                    InboundUpgradeSend, ListenUpgradeError, UpgradeInfoSend,
                },
                ConnectionHandler, ConnectionId, NetworkBehaviour,
            },
            StreamProtocol,
        },
        std::{
            assert_matches::assert_matches,
            ops::{Deref, DerefMut},
            pin::Pin,
            task::Poll,
        },
    };

    #[derive(Clone)]
    pub struct EventSender {
        pub(crate) events: libp2p::futures::channel::mpsc::Sender<crate::Update>,
    }

    pub struct EventReceiver {
        pub(crate) events: libp2p::futures::channel::mpsc::Receiver<crate::Update>,
    }

    #[must_use]
    pub fn channel() -> (EventSender, EventReceiver) {
        let (events_sender, events_receiver) = libp2p::futures::channel::mpsc::channel(300);
        (EventSender { events: events_sender }, EventReceiver { events: events_receiver })
    }

    pub fn new<T: NetworkBehaviour>(inner: T, sender: EventSender) -> Behaviour<T> {
        Behaviour::new(inner, sender)
    }

    pub struct Behaviour<T: NetworkBehaviour> {
        inner: T,
        sender: EventSender,
    }

    impl<T: NetworkBehaviour> Behaviour<T> {
        fn new(inner: T, sender: EventSender) -> Self {
            Self { inner, sender }
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
                .map(|h| Handler::new(h, peer, connection_id, self.sender.clone()))
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
                .map(|h| Handler::new(h, peer, connection_id, self.sender.clone()))
        }

        fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
            self.inner.on_swarm_event(event);
        }

        fn on_connection_handler_event(
            &mut self,
            peer_id: libp2p::PeerId,
            connection_id: libp2p::swarm::ConnectionId,
            event: libp2p::swarm::THandlerOutEvent<Self>,
        ) {
            self.inner.on_connection_handler_event(peer_id, connection_id, event);
        }

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<
            libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>,
        > {
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
        peer: libp2p::PeerId,
        connection: ConnectionId,
        events: EventSender,
    }

    impl<T: ConnectionHandler> Handler<T> {
        pub fn new(
            inner: T,
            peer: libp2p::PeerId,
            connection: ConnectionId,
            events: EventSender,
        ) -> Self {
            Self { inner, peer, connection, events }
        }
    }

    impl<T: ConnectionHandler> ConnectionHandler for Handler<T> {
        type FromBehaviour = T::FromBehaviour;
        type InboundOpenInfo = T::InboundOpenInfo;
        type InboundProtocol = Protocol<T::InboundProtocol>;
        type OutboundOpenInfo = T::OutboundOpenInfo;
        type OutboundProtocol = T::OutboundProtocol;
        type ToBehaviour = T::ToBehaviour;

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
            self.inner.poll(cx)
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
                    self.events
                        .events
                        .try_send(crate::Update {
                            event: crate::Event::Stream,
                            peer: self.peer,
                            connection: self.connection,
                            protocol: StreamProtocol::try_from_owned(
                                i.protocol.1.as_ref().to_string(),
                            )
                            .unwrap(),
                        })
                        .unwrap();
                    libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedInbound(
                        FullyNegotiatedInbound { protocol: i.protocol.0, info: i.info },
                    )
                }
                CE::FullyNegotiatedOutbound(o) => {
                    CE::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                        protocol: o.protocol,
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
            self.inner.poll_close(cx)
        }
    }

    pub struct Protocol<T> {
        inner: T,
        peer: libp2p::PeerId,
        connection: ConnectionId,
    }

    impl<T> Protocol<T> {
        pub fn new(inner: T, peer: libp2p::PeerId, connection: ConnectionId) -> Self {
            Self { inner, peer, connection }
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
                let bytes = (crate::INIT_TAG, crate::PacketMeta {
                    peer: self.peer,
                    connection: self.connection,
                    protocol: StreamProtocol::try_from_owned(info.as_ref().to_string()).unwrap(),
                })
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
}

mod peer_id_codec {
    use super::*;

    pub fn decode(buffer: &mut &[u8]) -> Option<PeerId> {
        let read = Multihash::<64>::read(buffer);
        read.ok().and_then(|mh| PeerId::from_multihash(mh).ok())
    }

    pub fn encode(peer: &PeerId, buffer: &mut impl Buffer) -> Option<()> {
        let mh = Multihash::from(*peer);
        mh.write(WritableBuffer { buffer }).ok().map(drop)
    }
}

mod connection_id_codec {
    use {super::*, std::mem};

    pub fn decode(buffer: &mut &[u8]) -> Option<ConnectionId> {
        usize::decode(buffer).map(|i| unsafe { mem::transmute(i) })
    }

    pub fn encode(connection: &ConnectionId, buffer: &mut impl Buffer) -> Option<()> {
        let i = unsafe { mem::transmute::<ConnectionId, usize>(*connection) };
        i.encode(buffer)
    }
}

mod stream_protocol_codec {
    use super::*;

    pub fn decode(buffer: &mut &[u8]) -> Option<StreamProtocol> {
        let str = String::decode(buffer)?;
        StreamProtocol::try_from_owned(str).ok()
    }

    pub fn encode(protocol: &StreamProtocol, buffer: &mut impl Buffer) -> Option<()> {
        protocol.as_ref().encode(buffer)
    }
}
