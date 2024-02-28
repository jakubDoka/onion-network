#![feature(extract_if)]
#![feature(let_chains)]
#![feature(iter_collect_into)]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(macro_metavar_expr)]
#![feature(if_let_guard)]

use {
    component_utils::{FindAndRemove, LinearMap},
    libp2p::{
        core::upgrade::ReadyUpgrade,
        swarm::{
            dial_opts::{DialOpts, PeerCondition},
            ConnectionHandler, ConnectionId, DialError, DialFailure, NetworkBehaviour,
            NotifyHandler, StreamUpgradeError, SubstreamProtocol,
        },
        PeerId, StreamProtocol,
    },
    std::{io, iter, task::Poll},
};

component_utils::decl_stream_protocol!(PROTOCOL_NAME = "streaming");

#[derive(Default)]
pub struct Behaviour {
    stream_requests: Vec<(PeerId, ConnectionId)>,
    dial_requests: Vec<(PeerId, Option<ConnectionId>, usize)>,
    connected_peers: LinearMap<PeerId, ConnectionId>,
    events: Vec<Event>,
}

impl Behaviour {
    pub fn is_resolving_stream_for(&self, peer: PeerId) -> bool {
        self.stream_requests.iter().any(|(p, ..)| *p == peer)
            || self.dial_requests.iter().any(|(p, ..)| *p == peer)
    }

    pub fn create_stream(&mut self, with: PeerId) {
        if let Some(&id) = self.connected_peers.get(&with) {
            self.stream_requests.push((with, id));
        } else if let Some((_, _, count)) = self.dial_requests.iter_mut().find(|(p, ..)| *p == with)
        {
            *count += 1;
        } else {
            self.dial_requests.push((with, None, 0));
        }
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
        _: PeerId,
        _: &libp2p::Multiaddr,
        _: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(Handler::new(|| PROTOCOL_NAME))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &libp2p::Multiaddr,
        _: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(Handler::new(|| PROTOCOL_NAME))
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        match event {
            libp2p::swarm::FromSwarm::DialFailure(DialFailure {
                error,
                peer_id,
                connection_id,
                ..
            }) if let Some(&(peer, _, count)) = self
                .dial_requests
                .iter()
                .find(|(p, c, _)| *c == Some(connection_id) || Some(*p) == peer_id) =>
            {
                if let DialError::DialPeerConditionFalse(_) = error {
                } else {
                    for _ in 0..=count {
                        self.events.push(Event::OutgoingStream(
                            peer,
                            Err(StreamUpgradeError::Io(io::Error::other(error.to_string()))),
                        ));
                    }
                }
            }
            libp2p::swarm::FromSwarm::ConnectionClosed(c) => {
                self.connected_peers.remove(&c.peer_id);

                let count = self.stream_requests.extract_if(|(p, ..)| *p == c.peer_id).count();
                if let Some(additional) = count.checked_sub(1) {
                    if let Some((_, _, count)) =
                        self.dial_requests.iter_mut().find(|(p, ..)| *p == c.peer_id)
                    {
                        *count += additional;
                    } else {
                        self.dial_requests.push((c.peer_id, None, additional));
                    }
                }
            }
            libp2p::swarm::FromSwarm::ConnectionEstablished(c) => {
                if let Some((.., count)) =
                    self.dial_requests.find_and_remove(|(p, ..)| *p == c.peer_id)
                {
                    self.stream_requests
                        .extend(iter::repeat((c.peer_id, c.connection_id)).take(count + 1));
                }
                self.connected_peers.insert(c.peer_id, c.connection_id);
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
        _: &mut std::task::Context<'_>,
    ) -> Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        self.dial_requests.retain(|(p, _, count)| {
            if let Some(id) = self.connected_peers.get(p) {
                for _ in 0..=*count {
                    self.stream_requests.push((*p, *id));
                }
                false
            } else {
                true
            }
        });

        if let Some((peer, id, _)) = self.dial_requests.iter_mut().find(|(_, c, _)| c.is_none()) {
            return Poll::Ready(libp2p::swarm::ToSwarm::Dial { opts: new_dial_opts(*peer, id) });
        }

        if let Some((peer_id, conn_id)) = self.stream_requests.pop() {
            return Poll::Ready(libp2p::swarm::ToSwarm::NotifyHandler {
                peer_id,
                handler: NotifyHandler::One(conn_id),
                event: (),
            });
        }

        if let Some(event) = self.events.pop() {
            return Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
#[allow(clippy::type_complexity)]
pub enum Event {
    IncomingStream(PeerId, libp2p::Stream),
    OutgoingStream(PeerId, Result<libp2p::Stream, Error>),
    SearchRequest(PeerId),
}

pub type Error = StreamUpgradeError<void::Void>;

pub struct Handler {
    requested: usize,
    stream: Vec<HEvent>,
    waker: Option<std::task::Waker>,
    proto: fn() -> StreamProtocol,
}

impl Handler {
    pub fn new(proto: fn() -> StreamProtocol) -> Self {
        Self { requested: 0, stream: Vec::new(), waker: None, proto }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = ();
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
        component_utils::set_waker(&mut self.waker, cx.waker());
        if let Some(stream) = self.stream.pop() {
            return Poll::Ready(libp2p::swarm::ConnectionHandlerEvent::NotifyBehaviour(stream));
        }

        if let Some(next_requested) = self.requested.checked_sub(1) {
            self.requested = next_requested;
            return Poll::Ready(libp2p::swarm::ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new((self.proto)()), ()),
            });
        }
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, (): Self::FromBehaviour) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
        self.requested += 1;
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
        dht::Route,
        libp2p::{
            futures::{stream::FuturesUnordered, StreamExt},
            identity::{Keypair, PublicKey},
            multiaddr::Protocol,
            Multiaddr, Transport,
        },
        std::{net::Ipv4Addr, time::Duration},
    };

    #[derive(NetworkBehaviour, Default)]
    struct TestBehatiour {
        rpc: Behaviour,
        dht: dht::Behaviour,
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_random_rpc_calls() {
        env_logger::init();

        let pks =
            (0..10).map(|_| libp2p::identity::ed25519::Keypair::generate()).collect::<Vec<_>>();
        let public_keys = pks.iter().map(|kp| kp.public()).collect::<Vec<_>>();
        let peer_ids =
            pks.iter().map(|kp| PublicKey::from(kp.public()).to_peer_id()).collect::<Vec<_>>();
        let servers = pks.into_iter().map(Keypair::from).enumerate().map(|(i, kp)| {
            let beh = TestBehatiour::default();
            let transport = libp2p::tcp::tokio::Transport::new(libp2p::tcp::Config::default())
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(libp2p::noise::Config::new(&kp).unwrap())
                .multiplex(libp2p::yamux::Config::default())
                .boxed();
            let mut swarm = libp2p::Swarm::new(
                transport,
                beh,
                kp.public().to_peer_id(),
                libp2p::swarm::Config::with_tokio_executor()
                    .with_idle_connection_timeout(Duration::from_secs(10)),
            );

            swarm
                .listen_on(
                    Multiaddr::empty()
                        .with(Protocol::Ip4(Ipv4Addr::LOCALHOST))
                        .with(Protocol::Tcp(3000 + i as u16)),
                )
                .unwrap();

            for (j, pk) in public_keys.iter().enumerate() {
                swarm.behaviour_mut().dht.table.insert(Route::new(
                    pk.clone(),
                    Multiaddr::empty()
                        .with(Protocol::Ip4(Ipv4Addr::LOCALHOST))
                        .with(Protocol::Tcp(3000 + j as u16)),
                ));
            }

            swarm
        });

        async fn run_server(mut swarm: libp2p::Swarm<TestBehatiour>, mut all_peers: Vec<PeerId>) {
            all_peers.retain(|p| p != swarm.local_peer_id());
            let max_pending_requests = 10;
            let mut pending_request_count = 0;
            let mut iteration = 0;
            let mut total_requests = 0;
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
                        Event::OutgoingStream(_, e),
                    )) => {
                        e.unwrap();
                        pending_request_count -= 1;
                    }
                    libp2p::swarm::SwarmEvent::Behaviour(TestBehatiourEvent::Rpc(
                        Event::IncomingStream(..),
                    )) => {
                        total_requests += 1;
                    }
                    e => {
                        log::info!("event: {:?}", e);
                    }
                }

                iteration += 1;
            }
        }

        servers
            .map(|s| run_server(s, peer_ids.clone()))
            .collect::<FuturesUnordered<_>>()
            .next()
            .await;
    }
}
