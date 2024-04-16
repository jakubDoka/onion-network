use {
    crypto::{
        proof::{Nonce, NonceInt, Proof},
        sign,
    },
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
        let is_consistemt = allocation.context.issuer == allocation.context.dest;
        handlers::ensure!(is_consistemt, ClientError::InvalidProof);
        handlers::ensure!(allocation.verify(), ClientError::InvalidProof);

        let poof = Proof {
            pk: issuer,
            nonce: 0,
            signature: allocation.signature,
            context: BandwidthUse { proof: allocation, amount: 0 },
        };

        match self.bandwidths.entry(allocation.context.issuer) {
            hash_map::Entry::Occupied(o) if o.get().context.is_exhaused() => {
                let expected_nonce = o.get().context.proof.nonce + 1;
                let is_valid = expected_nonce == allocation.nonce;
                handlers::ensure!(is_valid, ClientError::InvalidNonce(expected_nonce));
                *o.into_mut() = poof;
            }
            hash_map::Entry::Occupied(_) => return Err(ClientError::AlreadyRegistered),
            hash_map::Entry::Vacant(entry) => _ = entry.insert(poof),
        }

        Ok(())
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
        } else {
            *bandwidth.get_mut() = new_bandwidth;
        }

        Some(allowance)
    }
}

// TODO: use lmdb instead
pub struct Satelites {
    satelites: HashMap<UserIdentity, SateliteMeta>,
}

impl Satelites {
    pub fn new(_storage_dir: &str) -> anyhow::Result<Self> {
        Ok(Self { satelites: HashMap::new() })
    }

    pub fn add_satelite(&mut self, satelite: UserIdentity) -> Result<(), ClientError> {
        match self.satelites.entry(satelite) {
            hash_map::Entry::Occupied(_) => Err(ClientError::AlreadyRegistered),
            hash_map::Entry::Vacant(entry) => {
                entry.insert(SateliteMeta::default());
                Ok(())
            }
        }
    }

    pub fn advance_nonce(&mut self, satelite: UserIdentity, nonce: u64) -> Result<(), ClientError> {
        let satelite = self.satelites.get_mut(&satelite).ok_or(ClientError::InvalidProof)?;
        let is_valid = satelite.nonce.advance_to(nonce);
        handlers::ensure!(is_valid, ClientError::InvalidNonce(satelite.nonce + 1));
        Ok(())
    }

    pub fn advance_our_nonce(&mut self, satelite: UserIdentity) -> Result<u64, ClientError> {
        let s = self.satelites.get_mut(&satelite).ok_or(ClientError::NotRegistered)?;
        s.our_nonce += 1;
        Ok(s.our_nonce)
    }

    pub fn is_registered(&self, satelite: UserIdentity) -> bool {
        self.satelites.contains_key(&satelite)
    }
}

#[derive(Default)]
struct SateliteMeta {
    nonce: Nonce,
    our_nonce: Nonce,
}

pub struct Storage {
    pub bandwidts: RwLock<Bandwidths>,
    pub satelites: RwLock<Satelites>,
}

impl Storage {
    pub(crate) fn new(storage_dir: &str) -> anyhow::Result<Self> {
        Ok(Self {
            bandwidts: Bandwidths::new(storage_dir)?.into(),
            satelites: Satelites::new(storage_dir)?.into(),
        })
    }
}
