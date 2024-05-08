#![feature(let_chains)]
use {
    libp2p::{
        futures::{stream::FuturesUnordered, StreamExt},
        swarm::NetworkBehaviour,
    },
    std::{
        collections::HashSet, future::Future, net::IpAddr, num::NonZeroUsize, pin::Pin, task::Waker,
    },
};

pub type NodeId = [u8; 32];
pub type Validity = u64;

const ALLOWED_CACHE_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(3000) };
const BANNED_CACHE_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(1000) };

fn is_valid(validity: Validity) -> bool {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(false, |now| now.as_secs() < validity)
}

fn find_ip_addr(addr: &libp2p::Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|addr| match addr {
        libp2p::core::multiaddr::Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
        libp2p::core::multiaddr::Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
        _ => None,
    })
}

pub trait Auth: 'static + Send + Sync {
    type Future: Future<Output = Result<Validity, Validity>>;

    fn should_validate(&mut self, remote: &libp2p::Multiaddr, local: &libp2p::Multiaddr) -> bool;
    fn translate_peer_id(&mut self, pid: libp2p::PeerId) -> Option<NodeId>;
    fn translate_node_id(&mut self, node_id: NodeId) -> libp2p::PeerId;
    fn auth(&mut self, node_id: NodeId) -> Self::Future;
}

struct AuthFut<F>(NodeId, IpAddr, F);

impl<F: Future> Future for AuthFut<F> {
    type Output = (NodeId, IpAddr, F::Output);

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        unsafe {
            let s = self.get_unchecked_mut();
            Pin::new_unchecked(&mut s.2).poll(cx).map(|r| (s.0, s.1, r))
        }
    }
}

// TODO: Add ip banning
pub struct Behaviour<F: Auth> {
    auth: F,
    allowed: lru::LruCache<NodeId, Validity>,
    banned: lru::LruCache<IpAddr, Validity>,
    progressing_auths: HashSet<IpAddr>,
    auths: FuturesUnordered<AuthFut<<F as Auth>::Future>>,
    auth_waker: Option<Waker>,
}

impl<F: Auth> Behaviour<F> {
    pub fn new(filter: F) -> Self {
        Behaviour {
            auth: filter,
            allowed: lru::LruCache::new(ALLOWED_CACHE_SIZE),
            banned: lru::LruCache::new(BANNED_CACHE_SIZE),
            progressing_auths: HashSet::new(),
            auths: FuturesUnordered::new(),
            auth_waker: None,
        }
    }
}

impl<F: Auth> NetworkBehaviour for Behaviour<F> {
    type ConnectionHandler = libp2p::swarm::dummy::ConnectionHandler;
    type ToSwarm = void::Void;

    fn handle_pending_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        local_addr: &libp2p::Multiaddr,
        remote_addr: &libp2p::Multiaddr,
    ) -> Result<(), libp2p::swarm::ConnectionDenied> {
        if !self.auth.should_validate(remote_addr, local_addr) {
            return Ok(());
        }

        let Some(ip) = find_ip_addr(&remote_addr) else {
            return Err(libp2p::swarm::ConnectionDenied::new(ExpectedIpAddr));
        };

        if self.progressing_auths.contains(&ip) {
            return Err(libp2p::swarm::ConnectionDenied::new(AlreadyAuthenticating));
        }

        if let Some(&validity) = self.banned.get(&ip) {
            if is_valid(validity) {
                return Err(libp2p::swarm::ConnectionDenied::new(Banned));
            }
            self.banned.pop(&ip);
        }

        Ok(())
    }

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: libp2p::PeerId,
        local_addr: &libp2p::Multiaddr,
        remote_addr: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        if !self.auth.should_validate(remote_addr, local_addr) {
            return Ok(libp2p::swarm::dummy::ConnectionHandler);
        }

        let Some(ip) = find_ip_addr(&remote_addr) else {
            return Err(libp2p::swarm::ConnectionDenied::new(ExpectedIpAddr));
        };

        let Some(node_id) = self.auth.translate_peer_id(peer) else {
            return Err(libp2p::swarm::ConnectionDenied::new(InvalidPeerId));
        };

        if let Some(&validity) = self.allowed.get(&node_id) {
            if is_valid(validity) {
                return Ok(libp2p::swarm::dummy::ConnectionHandler);
            }
            self.allowed.pop(&node_id);
        }

        self.progressing_auths.insert(ip);
        self.auths.push(AuthFut(node_id, ip, self.auth.auth(node_id)));
        if let Some(waker) = self.auth_waker.take() {
            waker.wake();
        }

        Ok(libp2p::swarm::dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: libp2p::PeerId,
        _addr: &libp2p::Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(libp2p::swarm::dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, _event: libp2p::swarm::FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        _peer_id: libp2p::PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        void::unreachable(event)
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        while let std::task::Poll::Ready(Some((node, ip, result))) = self.auths.poll_next_unpin(cx)
        {
            self.progressing_auths.remove(&ip);
            match result {
                Ok(validity) => _ = self.allowed.put(node, validity),
                Err(validity) => {
                    log::warn!("Banning node {:?}", node);
                    if !ip.is_loopback() {
                        self.banned.put(ip, validity);
                    }
                    return std::task::Poll::Ready(libp2p::swarm::ToSwarm::CloseConnection {
                        peer_id: self.auth.translate_node_id(node),
                        connection: libp2p::swarm::CloseConnection::All,
                    });
                }
            }
        }

        component_utils::set_waker(&mut self.auth_waker, cx.waker());
        std::task::Poll::Pending
    }
}

macro_rules! decl_error {
    ($($name:ident)*) => {$(
        #[derive(Debug)]
        struct $name;

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, stringify!($name))
            }
        }

        impl std::error::Error for $name {}
    )*}
}

decl_error!(ExpectedIpAddr AlreadyAuthenticating InvalidPeerId Banned);
