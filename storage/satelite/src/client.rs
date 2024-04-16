use {
    crate::storage::UserMeta,
    anyhow::Context,
    crypto::proof::Proof,
    rand_core::OsRng,
    std::net::SocketAddr,
    storage_spec::{
        Address, BandwidthContext, ClientError, ClientResult as Result, ExpandedHolders, File,
        FileMeta, StoreContext, MAX_PIECES,
    },
};

pub async fn register(cx: crate::Context, proof: Proof<crypto::Hash>) -> Result<()> {
    let our_id = cx.keys.sign.identity();
    handlers::ensure!(our_id == proof.context && proof.nonce == 0, ClientError::InvalidProof);
    handlers::ensure!(proof.verify(), ClientError::InvalidProof);
    let success = handlers::blocking!(cx.store.users.save(proof.identity(), UserMeta::default()))
        .context("registering node")?;
    handlers::ensure!(success, ClientError::AlreadyRegistered);
    Ok(())
}

// TODO: Include payment
pub async fn allocate_file(
    cx: crate::Context,
    (size, proof): (u64, Proof<crypto::Hash>),
) -> Result<(File, [Proof<StoreContext>; MAX_PIECES])> {
    handlers::ensure!(cx.keys.sign.identity() == proof.context, ClientError::InvalidProof);
    handlers::ensure!(proof.verify(), ClientError::InvalidProof);
    let success = handlers::blocking!(cx.store.users.advance_nonce(proof.identity(), proof.nonce))?;
    handlers::ensure!(success, ClientError::InvalidProof);

    let dest = cx.store.nodes.write().unwrap().allocate_file(size);
    handlers::ensure!(let Some((address, holders, nonces)) = dest, ClientError::NotEnoughtNodes);
    let meta = FileMeta { holders, owner: proof.identity() };
    let success = handlers::blocking!(cx.store.files.save(address, meta)).context("saving file")?;
    handlers::ensure!(success, ClientError::YouWonTheLottery);
    let holders = cx.store.nodes.read().unwrap().expand_holders(holders);
    let mut nonces = nonces.into_iter();
    Ok((
        File { address, holders },
        holders.map(|(dest, _)| {
            let ctx = StoreContext { address, dest };
            let nonce = Proof::new(&cx.keys.sign, &mut nonces.next().unwrap(), ctx, OsRng).nonce;
            Proof::new(&cx.keys.sign, &mut { nonce }, ctx, OsRng)
        }),
    ))
}

pub async fn delete_file(cx: crate::Context, proof: Proof<Address>) -> Result<()> {
    handlers::ensure!(proof.verify(), ClientError::InvalidProof);
    let success = handlers::blocking!(cx.store.users.advance_nonce(proof.identity(), proof.nonce))?;
    handlers::ensure!(success, ClientError::InvalidProof);

    let success =
        handlers::blocking!(cx.store.files.delete(proof.context, |m| m.owner == proof.identity()))
            .context("deleting file")?;
    handlers::ensure!(success, ClientError::NotAllowed);
    Ok(())
}

pub async fn get_file_holders(cx: crate::Context, address: Address) -> Result<ExpandedHolders> {
    let meta = handlers::blocking!(cx.store.files.load(address)).context("getting file")?;
    handlers::ensure!(let Some(meta) = meta, ClientError::NotFound);
    Ok(cx.store.nodes.read().unwrap().expand_holders(meta.holders))
}

// TODO: Include payment
// TODO: optimize response size
pub async fn allocate_bandwidth(
    cx: crate::Context,
    (proof, target, amount): (Proof<crypto::Hash>, ExpandedHolders, u64),
) -> Result<[(Proof<BandwidthContext>, SocketAddr); MAX_PIECES]> {
    handlers::ensure!(cx.keys.sign.identity() == proof.context, ClientError::InvalidProof);
    handlers::ensure!(proof.verify(), ClientError::InvalidProof);
    let success = handlers::blocking!(cx.store.users.advance_nonce(proof.identity(), proof.nonce))?;
    handlers::ensure!(success, ClientError::InvalidProof);

    Ok(target.map(|(dest, addr)| {
        let ctx = BandwidthContext { dest, amount, issuer: proof.identity() };
        (Proof::new(&cx.keys.sign, &mut { proof.nonce }, ctx, OsRng), addr)
    }))
}
