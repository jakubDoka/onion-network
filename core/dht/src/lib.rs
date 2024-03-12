#![feature(trait_alias)]
#![feature(let_chains)]
pub use primitive_types::U256;
use {
    libp2p::{
        identity::{ed25519, PublicKey},
        multihash::Multihash,
        swarm::NetworkBehaviour,
        Multiaddr, PeerId,
    },
    std::{convert::Infallible, iter},
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
        self.routes.sort_by_key(|r| r.id);
    }

    pub fn insert(&mut self, route: Route) {
        match self.routes.binary_search_by_key(&route.id, |r| r.id) {
            Ok(i) => self.routes[i] = route,
            Err(i) => self.routes.insert(i, route),
        }
    }

    pub fn remove(&mut self, id: PeerId) -> Option<Route> {
        let id = try_peer_id_to_ed(id)?;
        let index = self.routes.binary_search_by_key(&id.into(), |r| r.id).ok()?;
        Some(self.routes.remove(index))
    }

    #[must_use]
    pub fn get(&self, id: PeerId) -> Option<&Multiaddr> {
        let id = try_peer_id_to_ed(id)?;
        let id: U256 = id.into();
        let index = self.routes.binary_search_by_key(&id, |r| r.id).ok()?;
        Some(&self.routes[index].addr)
    }

    pub fn closest(&self, data: &[u8]) -> impl Iterator<Item = &Route> + '_ {
        let hash = blake3::hash(data);
        let id = U256::from(hash.as_bytes());
        self.closest_low(id)
    }

    fn closest_low(&self, id: U256) -> impl Iterator<Item = &Route> + '_ {
        let index = self.routes.binary_search_by_key(&id, |r| r.id).unwrap_or_else(|i| i);

        let mut left = self.routes[..index].iter().rev();
        let mut right = self.routes[index..].iter();
        let mut left_peek = left.next().or_else(|| right.next_back());
        let mut right_peek = right.next().or_else(|| left.next_back());

        iter::from_fn(move || {
            let (left_route, right_route) = match (left_peek, right_peek) {
                (Some(left), Some(right)) => (left, right),
                // we do not peek anymore since this must be the last one
                (Some(either), None) | (None, Some(either)) => {
                    left_peek = None;
                    right_peek = None;
                    return Some(either);
                }
                (None, None) => return None,
            };

            if shortest_distance(id, left_route.id) < shortest_distance(id, right_route.id) {
                left_peek = left.next().or_else(|| right.next_back());
                Some(left_route)
            } else {
                right_peek = right.next().or_else(|| left.next_back());
                Some(right_route)
            }
        })
    }
}

fn shortest_distance(a: U256, b: U256) -> U256 {
    a.overflowing_sub(b).0.min(b.overflowing_sub(a).0)
}

#[must_use]
pub fn try_peer_id_to_ed(id: PeerId) -> Option<[u8; 32]> {
    let multihash: &Multihash<64> = id.as_ref();
    let bytes = multihash.digest();
    bytes[bytes.len() - 32..].try_into().ok()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub id: U256,
    pub addr: Multiaddr,
}

impl Route {
    #[must_use]
    pub fn new(id: ed25519::PublicKey, addr: Multiaddr) -> Self {
        let id = U256::from(id.to_bytes());
        Self { id, addr }
    }

    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        let bytes: [u8; 32] = self.id.into();
        let key = ed25519::PublicKey::try_from_bytes(&bytes).expect("id to always be valid ed key");
        let key = PublicKey::from(key);
        PeerId::from(key)
    }
}

#[cfg(test)]
mod tests {
    use {super::*, libp2p::identity};

    #[test]
    fn convert_peer_id() {
        let key = ed25519::Keypair::generate();
        let id = key.public();
        let peer_id = identity::PublicKey::from(id.clone()).to_peer_id();

        assert_eq!(try_peer_id_to_ed(peer_id), Some(id.to_bytes()));
    }

    #[test]
    fn closest_correct_len() {
        let count = 10;
        let table = RoutingTable {
            routes: (0..count).map(|i| Route { id: i.into(), addr: Multiaddr::empty() }).collect(),
        };

        assert_eq!(table.closest(&[]).count(), count);
    }

    #[test]
    fn wrap_around() {
        let table = RoutingTable {
            routes: vec![
                Route { id: U256::from(2), addr: Multiaddr::empty() },
                Route { id: U256::MAX / 2, addr: Multiaddr::empty() },
                Route { id: U256::MAX, addr: Multiaddr::empty() },
            ],
        };

        assert_eq!(table.closest_low(1.into()).collect::<Vec<_>>(), vec![
            &table.routes[0],
            &table.routes[2],
            &table.routes[1]
        ]);
    }
}
