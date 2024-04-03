use {crate::OurPk, codec::Codec, crypto::proof::Proof};

type Result<T, E = NodeError> = std::result::Result<T, E>;

#[derive(Codec)]
pub enum NodeError {
    InvalidProof,
    AlreadyRegistered,
    NotRegistered,
    InvalidNonce(u64),
    StoreError,
}

impl From<lmdb_zero::Error> for NodeError {
    fn from(err: lmdb_zero::Error) -> Self {
        log::error!("lmdb error: {:?}", err);
        NodeError::StoreError
    }
}

pub async fn register(cx: crate::Context, our: OurPk, proof: Proof<crypto::Hash>) -> Result<()> {
    handlers::ensure!(our.to_bytes() == proof.context && proof.nonce == 0, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);
    handlers::ensure!(cx.store.register(proof.identity())?, NodeError::AlreadyRegistered);
    Ok(())
}

// TODO: ratelimit this
pub async fn get_gc_meta(cx: crate::Context, our: OurPk, proof: Proof<crypto::Hash>) -> Result<()> {
    handlers::ensure!(our.to_bytes() == proof.context, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);
    handlers::ensure!(let Some(mut node) = cx.store.load_node(proof.identity())?, NodeError::NotRegistered);

    Ok(())
}
