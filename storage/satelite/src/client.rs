use {
    crate::{storage::UserMeta, OurPk},
    anyhow::Context,
    codec::AsRawBytes,
    crypto::proof::Proof,
    storage_spec::{Address, ClientError, ExpandedHolders, File, FileMeta},
};

type Result<T, E = ClientError> = std::result::Result<T, E>;

pub async fn register(cx: crate::Context, pk: OurPk, proof: Proof<crypto::Hash>) -> Result<()> {
    handlers::ensure!(
        pk.to_bytes() == proof.context && proof.nonce == 0,
        ClientError::InvalidProof
    );
    handlers::ensure!(proof.verify(), ClientError::InvalidProof);
    let success = handlers::blocking!(cx.store.users.save(proof.identity(), UserMeta::default()))
        .context("registering node")?;
    handlers::ensure!(success, ClientError::AlreadyRegistered);
    Ok(())
}

// TODO: Include payment
pub async fn allocate_file(
    cx: crate::Context,
    pk: OurPk,
    (size, proof): (u64, Proof<crypto::Hash>),
) -> Result<File> {
    handlers::ensure!(pk.to_bytes() == proof.context, ClientError::InvalidProof);
    handlers::ensure!(proof.verify(), ClientError::InvalidProof);
    let validated =
        handlers::blocking!(cx.store.users.advance_nonce(proof.identity(), proof.nonce))?;
    handlers::ensure!(validated, ClientError::InvalidProof);

    let dest = cx.store.nodes.write().unwrap().allocate_file(size);
    handlers::ensure!(let Some((address, holders)) = dest, ClientError::NotEnoughtNodes);
    let meta = FileMeta { holders, owner: proof.identity() };
    let success = handlers::blocking!(cx.store.files.save(address, meta)).context("saving file")?;
    handlers::ensure!(success, ClientError::YouWonTheLottery);
    let holders = cx.store.nodes.read().unwrap().expand_holders(holders);
    Ok(File { address, holders })
}

pub async fn delete_file(cx: crate::Context, proof: Proof<AsRawBytes<Address>>) -> Result<()> {
    handlers::ensure!(proof.verify(), ClientError::InvalidProof);
    let validated =
        handlers::blocking!(cx.store.users.advance_nonce(proof.identity(), proof.nonce))?;
    handlers::ensure!(validated, ClientError::InvalidProof);

    let success = handlers::blocking!(cx
        .store
        .files
        .delete(proof.context.0, |m| m.owner == proof.identity()))
    .context("deleting file")?;
    handlers::ensure!(success, ClientError::NotAllowed);
    Ok(())
}

pub async fn get_file_holders(
    cx: crate::Context,
    address: AsRawBytes<Address>,
) -> Result<ExpandedHolders> {
    let meta = handlers::blocking!(cx.store.files.load(address.0)).context("getting file")?;
    handlers::ensure!(let Some(meta) = meta, ClientError::NotFound);
    Ok(cx.store.nodes.read().unwrap().expand_holders(meta.holders))
}

// TODO: Include payment
pub async fn allocate_bandwidth(cx: crate::Context, (): ()) -> Result<()> {
    todo!()
}
