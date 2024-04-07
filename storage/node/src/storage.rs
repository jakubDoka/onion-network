use {
    crypto::{proof::Proof, sign},
    std::{
        collections::{hash_map, HashMap},
        num::NonZeroU64,
        sync::RwLock,
    },
    storage_spec::{
        BandwidthContext, BandwidthUse, ClientError, CompactBandwidthUse, UserIdentity,
    },
};

// TODO: use lmdb instead
pub struct Bandwidths {
    bandwidths: HashMap<UserIdentity, Proof<BandwidthUse>>,
    finished_bandwidths: Vec<Proof<BandwidthUse>>,
}

impl Bandwidths {
    pub fn new(_storage_dir: &str) -> anyhow::Result<Self> {
        Ok(Self { bandwidths: HashMap::new(), finished_bandwidths: Vec::new() })
    }

    pub fn register(
        &mut self,
        issuer: sign::PublicKey,
        allocation: Proof<BandwidthContext>,
    ) -> Result<(), ClientError> {
        handlers::ensure!(allocation.context.amount != 0, ClientError::InvalidProof);
        let is_consistemt = allocation.context.issuer != allocation.context.dest;
        handlers::ensure!(is_consistemt, ClientError::InvalidProof);
        handlers::ensure!(allocation.verify(), ClientError::InvalidProof);

        match self.bandwidths.entry(allocation.context.issuer) {
            hash_map::Entry::Occupied(_) => Err(ClientError::AlreadyRegistered),
            hash_map::Entry::Vacant(entry) => {
                entry.insert(Proof {
                    pk: issuer,
                    nonce: 0,
                    signature: allocation.signature,
                    context: BandwidthUse { proof: allocation, amount: 0 },
                });
                Ok(())
            }
        }
    }

    pub fn update_allocation(
        &mut self,
        issuer: UserIdentity,
        buse: CompactBandwidthUse,
    ) -> Option<NonZeroU64> {
        let hash_map::Entry::Occupied(mut bandwidth) = self.bandwidths.entry(issuer) else {
            return None;
        };

        let allowance = buse.amount.checked_sub(bandwidth.get().context.amount)?.try_into().ok()?;

        let mut new_bandwidth = *bandwidth.get();
        new_bandwidth.context.amount = buse.amount;
        new_bandwidth.signature = buse.signature;
        new_bandwidth.verify().then_some(())?;

        if new_bandwidth.context.is_exhaused() {
            self.finished_bandwidths.push(new_bandwidth);
            bandwidth.remove();
        } else {
            *bandwidth.get_mut() = new_bandwidth;
        }

        Some(allowance)
    }
}

pub struct Storage {
    pub bandwidts: RwLock<Bandwidths>,
}

impl Storage {
    pub(crate) fn new(storage_dir: &str) -> anyhow::Result<Self> {
        Ok(Self { bandwidts: Bandwidths::new(storage_dir)?.into() })
    }
}
