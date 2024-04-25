#![feature(trait_alias)]
#![feature(let_chains)]
pub use primitive_types::U256;
use {
    arrayvec::ArrayVec,
    libp2p::{swarm::NetworkBehaviour, Multiaddr, PeerId},
    opfusk::{PeerIdExt, ToPeerId},
    std::convert::Infallible,
};

pub type SharedRoutingTable = &'static spin::RwLock<RoutingTable>;

pub type Filter = fn(
    &mut RoutingTable,
    PeerId,
    &Multiaddr,
    &Multiaddr,
) -> Result<(), libp2p::swarm::ConnectionDenied>;

#[derive(Clone)]
pub struct Behaviour {
    pub table: SharedRoutingTable,
    filter: Filter,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self::new(|_, _, _, _| Ok(()))
    }
}

impl Behaviour {
    pub fn new(filter: Filter) -> Self {
        Self { table: Box::leak(Box::default()), filter }
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
        (self.filter)(&mut self.table.write(), peer, local_addr, remote_addr)?;
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
            && let Some(addr) = self.table.read().get(peer)
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
        self.routes.sort_unstable_by_key(|r| r.id);
    }

    pub fn insert(&mut self, route: Route) {
        let index = self.routes.binary_search_by_key(&route.id, |r| r.id);
        match index {
            Ok(index) => self.routes[index] = route,
            Err(index) => self.routes.insert(index, route),
        }
    }

    pub fn remove(&mut self, id: [u8; 32]) -> Option<Route> {
        let id = U256::from(id);
        let index = self.routes.binary_search_by_key(&id, |r| r.id);
        index.map(|index| self.routes.remove(index)).ok()
    }

    #[must_use]
    pub fn get(&self, id: PeerId) -> Option<&Multiaddr> {
        let id = id.to_hash();
        let id = U256::from(id);
        let index = self.routes.binary_search_by_key(&id, |r| r.id);
        index.map(|index| &self.routes[index].addr).ok()
    }

    pub fn closest<const COUNT: usize>(&self, key: &[u8]) -> ArrayVec<U256, COUNT> {
        let &key = blake3::hash(key).as_bytes();
        let key = U256::from(key);
        let mut slots = ArrayVec::<U256, COUNT>::new();
        let mut iter = self.routes.iter().map(|r| r.id);

        slots.extend(iter.by_ref().take(COUNT));
        slots.sort_unstable_by_key(|&id| id ^ key);

        for id in iter {
            let distance = id ^ key;
            if distance >= *slots.last().unwrap() ^ key {
                continue;
            }
            slots.pop();
            let point = slots.partition_point(|&slot| slot ^ key < distance);
            slots.insert(point, id);
        }

        slots
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
