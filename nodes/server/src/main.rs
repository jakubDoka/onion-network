#![feature(iter_advance_by)]
#![feature(associated_type_bounds)]
#![feature(impl_trait_in_assoc_type)]
#![feature(array_windows)]
#![feature(iter_collect_into)]
#![feature(let_chains)]
#![feature(entry_insert)]
#![feature(iter_next_chunk)]
#![feature(if_let_guard)]
#![feature(map_try_insert)]
#![feature(macro_metavar_expr)]
#![feature(slice_take)]

use {
    self::handlers::chat::Chat,
    crate::handlers::{Handler, Router},
    anyhow::Context as _,
    chain_api::{ContractId, NodeAddress, NodeData},
    chat_spec::{rpcs, ChatName, Identity, PossibleTopic, Profile, Request, REPLICATION_FACTOR},
    component_utils::{Codec, LinearMap, Reminder, ReminderOwned},
    crypto::{enc, sign, TransmutationCircle},
    dashmap::{DashMap, DashSet},
    dht::Route,
    futures::channel::mpsc,
    libp2p::{
        core::{multiaddr, muxing::StreamMuxerBox, upgrade::Version},
        futures::{self, channel::oneshot, SinkExt, StreamExt},
        identity::{self, ed25519},
        swarm::{NetworkBehaviour, SwarmEvent},
        Multiaddr, PeerId, Transport,
    },
    onion::{EncryptedStream, PathId},
    rand_core::OsRng,
    rpc::CallId,
    std::{
        collections::HashMap,
        convert::Infallible,
        fs,
        future::Future,
        io,
        net::{IpAddr, Ipv4Addr},
        sync::Arc,
        task::Poll,
        time::Duration,
    },
    tokio::sync::RwLock,
};

mod db;
mod handlers;
#[cfg(test)]
mod tests;

type Context = &'static OwnedContext;
type StakeEvents = futures::channel::mpsc::Receiver<chain_api::Result<chain_api::StakeEvent>>;
type SE = libp2p::swarm::SwarmEvent<<Behaviour as NetworkBehaviour>::ToSwarm>;

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
        rpc_timeout: u64,
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

enum RpcRespChannel {
    Oneshot(oneshot::Sender<rpc::Result<Vec<u8>>>),
    Mpsc(mpsc::Sender<(PeerId, rpc::Result<Vec<u8>>)>),
}

impl RpcRespChannel {
    fn send(self, peer: PeerId, msg: rpc::Result<Vec<u8>>) {
        match self {
            Self::Oneshot(s) => _ = s.send(msg),
            Self::Mpsc(mut s) => _ = s.try_send((peer, msg)),
        }
    }
}

struct Server {
    swarm: libp2p::swarm::Swarm<Behaviour>,
    context: Context,
    request_events: mpsc::Receiver<RequestEvent>,
    clients: futures::stream::SelectAll<Stream>,
    client_router: handlers::Router,
    server_router: handlers::Router,
    stake_events: StakeEvents,
    pending_rpcs: HashMap<CallId, RpcRespChannel>,
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
            rpc: topology_wrapper::new(
                rpc::Behaviour::new(
                    rpc::Config::new().request_timeout(Duration::from_millis(config.rpc_timeout)),
                ),
                sender.clone(),
            ),
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

        let (request_event_sink, request_events) = mpsc::channel(100);

        Ok(Self {
            swarm,
            context: Box::leak(Box::new(OwnedContext {
                profiles: Default::default(),
                online: Default::default(),
                chats: Default::default(),
                recovers: Default::default(),
                request_event_sink,
            })),
            request_events,
            clients: Default::default(),
            stake_events,
            client_router: handlers::router!(
                chat::{
                    rpcs::CREATE_CHAT => create.repl();
                    rpcs::ADD_MEMBER => add_member.repl().restore();
                    rpcs::SEND_MESSAGE => send_message.repl().restore();
                    rpcs::FETCH_MESSAGES => fetch_messages.restore();
                };
                profile::{
                    rpcs::CREATE_PROFILE => create.repl();
                    rpcs::SEND_MAIL => send_mail.repl().restore();
                    rpcs::READ_MAIL => read_mail.repl().restore();
                    rpcs::SET_VAULT => set_vault.repl().restore();
                    rpcs::FETCH_PROFILE => fetch_keys.restore();
                    rpcs::FETCH_VAULT => fetch_vault.restore();
                };
            ),
            server_router: handlers::router!(
                chat::{
                    rpcs::CREATE_CHAT => create;
                    rpcs::ADD_MEMBER => add_member.restore();
                    rpcs::SEND_MESSAGE => send_message.restore();
                    rpcs::BLOCK_PROPOSAL => propose_msg_block;
                    rpcs::MAJOR_BLOCK => receive_block;
                    rpcs::FETCH_MINIMAL_CHAT_DATA => fetch_minimal_chat_data;
                };
                profile::{
                    rpcs::CREATE_PROFILE => create;
                    rpcs::SEND_MAIL => send_mail.restore();
                    rpcs::READ_MAIL => read_mail.restore();
                    rpcs::SET_VAULT => set_vault.restore();
                    rpcs::FETCH_PROFILE_FULL => fetch_full;
                };
            ),
            pending_rpcs: Default::default(),
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
                let Some((prefix, topic, body)) =
                    <(u8, PossibleTopic, Reminder)>::decode(&mut body.as_slice())
                else {
                    log::warn!("received invalid internal rpc request");
                    return;
                };

                let Some(we_have_the_topic) = self.validate_topic(Some(topic)) else {
                    log::warn!("received request with invalid topic: {:?}", topic);
                    return;
                };

                let request = Request { prefix, id, topic: Some(topic), body };

                self.server_router.handle(request, handlers::State {
                    swarm: &mut self.swarm,
                    location: OnlineLocation::Remote(peer),
                    context: self.context,
                    missing_topic: !we_have_the_topic,
                });
            }
            SwarmEvent::Behaviour(BehaviourEvent::Rpc(rpc::Event::Response(peer, id, body))) => {
                let Some(resp) = self.pending_rpcs.remove(&id) else {
                    log::warn!("received response to unknown rpc {:?} {:?}", id, body);
                    return;
                };
                resp.send(peer, body);
            }
            SwarmEvent::Behaviour(BehaviourEvent::Onion(onion::Event::InboundStream(
                inner,
                id,
            ))) => {
                self.clients.push(Stream::new(id, inner));
            }
            SwarmEvent::Behaviour(_ev) => {
                todo!()
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

        let Some(we_have_the_topic) = self.validate_topic(req.topic) else {
            log::warn!("client sent message with invalid topic: {:?}", req.topic);
            return;
        };

        log::debug!("received message from client: {:?} {:?}", req.id, req.prefix);

        if req.prefix == rpcs::SUBSCRIBE
            && let Some(topic) = req.topic
            && we_have_the_topic
        {
            let client = self.clients.iter_mut().find(|c| c.id == id).expect("no you don't");
            match topic {
                PossibleTopic::Profile(identity) => client.profile_sub = Some((identity, req.id)),
                PossibleTopic::Chat(chat) => _ = client.subscriptions.insert(chat, req.id),
            }
            _ = client.inner.write((req.id, ()));
        } else {
            self.client_router.handle(req, handlers::State {
                swarm: &mut self.swarm,
                location: OnlineLocation::Local(id),
                context: self.context,
                missing_topic: !we_have_the_topic,
            });
        }
    }

    fn validate_topic(&self, topic: Option<PossibleTopic>) -> Option<bool> {
        let Some(topic) = topic else {
            return Some(false);
        };

        if self
            .swarm
            .behaviour()
            .dht
            .table
            .closest(topic.as_bytes())
            .take(REPLICATION_FACTOR.get() + 1)
            .all(|r| r.peer_id() != *self.swarm.local_peer_id())
        {
            return None;
        }

        let mut we_have_the_topic = match topic {
            PossibleTopic::Profile(prf) => self.context.profiles.contains_key(&prf),
            PossibleTopic::Chat(chat) => self.context.chats.contains_key(&chat),
        };

        if !we_have_the_topic {
            we_have_the_topic = !self.context.recovers.insert(topic);
        } else {
            self.context.recovers.remove(&topic);
        }

        Some(we_have_the_topic)
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

    fn handle_client_router_response(
        &mut self,
        client: PathId,
        id: CallId,
        res: handlers::Response,
    ) {
        match res {
            handlers::Response::Success(pl) | handlers::Response::Failure(pl) => {
                let Some(client) = self.clients.iter_mut().find(|c| c.id == client) else {
                    log::warn!("client did not wait for respnse");
                    return;
                };
                if client.inner.write((id, ReminderOwned(pl))).is_none() {
                    log::warn!("client cant keep up with own requests");
                    client.inner.close();
                };
            }
            handlers::Response::DontRespond => {}
        }
    }

    fn handle_chat_event(&mut self, chat: ChatName, event: Vec<u8>) {
        for client in self.clients.iter_mut() {
            let Some(call) = client.subscriptions.get(&chat) else {
                continue;
            };

            _ = client.inner.write((call, Reminder(&event)));
        }
    }

    fn handle_profile_event(
        &mut self,
        identity: Identity,
        event: Vec<u8>,
        success_resp: oneshot::Sender<()>,
    ) {
        let Some((id, client)) = self
            .clients
            .iter_mut()
            .find_map(|c| (c.profile_sub?.0 == identity).then_some((c.profile_sub?.1, c)))
        else {
            return;
        };

        if client.inner.write((id, ReminderOwned(event))).is_none() {
            return;
        }

        _ = success_resp.send(());
    }

    fn handle_rpc(
        &mut self,
        peer: PeerId,
        msg: Vec<u8>,
        resp: Option<oneshot::Sender<rpc::Result<Vec<u8>>>>,
    ) {
        let call_id =
            match self.swarm.behaviour_mut().rpc.request(peer, msg.as_slice(), resp.is_none()) {
                Ok(call_id) => call_id,
                Err(e) => {
                    if let Some(resp) = resp {
                        _ = resp.send(Err(e));
                    }
                    return;
                }
            };

        if let Some(resp) = resp {
            self.pending_rpcs.insert(call_id, RpcRespChannel::Oneshot(resp));
        }
    }

    fn handle_repl_rpc(
        &mut self,
        topic: PossibleTopic,
        msg: Vec<u8>,
        mut resp: Option<mpsc::Sender<(PeerId, rpc::Result<Vec<u8>>)>>,
    ) {
        let us = *self.swarm.local_peer_id();
        let beh = self.swarm.behaviour_mut();
        for peer in beh
            .dht
            .table
            .closest(topic.as_bytes())
            .take(REPLICATION_FACTOR.get() + 1)
            .map(Route::peer_id)
            .filter(|p| *p != us)
        {
            let call_id = match beh.rpc.request(peer, msg.as_slice(), resp.is_none()) {
                Ok(call_id) => call_id,
                Err(e) => {
                    if let Some(resp) = &mut resp {
                        _ = resp.try_send((peer, Err(e)));
                    }
                    continue;
                }
            };

            if let Some(resp) = &resp {
                self.pending_rpcs.insert(call_id, RpcRespChannel::Mpsc(resp.clone()));
            }
        }
    }

    fn handle_server_router_response(&mut self, peer: PeerId, id: CallId, res: handlers::Response) {
        match res {
            handlers::Response::Success(b) | handlers::Response::Failure(b) => {
                self.swarm.behaviour_mut().rpc.respond(peer, id, b)
            }
            handlers::Response::DontRespond => {}
        };
    }
}

impl Future for Server {
    type Output = Infallible;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        let mut did_something = true;
        while std::mem::take(&mut did_something) {
            while let Poll::Ready(Some((id, m))) = self.clients.poll_next_unpin(cx) {
                self.handle_client_message(id, m);
                did_something = true;
            }

            while let Poll::Ready(Some(e)) = self.swarm.poll_next_unpin(cx) {
                self.handle_event(e);
                did_something = true;
            }

            while let Poll::Ready(Some(e)) = self.stake_events.poll_next_unpin(cx) {
                self.handle_stake_event(e);
                did_something = true;
            }

            while let Poll::Ready((res, OnlineLocation::Local(peer), id)) =
                self.client_router.poll(cx)
            {
                self.handle_client_router_response(peer, id, res);
                did_something = true;
            }

            while let Poll::Ready((res, OnlineLocation::Remote(peer), id)) =
                self.server_router.poll(cx)
            {
                self.handle_server_router_response(peer, id, res);
                did_something = true;
            }

            while let Poll::Ready(Some(event)) = self.request_events.poll_next_unpin(cx) {
                match event {
                    RequestEvent::Profile(identity, event, resp) => {
                        self.handle_profile_event(identity, event, resp)
                    }
                    RequestEvent::Chat(topic, event) => self.handle_chat_event(topic, event),
                    RequestEvent::Rpc(peer, msg, resp) => self.handle_rpc(peer, msg, resp),
                    RequestEvent::ReplRpc(topic, msg, resp) => {
                        self.handle_repl_rpc(topic, msg, resp)
                    }
                }
                did_something = true;
            }
        }

        Poll::Pending
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    onion: topology_wrapper::Behaviour<onion::Behaviour>,
    dht: dht::Behaviour,
    rpc: topology_wrapper::Behaviour<rpc::Behaviour>,
    report: topology_wrapper::report::Behaviour,
}

enum InnerStream {
    Normal(EncryptedStream),
    #[cfg(test)]
    Test(mpsc::Receiver<Vec<u8>>, Option<mpsc::Sender<Vec<u8>>>),
}

impl InnerStream {
    #[must_use = "write could have failed"]
    pub fn write<'a>(&mut self, data: impl Codec<'a>) -> Option<()> {
        match self {
            Self::Normal(s) => s.write(data),
            #[cfg(test)]
            Self::Test(_, s) => s.as_mut()?.try_send(data.to_bytes()).ok(),
        }
    }

    pub fn close(&mut self) {
        match self {
            Self::Normal(s) => s.close(),
            #[cfg(test)]
            Self::Test(_, s) => _ = s.take(),
        }
    }
}

pub struct Stream {
    id: PathId,
    profile_sub: Option<(Identity, CallId)>,
    subscriptions: LinearMap<ChatName, CallId>,
    inner: InnerStream,
}

impl Stream {
    fn new(id: PathId, inner: EncryptedStream) -> Self {
        Self {
            id,
            profile_sub: Default::default(),
            subscriptions: Default::default(),
            inner: InnerStream::Normal(inner),
        }
    }

    #[cfg(test)]
    fn new_test() -> [Self; 2] {
        let (input_s, input_r) = mpsc::channel(1);
        let (output_s, output_r) = mpsc::channel(1);
        let id = PathId::new();
        [(input_r, output_s), (output_r, input_s)].map(|(a, b)| Self {
            id,
            profile_sub: Default::default(),
            subscriptions: Default::default(),
            inner: InnerStream::Test(a, Some(b)),
        })
    }
}

impl libp2p::futures::Stream for Stream {
    type Item = (PathId, io::Result<Vec<u8>>);

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        match &mut self.inner {
            InnerStream::Normal(n) => n.poll_next_unpin(cx).map(|v| v.map(|v| (self.id, v))),
            #[cfg(test)]
            InnerStream::Test(t, _) => t.poll_next_unpin(cx).map(|v| v.map(|v| (self.id, Ok(v)))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OnlineLocation {
    Local(PathId),
    Remote(PeerId),
}

enum RequestEvent {
    Profile(Identity, Vec<u8>, oneshot::Sender<()>),
    Chat(ChatName, Vec<u8>),
    Rpc(PeerId, Vec<u8>, Option<oneshot::Sender<rpc::Result<Vec<u8>>>>),
    ReplRpc(PossibleTopic, Vec<u8>, Option<mpsc::Sender<(PeerId, rpc::Result<Vec<u8>>)>>),
}

pub struct OwnedContext {
    profiles: DashMap<Identity, Profile>,
    online: DashMap<Identity, OnlineLocation>,
    chats: DashMap<ChatName, Arc<RwLock<Chat>>>,
    recovers: DashSet<PossibleTopic>,
    request_event_sink: mpsc::Sender<RequestEvent>,
}

fn rpc<'a>(prefix: u8, topic: impl Into<PossibleTopic>, msg: impl Codec<'a>) -> Vec<u8> {
    (prefix, topic.into(), msg).to_bytes()
}

impl OwnedContext {
    async fn push_chat_event(&self, topic: ChatName, msg: chat_spec::ChatEvent<'_>) {
        _ = self.request_event_sink.clone().send(RequestEvent::Chat(topic, msg.to_bytes())).await;
    }

    async fn push_profile_event(&self, for_who: [u8; 32], mail: &[u8]) -> bool {
        let (sender, recv) = oneshot::channel();
        _ = self
            .request_event_sink
            .clone()
            .send(RequestEvent::Profile(for_who, mail.to_vec(), sender))
            .await;
        recv.await.is_ok()
    }

    async fn repl_rpc_no_resp<'a>(
        &self,
        topic: impl Into<PossibleTopic>,
        prefix: u8,
        msg: impl Codec<'a>,
    ) {
        let topic = topic.into();
        _ = self
            .request_event_sink
            .clone()
            .send(RequestEvent::ReplRpc(topic, rpc(prefix, topic, msg), None))
            .await;
    }

    async fn send_rpc_no_resp<'a>(
        &self,
        topic: impl Into<PossibleTopic>,
        dest: PeerId,
        prefix: u8,
        msg: impl Codec<'a>,
    ) {
        let topic = topic.into();
        _ = self
            .request_event_sink
            .clone()
            .send(RequestEvent::Rpc(dest, rpc(prefix, topic, msg), None))
            .await;
    }

    async fn replicate_rpc<'a>(
        &self,
        topic: impl Into<PossibleTopic>,
        prefix: u8,
        msg: impl Codec<'a>,
    ) -> impl futures::Stream<Item = (PeerId, rpc::Result<Vec<u8>>)> {
        let topic = topic.into();
        let (sender, recv) = mpsc::channel(REPLICATION_FACTOR.get());
        _ = self
            .request_event_sink
            .clone()
            .send(RequestEvent::ReplRpc(topic, rpc(prefix, topic, msg), Some(sender)))
            .await;
        recv
    }

    async fn send_rpc<'a>(
        &self,
        topic: impl Into<PossibleTopic>,
        peer: PeerId,
        send_mail: u8,
        mail: impl Codec<'a>,
    ) -> rpc::Result<Vec<u8>> {
        let topic = topic.into();
        let (sender, recv) = oneshot::channel();
        _ = self
            .request_event_sink
            .clone()
            .send(RequestEvent::Rpc(peer, rpc(send_mail, topic, mail), Some(sender)))
            .await;
        recv.await.unwrap_or(Err(rpc::Error::Timeout.into()))
    }
}
