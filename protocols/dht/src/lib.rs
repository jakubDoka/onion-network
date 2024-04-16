#![feature(trait_alias)]
#![feature(let_chains)]
pub use primitive_types::U256;
use {
    libp2p::{swarm::NetworkBehaviour, Multiaddr, PeerId},
    opfusk::{PeerIdExt, ToPeerId},
    std::convert::Infallible,
};

pub type Filter = fn(
    &mut RoutingTable,
    PeerId,
    &Multiaddr,
    &Multiaddr,
) -> Result<(), libp2p::swarm::ConnectionDenied>;

#[derive(Clone)]
pub struct Behaviour {
    pub table: RoutingTable,
    filter: Filter,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self::new(|_, _, _, _| Ok(()))
    }
}

impl Behaviour {
    pub fn new(filter: Filter) -> Self {
        Self { table: RoutingTable::default(), filter }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = libp2p::swarm::dummy::ConnectionHandler;
    type ToSwarm = Infallible;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        (self.filter)(&mut self.table, peer, local_addr, remote_addr)?;
        Ok(libp2p::swarm::dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(libp2p::swarm::dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        _effective_role: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
        if addresses.is_empty()
            && let Some(peer) = maybe_peer
            && let Some(addr) = self.table.get(peer)
        {
            return Ok(vec![addr.clone()]);
        }

        Ok(vec![])
    }

    fn on_swarm_event(&mut self, _: libp2p::swarm::FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        _event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        match _event {}
    }

    fn poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        std::task::Poll::Pending
    }
}

#[derive(Default, Clone)]
pub struct RoutingTable {
    // sorted vec is perfect since we almost never insert new entries
    routes: Vec<Route>,
}

impl RoutingTable {
    pub fn iter(&self) -> impl Iterator<Item = &Route> {
        self.routes.iter()
    }

    pub fn bulk_insert(&mut self, routes: impl IntoIterator<Item = Route>) {
        assert!(self.routes.is_empty());
        self.routes.extend(routes);
    }

    pub fn insert(&mut self, route: Route) {
        self.remove(route.id.into());
        self.routes.push(route);
    }

    pub fn remove(&mut self, id: [u8; 32]) -> Option<Route> {
        let index = self.routes.iter().position(|r| r.id == id.into())?;
        Some(self.routes.remove(index))
    }

    #[must_use]
    pub fn get(&self, id: PeerId) -> Option<&Multiaddr> {
        let id = id.to_hash();
        let id: U256 = id.into();
        Some(&self.routes.iter().find(|r| r.id == id)?.addr)
    }

    pub fn closest(&mut self, data: &[u8]) -> impl Iterator<Item = &Route> + '_ {
        let hash = blake3::hash(data);
        let id = U256::from(hash.as_bytes());
        self.closest_low(id)
    }

    fn closest_low(&mut self, id: U256) -> impl Iterator<Item = &Route> + '_ {
        self.routes.sort_unstable_by_key(|r| r.id ^ id);
        self.routes.iter()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub id: U256,
    pub addr: Multiaddr,
}

impl Route {
    pub fn new(id: [u8; 32], addr: Multiaddr) -> Self {
        let id = U256::from(id);
        Self { id, addr }
    }

    pub fn peer_id(&self) -> PeerId {
        let bytes: [u8; 32] = self.id.into();
        bytes.to_peer_id()
    }
}
