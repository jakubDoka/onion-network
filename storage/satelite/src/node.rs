use {
    crypto::proof::Proof,
    storage_spec::{BlockId, FreeSpace, NodeError, NodeResult as Result},
};

// TODO: add curration period
pub async fn register(
    cx: crate::Context,
    (proof, size): (Proof<crypto::Hash>, FreeSpace),
) -> Result<()> {
    let our = cx.keys.sign.identity();
    handlers::ensure!(our == proof.context && proof.nonce == 0, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);

    let success = cx.store.nodes.write().unwrap().register_node(proof.identity(), size);
    handlers::ensure!(success, NodeError::AlreadyRegistered);
    Ok(())
}

pub async fn get_gc_meta(
    cx: crate::Context,
    proof: Proof<crypto::Hash>,
) -> Result<Vec<(BlockId, u32)>> {
    handlers::ensure!(cx.keys.sign.identity() == proof.context, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);

    let success = cx.store.nodes.write().unwrap().request_gc(proof.identity(), proof.nonce);
    handlers::ensure!(let Some(node_id) = success, NodeError::NotRegistered);
    handlers::blocking!(cx.store.files.get_blocks_for(node_id)).map_err(Into::into)
}
