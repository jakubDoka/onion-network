use {
    crypto::proof::Proof,
    libp2p::Multiaddr,
    std::net::{IpAddr, SocketAddr},
    storage_spec::{Address, FreeSpace, NodeError, NodeResult as Result},
};

// TODO: add curration period
pub async fn register(
    cx: crate::Context,
    addr: Multiaddr,
    (proof, size): (Proof<crypto::Hash>, FreeSpace),
) -> Result<()> {
    let our = cx.keys.sign.identity();
    handlers::ensure!(our == proof.context && proof.nonce == 0, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);

    let mut nodes = cx.store.nodes.write().unwrap();
    let addr = extract_socket_addr(addr).ok_or(NodeError::InvalidAddress)?;
    let success = nodes.register_node(proof.identity(), proof.pk.pre, addr, size);
    handlers::ensure!(success, NodeError::AlreadyRegistered);
    Ok(())
}

pub async fn heartbeat(
    cx: crate::Context,
    addr: Multiaddr,
    proof: Proof<crypto::Hash>,
) -> Result<()> {
    handlers::ensure!(cx.keys.sign.identity() == proof.context, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);
    let addr = extract_socket_addr(addr).ok_or(NodeError::InvalidAddress)?;
    let success = cx.store.nodes.write().unwrap().update_addr(proof.identity(), proof.nonce, addr);
    handlers::ensure!(success, NodeError::NotRegistered);
    Ok(())
}

pub async fn get_gc_meta(cx: crate::Context, proof: Proof<crypto::Hash>) -> Result<Vec<Address>> {
    handlers::ensure!(cx.keys.sign.identity() == proof.context, NodeError::InvalidProof);
    handlers::ensure!(proof.verify(), NodeError::InvalidProof);

    let success = cx.store.nodes.write().unwrap().request_gc(proof.identity(), proof.nonce);
    handlers::ensure!(let Some(node_id) = success, NodeError::NotRegistered);
    handlers::blocking!(cx.store.files.get_files_for(node_id)).map_err(Into::into)
}

fn extract_socket_addr(addr: Multiaddr) -> Option<SocketAddr> {
    use libp2p::core::multiaddr::Protocol;

    let mut iter = addr.iter();
    let ip = match iter.next() {
        Some(Protocol::Ip4(ip)) => IpAddr::V4(ip),
        Some(Protocol::Ip6(ip)) => IpAddr::V6(ip),
        _ => return None,
    };
    let port = match iter.next() {
        Some(Protocol::Tcp(port)) | Some(Protocol::Udp(port)) => port,
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}
