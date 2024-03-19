use {
    self::polkadot::runtime_types::pallet_node_staker::pallet::{
        NodeAddress, NodeData, Stake, Votes,
    },
    std::net::IpAddr,
    subxt::PolkadotConfig,
};
pub use {
    polkadot::*,
    subxt::{self, ext::*},
    subxt_signer,
};

pub type Config = PolkadotConfig;
pub type AccountId = <Config as subxt::config::Config>::AccountId;
pub type Balance = u128;
pub type BlockNumber = u32;
pub type Hash = <Config as subxt::config::Config>::Hash;
pub type Error = subxt::Error;
pub type Result<T, E = Error> = std::result::Result<T, E>;

pub type Keypair = subxt_signer::sr25519::Keypair;

#[allow(clippy::needless_return)]
#[subxt::subxt(runtime_metadata_path = "metadata.scale", generate_docs)]
mod polkadot {}

impl Stake {
    pub fn fake() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

impl Clone for Votes {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for Votes {}

impl Clone for Stake {
    fn clone(&self) -> Self {
        Stake {
            owner: self.owner.clone(),
            amount: self.amount,
            created_at: self.created_at,
            votes: self.votes,
            id: self.id,
            addr: self.addr,
        }
    }
}

impl Clone for NodeAddress {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for NodeAddress {}

impl Clone for NodeData {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for NodeData {}

impl From<(IpAddr, u16)> for NodeAddress {
    fn from((addr, port): (IpAddr, u16)) -> Self {
        match addr {
            IpAddr::V4(ip) => {
                let mut bytes = [0; 4 + 2];
                bytes[..4].copy_from_slice(&ip.octets());
                bytes[4..].copy_from_slice(&port.to_be_bytes());
                Self::Ip4(bytes)
            }
            IpAddr::V6(ip) => {
                let mut bytes = [0; 16 + 2];
                bytes[..16].copy_from_slice(&ip.octets());
                bytes[16..].copy_from_slice(&port.to_be_bytes());
                Self::Ip6(bytes)
            }
        }
    }
}

impl From<NodeAddress> for (IpAddr, u16) {
    fn from(value: NodeAddress) -> Self {
        match value {
            NodeAddress::Ip4(bytes) => {
                let mut ip = [0; 4];
                ip.copy_from_slice(&bytes[..4]);
                let port = u16::from_be_bytes([bytes[4], bytes[5]]);
                (IpAddr::V4(ip.into()), port)
            }
            NodeAddress::Ip6(bytes) => {
                let mut ip = [0; 16];
                ip.copy_from_slice(&bytes[..16]);
                let port = u16::from_be_bytes([bytes[16], bytes[17]]);
                (IpAddr::V6(ip.into()), port)
            }
        }
    }
}
