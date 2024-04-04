use {
    crate::OurPk,
    crypto::proof::Proof,
    storage_spec::{BlockId, FreeSpace, NodeError},
};

type Result<T, E = NodeError> = std::result::Result<T, E>;

// TODO: add curration period
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

pub async fn get_gc_meta(
    cx: crate::Context,
    our: OurPk,
    proof: Proof<crypto::Hash>,
) -> Result<Vec<(BlockId, u32)>> {
    handlers::ensure!(our.to_bytes() == proof.context, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);

    let success = cx.store.nodes.write().unwrap().request_gc(proof.identity(), proof.nonce);
    handlers::ensure!(let Some(node_id) = success, NodeError::NotRegistered);
    handlers::blocking!(cx.store.files.get_blocks_for(node_id)).map_err(Into::into)
}
