use {
    crypto::proof::Proof,
    rand_core::OsRng,
    std::{fs, os::unix::fs::FileExt},
    storage_spec::{Address, ClientError, ClientResult as Result, PIECE_SIZE},
};

pub async fn get_piece(
    cx: crate::Context,
    (proof, addr, offset): (Proof<crypto::Hash>, Address, u64),
) -> Result<Proof<[u8; PIECE_SIZE]>> {
    handlers::ensure!(cx.keys.sign.identity() == proof.context, ClientError::InvalidProof);
    handlers::ensure!(proof.verify(), ClientError::InvalidProof);
    cx.storage.satelites.write().unwrap().advance_nonce(proof.context, proof.nonce)?;

    let piece = handlers::blocking(move || {
        let file = fs::File::open(addr.to_file_name())?;
        let len = (addr.size - offset).min(PIECE_SIZE as u64) as usize;

        let mut buf = [0u8; PIECE_SIZE];
        file.read_exact_at(&mut buf[..len], offset)?;
        Ok::<_, anyhow::Error>(buf)
    })
    .await?;

    Ok(Proof::new(&cx.keys.sign, &mut 0, piece, OsRng))
}
