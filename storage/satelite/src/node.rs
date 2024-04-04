use {crate::OurPk, codec::Codec, crypto::proof::Proof, storage_spec::FreeSpace};

type Result<T, E = NodeError> = std::result::Result<T, E>;

#[derive(Codec, thiserror::Error, Debug)]
pub enum NodeError {
    #[error("invalid proof")]
    InvalidProof,
    #[error("already registered")]
    AlreadyRegistered,
    #[error("not registered")]
    NotRegistered,
    #[error("invalid nonce, expected: {0}")]
    InvalidNonce(u64),
    #[error("store thrown unexpected error, actual message is logged")]
    StoreError,
}

impl From<lmdb_zero::Error> for NodeError {
    fn from(err: lmdb_zero::Error) -> Self {
        log::error!("lmdb error: {:?}", err);
        NodeError::StoreError
    }
}

pub async fn register(
    cx: crate::Context,
    our: OurPk,
    (proof, size): (Proof<crypto::Hash>, FreeSpace),
) -> Result<()> {
    handlers::ensure!(our.to_bytes() == proof.context && proof.nonce == 0, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);

    let success = cx.store.nodes.write().unwrap().register_node(proof.identity(), size);
    handlers::ensure!(success, NodeError::AlreadyRegistered);
    Ok(())
}

pub async fn get_gc_meta(cx: crate::Context, our: OurPk, proof: Proof<crypto::Hash>) -> Result<()> {
    handlers::ensure!(our.to_bytes() == proof.context, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);

    let success = cx.store.nodes.write().unwrap().request_gc(proof.identity(), proof.nonce);
    handlers::ensure!(success.is_some(), NodeError::NotRegistered);

    todo!()
}
