use {
    crypto::{enc, TransmutationCircle},
    futures::{AsyncReadExt, Future},
    libp2p::{
        core::{upgrade::DeniedUpgrade, UpgradeInfo},
        swarm::{ConnectionHandler, NetworkBehaviour, SubstreamProtocol},
        OutboundUpgrade, PeerId, StreamProtocol,
    },
    std::{collections::HashMap, convert::Infallible, io, iter, mem},
};

component_utils::decl_stream_protocol!(KEY_SHARE_PROTOCOL = "ksr");

#[derive(Default)]
pub struct Behaviour {
    pub keys: HashMap<PeerId, enc::PublicKey>,
    new_keys: Vec<(PeerId, enc::PublicKey)>,
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = (PeerId, enc::PublicKey);

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: PeerId,
        _local_addr: &libp2p::Multiaddr,
        _remote_addr: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(Handler { connect: false, key: None })
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        _addr: &libp2p::Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(Handler { connect: !self.keys.contains_key(&peer), key: None })
    }

    fn on_swarm_event(&mut self, _event: libp2p::swarm::FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.keys.insert(peer_id, event);
        self.new_keys.push((peer_id, event));
    }

    fn poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        if let Some((peer_id, key)) = self.new_keys.pop() {
            std::task::Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent((peer_id, key)))
        } else {
            std::task::Poll::Pending
        }
    }
}

pub struct Handler {
    connect: bool,
    key: Option<enc::PublicKey>,
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Infallible;
    type InboundOpenInfo = ();
    type InboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = ();
    type OutboundProtocol = Protocol;
    type ToBehaviour = enc::PublicKey;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn poll(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<
        libp2p::swarm::ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
        >,
    > {
        if mem::take(&mut self.connect) {
            std::task::Poll::Ready(
                libp2p::swarm::ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(Protocol, ()),
                },
            )
        } else if let Some(key) = self.key.take() {
            std::task::Poll::Ready(libp2p::swarm::ConnectionHandlerEvent::NotifyBehaviour(key))
        } else {
            std::task::Poll::Pending
        }
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {}
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
        if let libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(o) = event {
            self.key = Some(o.protocol);
        }
    }
}

pub struct Protocol;

impl UpgradeInfo for Protocol {
    type Info = StreamProtocol;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(KEY_SHARE_PROTOCOL)
    }
}

impl OutboundUpgrade<libp2p::Stream> for Protocol {
    type Error = io::Error;
    type Output = enc::PublicKey;

    type Future = impl Future<Output = Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, mut socket: libp2p::Stream, _: Self::Info) -> Self::Future {
        async move {
            let mut key = [0; std::mem::size_of::<enc::PublicKey>()];
            socket.read_exact(&mut key).await?;
            Ok(enc::PublicKey::from_bytes(key))
        }
    }
}
