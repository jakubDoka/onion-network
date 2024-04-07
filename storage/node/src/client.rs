use {
    anyhow::Context,
    codec::Codec,
    crypto::proof::Proof,
    libp2p::{
        futures::{self, AsyncReadExt},
        PeerId,
    },
    rpc::CallId,
    std::{fs, io::Write},
    storage_spec::{ClientError, ClientResult as Result, StoreContext},
};

pub async fn store_file(
    cx: crate::Context,
    origin: PeerId,
    cid: CallId,
    proof: Proof<StoreContext>,
) -> Result<()> {
    // TODO: verify we are registered to the satelite
    handlers::ensure!(proof.context.dest == cx.keys.sign.identity(), ClientError::InvalidProof);
    handlers::ensure!(proof.verify(), ClientError::InvalidProof);

    let mut stream = cx.establish_stream(origin, cid).await?;

    let fut = async move {
        let mut file = fs::File::open(chain_api::to_hex(proof.context.address.to_bytes()))?;

        let mut buf = [0; 1024 * 4];
        let mut offset = 0;
        loop {
            while offset < buf.len() {
                let n = stream.read(&mut buf[offset..]).await.context("reading from stream")?;
                if n == 0 {
                    return Ok::<_, anyhow::Error>(());
                }
                offset += n;
            }

            file.write_all(&buf).context("writing to disk")?;
            offset = 0;
        }
    };
    handlers::blocking!(futures::executor::block_on(fut)).context("downloading file")?;

    Ok(())
}

pub async fn read_file((): ()) -> Result<()> {
    todo!()
}
