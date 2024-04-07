use {
    anyhow::Context,
    codec::Codec,
    component_utils::PacketReader,
    crypto::{proof::Proof, sign},
    libp2p::{
        futures::{self, AsyncReadExt, AsyncWriteExt},
        PeerId,
    },
    rpc::CallId,
    std::{
        convert::identity,
        fs, future,
        io::{Read, Seek, Write},
    },
    storage_spec::{
        Address, BandwidthContext, ClientError, ClientResult as Result, CompactBandwidthUse,
        StoreContext, UserIdentity,
    },
};

const BUFFER_SIZE: usize = 1024 * 32;

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

    let download_task = async move {
        let mut file = fs::File::open(chain_api::to_hex(proof.context.address.to_bytes()))?;
        let mut len = proof.context.address.size;

        let mut buf = [0; BUFFER_SIZE];
        while len != 0 {
            let to_read = buf.len().min(len as _);
            len -= to_read as u64;

            stream.read_exact(&mut buf[..to_read]).await.context("reading from stream")?;
            file.write_all(&buf[..to_read]).context("writing to disk")?;
        }

        Ok::<_, anyhow::Error>(())
    };
    handlers::blocking!(futures::executor::block_on(download_task)).context("downloading file")?;

    Ok(())
}

pub async fn read_file(
    cx: crate::Context,
    origin: PeerId,
    cid: CallId,
    (address, offset, proof): (
        Address,
        u64,
        Result<(sign::PublicKey, Proof<BandwidthContext>), UserIdentity>,
    ),
) -> Result<()> {
    if let Ok((sk, proof)) = proof {
        cx.storage.bandwidts.write().unwrap().register(sk, proof)?;
    }

    let identity = proof.map_or_else(identity, |(_, proof)| proof.context.dest);
    let mut stream = cx.establish_stream(origin, cid).await?;

    // TODO: report the errors to user properly
    let upload_task = async move {
        let mut file = fs::File::open(chain_api::to_hex(address.to_bytes()))?;
        let mut len = file.metadata()?.len() - offset;
        file.seek(std::io::SeekFrom::Start(offset)).context("seekig")?;

        let mut buf = [0u8; BUFFER_SIZE];
        let mut reader = PacketReader::default();
        while len != 0 {
            let packet =
                future::poll_fn(|cx| reader.poll_packet(cx, &mut stream).map_ok(|v| v.to_vec()))
                    .await?;
            let bandwidth_use = CompactBandwidthUse::decode(&mut packet.as_slice())
                .context("decoding sig packet")?;
            let allowance = cx
                .storage
                .bandwidts
                .write()
                .unwrap()
                .update_allocation(identity, bandwidth_use)
                .context("cannot allocate bandwidth")?;

            let to_read = buf.len().min(len as _).min(allowance.get() as usize);
            len -= to_read as u64;

            file.read_exact(&mut buf[..to_read]).context("reading from file")?;
            stream.write_all(&buf[..to_read]).await.context("writing to stream")?;
        }

        Ok::<_, anyhow::Error>(())
    };

    handlers::blocking!(futures::executor::block_on(upload_task)).context("uploading file")?;

    Ok(())
}
