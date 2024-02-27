#![feature(iter_advance_by)]
#![feature(iter_collect_into)]
#![feature(let_chains)]
#![feature(entry_insert)]
#![feature(iter_next_chunk)]
#![feature(if_let_guard)]
#![feature(map_try_insert)]
#![feature(macro_metavar_expr)]
#![feature(slice_take)]

#[cfg(test)]
use futures::channel::mpsc;
use {
    self::handlers::RequestOrigin,
    anyhow::Context as _,
    chain_api::{ContractId, NodeAddress, NodeData},
    chat_spec::{
        CallId, ChatName, CreateChat, CreateProfile, FetchFullProfile, FetchLatestBlock,
        FetchMessages, FetchProfile, FetchVault, Identity, PerformChatAction, PossibleTopic,
        Profile, ProposeMsgBlock, Protocol, ReadMail, SendBlock, SetVault, Subscribe, Topic,
        REPLICATION_FACTOR,
    },
    component_utils::{Codec, LinearMap, Reminder},
    crypto::{enc, sign, TransmutationCircle},
    dht::Route,
    handlers::{Chat, Handler, HandlerNest, Repl, Retry, SendMail, TryUnwrap},
    libp2p::{
        core::{multiaddr, muxing::StreamMuxerBox, upgrade::Version},
        futures::{self, stream::SelectAll, SinkExt, StreamExt},
        identity::{self, ed25519},
        swarm::{NetworkBehaviour, SwarmEvent},
        Multiaddr, PeerId, Transport,
    },
    onion::{EncryptedStream, PathId},
    rand_core::OsRng,
    std::{
        collections::HashMap,
        convert::Infallible,
        fs,
        future::Future,
        io,
        net::{IpAddr, Ipv4Addr},
        time::Duration,
    },
};

#[macro_export]
macro_rules! extract_ctx {
    ($self:expr) => {
        $crate::Context {
            swarm: &mut $self.swarm,
            clients: &mut $self.clients,
            storage: &mut $self.storage,
            res: &mut $self.res,
        }
    };
}

mod handlers;
#[cfg(test)]
mod tests;

type ReplRetry<T> = Repl<Retry<T>>;

compose_handlers! {
    InternalServer {
        CreateProfile,
        Retry<SetVault>,
        Retry<SendMail>,
        Retry<ReadMail>,
        Retry<FetchProfile>,
        FetchFullProfile,

        CreateChat,
        PerformChatAction,
        ProposeMsgBlock,
        SendBlock,
        FetchLatestBlock,
    }

    ExternalServer {
        Subscribe,

        Repl<CreateProfile>,
        ReplRetry<SetVault>,
        ReplRetry<SendMail>,
        ReplRetry<ReadMail>,
        ReplRetry<FetchProfile>,
        Retry<FetchVault>,

        Repl<CreateChat>,
        ReplRetry<PerformChatAction>,
        FetchMessages,
    }
}

#[derive(Clone)]
struct NodeKeys {
    enc: enc::Keypair,
    sign: sign::Keypair,
}

impl Default for NodeKeys {
    fn default() -> Self {
        Self { enc: enc::Keypair::new(OsRng), sign: sign::Keypair::new(OsRng) }
    }
}

impl NodeKeys {
    pub fn to_stored(&self) -> NodeData {
        NodeData {
            sign: crypto::hash::new(&self.sign.public_key()),
            enc: crypto::hash::new(&self.enc.public_key()),
            id: self.sign.public_key().pre,
        }
    }
}

crypto::impl_transmute! {
    NodeKeys,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let node_config = NodeConfig::from_env();
    let chain_config = ChainConfig::from_env();
    let (keys, is_new) = Server::load_keys(&node_config.key_path)?;
    let (node_list, stake_events) = deal_with_chain(chain_config, &keys, is_new).await?;

    Server::new(node_config, keys, node_list, stake_events)?.await;

    Ok(())
}

config::env_config! {
    struct NodeConfig {
        port: u16,
        ws_port: u16,
        key_path: String,
        boot_nodes: config::List<Multiaddr>,
        idle_timeout: u64,
    }
}

config::env_config! {
    struct ChainConfig {
        exposed_address: IpAddr,
        port: u16,
        nonce: u64,
        chain_node: String,
        node_account: String,
        node_contract: ContractId,
    }
}

type StakeEvents = futures::channel::mpsc::Receiver<chain_api::Result<chain_api::StakeEvent>>;

struct Server {
    swarm: libp2p::swarm::Swarm<Behaviour>,
    storage: Storage,
    clients: futures::stream::SelectAll<Stream>,
    buffer: Vec<u8>,
    internal: InternalServer,
    external: ExternalServer,
    stake_events: StakeEvents,
    res: TempRes,
}

fn unpack_node_id(id: sign::Ed) -> anyhow::Result<ed25519::PublicKey> {
    libp2p::identity::ed25519::PublicKey::try_from_bytes(&id).context("deriving ed signature")
}

fn unpack_node_addr(addr: chain_api::NodeAddress) -> Multiaddr {
    let (addr, port) = addr.into();
    Multiaddr::empty()
        .with(match addr {
            IpAddr::V4(ip) => multiaddr::Protocol::Ip4(ip),
            IpAddr::V6(ip) => multiaddr::Protocol::Ip6(ip),
        })
        .with(multiaddr::Protocol::Tcp(port))
}

async fn deal_with_chain(
    config: ChainConfig,
    keys: &NodeKeys,
    is_new: bool,
) -> anyhow::Result<(Vec<(NodeData, NodeAddress)>, StakeEvents)> {
    let ChainConfig { chain_node, node_account, node_contract, port, exposed_address, nonce } =
        config;
    let (mut chain_events_tx, stake_events) = futures::channel::mpsc::channel(0);
    let account = if node_account.starts_with("//") {
        chain_api::dev_keypair(&node_account)
    } else {
        chain_api::mnemonic_keypair(&node_account)
    };

    let client = chain_api::Client::with_signer(&chain_node, account)
        .await
        .context("connecting to chain")?;

    let node_list = client.list(node_contract.clone()).await.context("fetching node list")?;

    let stream = client.node_contract_event_stream(node_contract.clone()).await?;
    tokio::spawn(async move {
        let mut stream = std::pin::pin!(stream);
        // TODO: properly recover fro errors, add node pool to try
        while let Some(event) = stream.next().await {
            _ = chain_events_tx.send(event).await;
        }
    });

    if is_new {
        let nonce = client.get_nonce().await.context("fetching nonce")? + nonce;
        client
            .join(node_contract.clone(), keys.to_stored(), (exposed_address, port).into(), nonce)
            .await
            .context("registeing to chain")?;
        log::info!("registered on chain");
    }

    log::info!("entered the network with {} nodes", node_list.len());

    Ok((node_list, stake_events))
}

fn filter_incoming(
    table: &mut dht::RoutingTable,
    peer: PeerId,
    local_addr: &Multiaddr,
    _: &Multiaddr,
) -> Result<(), libp2p::swarm::ConnectionDenied> {
    if local_addr.iter().any(|p| p == multiaddr::Protocol::Ws("/".into())) {
        return Ok(());
    }

    if table.get(peer).is_none() {
        return Err(libp2p::swarm::ConnectionDenied::new("not registered as a node"));
    }

    Ok(())
}

impl Server {
    fn new(
        config: NodeConfig,
        keys: NodeKeys,
        node_list: Vec<(NodeData, NodeAddress)>,
        stake_events: StakeEvents,
    ) -> anyhow::Result<Self> {
        let NodeConfig { port, ws_port, boot_nodes, idle_timeout, .. } = config;

        let local_key = libp2p::identity::Keypair::ed25519_from_bytes(keys.sign.pre_quantum())
            .context("deriving ed signature")?;
        let peer_id = local_key.public().to_peer_id();
        log::info!("peer id: {}", peer_id);

        let (sender, receiver) = topology_wrapper::channel();
        let behaviour = Behaviour {
            onion: topology_wrapper::new(
                onion::Behaviour::new(
                    onion::Config::new(keys.enc.into(), peer_id)
                        .max_streams(10)
                        .keep_alive_interval(Duration::from_secs(100)),
                ),
                sender.clone(),
            ),
            dht: dht::Behaviour::new(filter_incoming),
            rpc: topology_wrapper::new(rpc::Behaviour::default(), sender.clone()),
            report: topology_wrapper::report::new(receiver),
        };
        let transport = libp2p::websocket::WsConfig::new(libp2p::tcp::tokio::Transport::new(
            libp2p::tcp::Config::default(),
        ))
        .upgrade(Version::V1)
        .authenticate(libp2p::noise::Config::new(&local_key).context("noise initialization")?)
        .multiplex(libp2p::yamux::Config::default())
        .or_transport(
            libp2p::tcp::tokio::Transport::new(libp2p::tcp::Config::default())
                .upgrade(Version::V1)
                .authenticate(
                    libp2p::noise::Config::new(&local_key).context("noise initialization")?,
                )
                .multiplex(libp2p::yamux::Config::default()),
        )
        .map(move |t, _| match t {
            futures::future::Either::Left((p, m)) => {
                (p, StreamMuxerBox::new(topology_wrapper::muxer::Muxer::new(m, sender)))
            }
            futures::future::Either::Right((p, m)) => {
                (p, StreamMuxerBox::new(topology_wrapper::muxer::Muxer::new(m, sender)))
            }
        })
        .boxed();
        let mut swarm = libp2p::swarm::Swarm::new(
            transport,
            behaviour,
            peer_id,
            libp2p::swarm::Config::with_tokio_executor()
                .with_idle_connection_timeout(Duration::from_millis(idle_timeout)),
        );

        swarm
            .listen_on(
                Multiaddr::empty()
                    .with(multiaddr::Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
                    .with(multiaddr::Protocol::Tcp(port)),
            )
            .context("starting to listen for peers")?;

        swarm
            .listen_on(
                Multiaddr::empty()
                    .with(multiaddr::Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
                    .with(multiaddr::Protocol::Tcp(ws_port))
                    .with(multiaddr::Protocol::Ws("/".into())),
            )
            .context("starting to isten for clients")?;

        let node_data = node_list
            .into_iter()
            .map(|(node, addr)| {
                let pk = unpack_node_id(node.id)?;
                let addr = unpack_node_addr(addr);
                Ok(Route::new(pk, addr))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        swarm.behaviour_mut().dht.table.bulk_insert(node_data);

        for boot_node in boot_nodes.0 {
            swarm.dial(boot_node).context("dialing a boot peer")?;
        }

        Ok(Self {
            swarm,
            clients: Default::default(),
            buffer: Default::default(),
            stake_events,
            storage: Default::default(),
            internal: Default::default(),
            external: Default::default(),
            res: Default::default(),
        })
    }

    fn load_keys(path: &str) -> io::Result<(NodeKeys, bool)> {
        let file = match fs::read(path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let nk = NodeKeys::default();
                fs::write(path, nk.as_bytes())?;
                return Ok((nk, true));
            }
            Err(e) => return Err(e),
        };

        let Some(nk) = NodeKeys::try_from_slice(&file).cloned() else {
            return Err(io::Error::other("invalid key file"));
        };

        Ok((nk, false))
    }

    fn handle_event(&mut self, event: SE) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Rpc(rpc::Event::Request(peer, id, body))) => {
                if self.swarm.behaviour_mut().dht.table.get(peer).is_none() {
                    log::warn!("peer {} made rpc request but is not on the white list", peer);
                    return;
                }

                let Some((&prefix, body)) = body.split_first() else {
                    log::info!("invalid rpc request");
                    return;
                };

                let req =
                    handlers::Request { prefix, id, origin: RequestOrigin::Server(peer), body };
                self.buffer.clear();
                let res = self.internal.execute(extract_ctx!(self), req, &mut self.buffer);
                match res {
                    Ok(false) => {}
                    Ok(true) => {
                        self.swarm.behaviour_mut().rpc.respond(peer, id, self.buffer.as_slice());
                    }
                    Err(e) => {
                        log::info!("failed to dispatch rpc request: {}", e);
                    }
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Onion(onion::Event::InboundStream(
                inner,
                id,
            ))) => {
                self.clients.push(Stream::new(id, inner));
            }
            SwarmEvent::Behaviour(ev) => {
                self.buffer.clear();
                let Ok((origin, id)) = Err(ev)
                    .or_else(|ev| {
                        self.internal.try_complete(extract_ctx!(self), ev, &mut self.buffer)
                    })
                    .or_else(|ev| {
                        self.external.try_complete(extract_ctx!(self), ev, &mut self.buffer)
                    })
                else {
                    return;
                };

                match origin {
                    RequestOrigin::Client(pid) => {
                        let Some(stream) = self.clients.iter_mut().find(|s| s.id == pid) else {
                            log::info!("client did not stay for response");
                            return;
                        };
                        if stream.inner.write((id, Reminder(&self.buffer))).is_none() {
                            log::info!("client cannot process the late response");
                        }
                    }
                    RequestOrigin::Server(mid) => {
                        self.swarm.behaviour_mut().rpc.respond(mid, id, self.buffer.as_slice());
                    }
                }
            }
            e => log::debug!("{e:?}"),
        }
    }

    fn handle_client_message(&mut self, id: PathId, req: io::Result<Vec<u8>>) {
        let req = match req {
            Ok(req) => req,
            Err(e) => {
                log::info!("failed to read from client: {}", e);
                return;
            }
        };

        let Some(req) = chat_spec::Request::decode(&mut req.as_slice()) else {
            log::info!("failed to decode client request: {:?}", req);
            return;
        };

        log::info!("received message from client: {:?} {:?}", req.id, req.prefix,);

        let req = handlers::Request {
            prefix: req.prefix,
            id: req.id,
            origin: RequestOrigin::Client(id),
            body: req.body.0,
        };
        self.buffer.clear();
        let res = self.external.execute(extract_ctx!(self), req, &mut self.buffer);

        match res {
            Ok(false) => {}
            Ok(true) => {
                let stream =
                    self.clients.iter_mut().find(|s| s.id == id).expect("we just received message");
                if stream.inner.write((req.id, Reminder(&self.buffer))).is_none() {
                    log::info!("client cannot process the response");
                }
            }
            Err(e) => {
                log::info!("failed to dispatch client request: {}", e);
            }
        }
    }

    fn handle_stake_event(&mut self, event: Result<chain_api::StakeEvent, chain_api::Error>) {
        let event = match event {
            Ok(event) => event,
            Err(e) => {
                log::info!("failed to read from chain: {}", e);
                return;
            }
        };

        match event {
            chain_api::StakeEvent::Joined(j) => {
                let Ok(pk) = unpack_node_id(j.identity) else {
                    log::info!("invalid node id");
                    return;
                };
                log::info!("node joined the network: {pk:?}");
                let route = Route::new(pk, unpack_node_addr(j.addr));
                self.swarm.behaviour_mut().dht.table.insert(route);
            }
            chain_api::StakeEvent::Reclaimed(r) => {
                let Ok(pk) = unpack_node_id(r.identity) else {
                    log::info!("invalid node id");
                    return;
                };
                log::info!("node left the network: {pk:?}");
                self.swarm
                    .behaviour_mut()
                    .dht
                    .table
                    .remove(identity::PublicKey::from(pk).to_peer_id());
            }
            chain_api::StakeEvent::AddrChanged(c) => {
                let Ok(pk) = unpack_node_id(c.identity) else {
                    log::info!("invalid node id");
                    return;
                };
                log::info!("node changed address: {pk:?}");
                let route = Route::new(pk, unpack_node_addr(c.addr));
                self.swarm.behaviour_mut().dht.table.insert(route);
            }
        }
    }
}

impl Future for Server {
    type Output = Infallible;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        while let std::task::Poll::Ready(Some((id, m))) = self.clients.poll_next_unpin(cx) {
            self.handle_client_message(id, m);
        }

        while let std::task::Poll::Ready(Some(e)) = self.swarm.poll_next_unpin(cx) {
            self.handle_event(e);
        }

        while let std::task::Poll::Ready(Some(e)) = self.stake_events.poll_next_unpin(cx) {
            self.handle_stake_event(e);
        }

        std::task::Poll::Pending
    }
}

#[derive(Default)]
pub struct TempRes {
    pub hashes: Vec<crypto::Hash>,
}

pub struct Context<'a> {
    swarm: &'a mut libp2p::swarm::Swarm<Behaviour>,
    clients: &'a mut SelectAll<Stream>,
    storage: &'a mut Storage,
    res: &'a mut TempRes,
}

impl Context<'_> {
    fn subscribe(&mut self, topic: PossibleTopic, id: CallId, origin: PathId) {
        let Some(stream) = self.clients.iter_mut().find(|s| s.id == origin) else {
            log::error!("whaaaat???");
            return;
        };
        stream.subscriptions.insert(topic, id);
    }

    fn push(&mut self, topic: ChatName, event: <ChatName as Topic>::Event<'_>) {
        handle_event(self.clients, PossibleTopic::Chat(topic), event);
    }

    fn is_valid_topic(&self, topic: PossibleTopic) -> bool {
        replicators_for(&self.swarm.behaviour().dht.table, topic)
            .any(|peer| peer == *self.swarm.local_peer_id())
    }

    fn _replicators_for(
        &self,
        topic: impl Into<PossibleTopic>,
    ) -> impl Iterator<Item = PeerId> + '_ {
        replicators_for(&self.swarm.behaviour().dht.table, topic.into())
    }

    fn other_replicators_for(
        &self,
        topic: impl Into<PossibleTopic>,
    ) -> impl Iterator<Item = PeerId> + '_ {
        other_replicators_for(
            &self.swarm.behaviour().dht.table,
            topic.into(),
            *self.swarm.local_peer_id(),
        )
    }
}

fn replicators_for(
    table: &dht::RoutingTable,
    topic: impl Into<PossibleTopic>,
) -> impl Iterator<Item = PeerId> + '_ {
    table.closest(topic.into().as_bytes()).take(REPLICATION_FACTOR.get() + 1).map(Route::peer_id)
}

fn other_replicators_for(
    table: &dht::RoutingTable,
    topic: impl Into<PossibleTopic>,
    us: PeerId,
) -> impl Iterator<Item = PeerId> + '_ {
    replicators_for(table, topic).filter(move |&p| p != us)
}

fn push_notification(
    streams: &mut SelectAll<Stream>,
    topic: Identity,
    event: <Identity as Topic>::Event<'_>,
    recip: PathId,
) -> bool {
    let Some(stream) = streams.iter_mut().find(|s| s.id == recip) else {
        return false;
    };

    let Some(&call_id) = stream.subscriptions.get(&PossibleTopic::Profile(topic)) else {
        return false;
    };

    if stream.inner.write((call_id, event)).is_none() {
        return false;
    }

    log::info!("pushed notification to client");
    true
}

fn handle_event<'a>(streams: &mut SelectAll<Stream>, topic: PossibleTopic, event: impl Codec<'a>) {
    for stream in streams.iter_mut() {
        let Some(&call_id) = stream.subscriptions.get(&topic) else {
            continue;
        };

        if stream.inner.write((call_id, &event)).is_none() {
            log::info!("client cannot process the subscription response");
            continue;
        }
    }
}

type SE = libp2p::swarm::SwarmEvent<<Behaviour as NetworkBehaviour>::ToSwarm>;

#[derive(NetworkBehaviour)]
struct Behaviour {
    onion: topology_wrapper::Behaviour<onion::Behaviour>,
    dht: dht::Behaviour,
    rpc: topology_wrapper::Behaviour<rpc::Behaviour>,
    report: topology_wrapper::report::Behaviour,
}

impl From<Infallible> for BehaviourEvent {
    fn from(v: Infallible) -> Self {
        match v {}
    }
}

impl TryUnwrap<Infallible> for BehaviourEvent {
    fn try_unwrap(self) -> Result<Infallible, Self> {
        Err(self)
    }
}

impl From<rpc::Event> for BehaviourEvent {
    fn from(v: rpc::Event) -> Self {
        Self::Rpc(v)
    }
}

impl TryUnwrap<rpc::Event> for BehaviourEvent {
    fn try_unwrap(self) -> Result<rpc::Event, Self> {
        match self {
            Self::Rpc(e) => Ok(e),
            e => Err(e),
        }
    }
}

impl<'a> TryUnwrap<&'a rpc::Event> for &'a Infallible {
    fn try_unwrap(self) -> Result<&'a rpc::Event, &'a Infallible> {
        Err(self)
    }
}

impl<'a> TryUnwrap<&'a Infallible> for &'a rpc::Event {
    fn try_unwrap(self) -> Result<&'a Infallible, &'a rpc::Event> {
        Err(self)
    }
}

enum InnerStream {
    Normal(EncryptedStream),
    #[cfg(test)]
    Test(mpsc::Receiver<Vec<u8>>, mpsc::Sender<Vec<u8>>),
}

impl InnerStream {
    #[must_use = "write could have failed"]
    pub fn write<'a>(&mut self, data: impl Codec<'a>) -> Option<()> {
        match self {
            Self::Normal(s) => s.write(data),
            #[cfg(test)]
            Self::Test(_, s) => s.try_send(data.to_bytes()).ok(),
        }
    }
}

pub struct Stream {
    id: PathId,
    subscriptions: LinearMap<PossibleTopic, CallId>,
    inner: InnerStream,
}

impl Stream {
    fn new(id: PathId, inner: EncryptedStream) -> Self {
        Self { id, subscriptions: Default::default(), inner: InnerStream::Normal(inner) }
    }

    #[cfg(test)]
    fn new_test() -> [Self; 2] {
        let (input_s, input_r) = mpsc::channel(1);
        let (output_s, output_r) = mpsc::channel(1);
        let id = PathId::new();
        [(input_r, output_s), (output_r, input_s)].map(|(a, b)| Self {
            id,
            subscriptions: Default::default(),
            inner: InnerStream::Test(a, b),
        })
    }
}

impl libp2p::futures::Stream for Stream {
    type Item = (PathId, io::Result<Vec<u8>>);

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match &mut self.inner {
            InnerStream::Normal(n) => n.poll_next_unpin(cx).map(|v| v.map(|v| (self.id, v))),
            #[cfg(test)]
            InnerStream::Test(t, _) => t.poll_next_unpin(cx).map(|v| v.map(|v| (self.id, Ok(v)))),
        }
    }
}

#[derive(Default)]
pub struct Storage {
    profiles: HashMap<Identity, Profile>,
    online: HashMap<Identity, RequestOrigin>,
    chats: HashMap<ChatName, Chat>,
}
