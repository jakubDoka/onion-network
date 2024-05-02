use {
    codec::Decode,
    crypto::enc,
    futures::{AsyncReadExt, AsyncWriteExt, Future},
    libp2p::{
        core::UpgradeInfo,
        swarm::{ConnectionHandler, NetworkBehaviour, SubstreamProtocol},
        InboundUpgrade, OutboundUpgrade, PeerId, StreamProtocol,
    },
    std::{collections::HashMap, convert::Infallible, io, iter},
};

component_utils::decl_stream_protocol!(KEY_SHARE_PROTOCOL = "ksr");

#[derive(Default)]
pub struct Behaviour {
    local_key: Option<enc::PublicKey>,
    pub keys: HashMap<PeerId, enc::PublicKey>,
    new_keys: Vec<(PeerId, enc::PublicKey)>,
}

impl Behaviour {
    pub fn new(local_key: enc::PublicKey) -> Self {
        Self { local_key: Some(local_key), ..Default::default() }
    }
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
        Ok(match self.local_key {
            Some(key) => Handler::Sharing(key),
            None => Handler::Done,
        })
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        _addr: &libp2p::Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(match self.keys.contains_key(&peer) {
            true => Handler::Done,
            false => Handler::Fesh,
        })
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

pub enum Handler {
    Fesh,
    Waiting,
    Received(enc::PublicKey),
    Sharing(enc::PublicKey),
    Done,
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Infallible;
    type InboundOpenInfo = ();
    type InboundProtocol = InboundProtocol;
    type OutboundOpenInfo = ();
    type OutboundProtocol = Protocol;
    type ToBehaviour = enc::PublicKey;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(
            InboundProtocol {
                key: match self {
                    Handler::Sharing(key) => Some(*key),
                    _ => None,
                },
            },
            (),
        )
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
        match self {
            Handler::Fesh => {
                *self = Handler::Waiting;
                std::task::Poll::Ready(
                    libp2p::swarm::ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(Protocol, ()),
                    },
                )
            }
            Handler::Received(key) => {
                let key = *key;
                *self = Handler::Done;
                std::task::Poll::Ready(libp2p::swarm::ConnectionHandlerEvent::NotifyBehaviour(key))
            }
            _ => std::task::Poll::Pending,
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
            *self = Handler::Received(o.protocol);
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
            enc::PublicKey::decode_exact(&key).ok_or(io::ErrorKind::InvalidData.into())
        }
    }
}

pub struct InboundProtocol {
    key: Option<enc::PublicKey>,
}

impl UpgradeInfo for InboundProtocol {
    type Info = StreamProtocol;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(KEY_SHARE_PROTOCOL)
    }
}

impl InboundUpgrade<libp2p::Stream> for InboundProtocol {
    type Error = io::Error;
    type Output = ();

    type Future = impl Future<Output = Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, mut socket: libp2p::Stream, _: Self::Info) -> Self::Future {
        async move {
            let Some(key) = self.key else { return Ok(()) };
            socket.write_all(key.as_ref()).await?;
            Ok(())
        }
    }
}
