use {
    polkadot::runtime_types::pallet_node_staker::pallet::{Event as PNSE, Event2 as PNSE2},
    runtime_types::pallet_node_staker::pallet::{NodeAddress, Stake, Stake2},
    std::net::{IpAddr, SocketAddr},
};
pub use {
    polkadot::*,
    subxt::{self, ext::*, Error, PolkadotConfig as Config},
    subxt_signer::{self, sr25519::Keypair},
};

pub type AccountId = <Config as subxt::config::Config>::AccountId;
pub type Balance = u128;
pub type BlockNumber = u32;
pub type Hash = <Config as subxt::config::Config>::Hash;
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[allow(clippy::needless_return)]
#[subxt::subxt(
    runtime_metadata_path = "metadata.scale",
    generate_docs,
    derive_for_type(
        path = "pallet_node_staker::pallet::Stake",
        derive = "Debug, Clone, PartialEq, Eq, PartialOrd, Ord"
    ),
    derive_for_type(path = "pallet_node_staker::pallet::Event", derive = "Clone, Copy"),
    derive_for_type(
        path = "pallet_node_staker::pallet::NodeAddress",
        derive = "Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash"
    ),
    derive_for_type(
        path = "pallet_user_manager::pallet::Profile",
        derive = "Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, codec::Codec"
    )
)]
mod polkadot {}

impl Clone for Stake2 {
    fn clone(&self) -> Self {
        unsafe { std::ptr::read(self) }
    }
}

impl From<PNSE2> for PNSE {
    fn from(event: PNSE2) -> Self {
        match event {
            PNSE2::Joined { identity, addr } => PNSE::Joined { identity, addr },
            PNSE2::AddrChanged { identity, addr } => PNSE::AddrChanged { identity, addr },
            PNSE2::Reclaimed { identity } => PNSE::Reclaimed { identity },
            PNSE2::Voted { source, target } => PNSE::Voted { source, target },
        }
    }
}

impl Stake {
    pub fn fake() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

impl From<SocketAddr> for NodeAddress {
    fn from(addr: SocketAddr) -> Self {
        match addr.ip() {
            IpAddr::V4(ip) => {
                let mut bytes = [0; 4 + 2];
                bytes[..4].copy_from_slice(&ip.octets());
                bytes[4..].copy_from_slice(&addr.port().to_be_bytes());
                Self::Ip4(bytes)
            }
            IpAddr::V6(ip) => {
                let mut bytes = [0; 16 + 2];
                bytes[..16].copy_from_slice(&ip.octets());
                bytes[16..].copy_from_slice(&addr.port().to_be_bytes());
                Self::Ip6(bytes)
            }
        }
    }
}

impl From<NodeAddress> for SocketAddr {
    fn from(value: NodeAddress) -> Self {
        match value {
            NodeAddress::Ip4(bytes) => {
                let mut ip = [0; 4];
                ip.copy_from_slice(&bytes[..4]);
                let port = u16::from_be_bytes([bytes[4], bytes[5]]);
                (IpAddr::V4(ip.into()), port).into()
            }
            NodeAddress::Ip6(bytes) => {
                let mut ip = [0; 16];
                ip.copy_from_slice(&bytes[..16]);
                let port = u16::from_be_bytes([bytes[16], bytes[17]]);
                (IpAddr::V6(ip.into()), port).into()
            }
        }
    }
}
