#![feature(iter_advance_by)]
#![feature(trait_alias)]
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
    self::api::{chat::Chat, MissingTopic},
    crate::api::Handler as _,
    anyhow::Context as _,
    chain_api::{NodeData, Stake},
    chat_spec::{rpcs, ChatError, ChatName, Identity, Profile, Request, Topic, REPLICATION_FACTOR},
    codec::{Codec, Reminder, ReminderOwned},
    crypto::{enc, sign},
    dashmap::{mapref::entry::Entry, DashMap},
    dht::Route,
    futures::channel::mpsc,
    libp2p::{
        core::{multiaddr, muxing::StreamMuxerBox, upgrade::Version},
        futures::{self, channel::oneshot, SinkExt, StreamExt},
        identity::ed25519,
        swarm::{NetworkBehaviour, SwarmEvent},
        Multiaddr, PeerId, Transport,
    },
    onion::{key_share, EncryptedStream, PathId},
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

mod api;
mod db;
#[cfg(test)]
mod tests;

type Context = &'static OwnedContext;
type StakeEvents = futures::channel::mpsc::Receiver<chain_api::Result<chain_api::StakeEvent>>;
type SE = libp2p::swarm::SwarmEvent<<Behaviour as NetworkBehaviour>::ToSwarm>;

#[derive(Clone, Codec)]
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
            sign: crypto::hash::new(self.sign.public_key()),
            enc: crypto::hash::new(self.enc.public_key()),
            id: self.sign.public_key().pre,
        }
    }
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
        chain_nodes: config::List<String>,
        node_account: String,
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
    client_router: handlers::Router<api::RouterContext>,
    server_router: handlers::Router<api::RouterContext>,
    stake_events: StakeEvents,
    pending_rpcs: HashMap<CallId, RpcRespChannel>,
}

fn unpack_node_id(id: sign::Pre) -> anyhow::Result<ed25519::PublicKey> {
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
) -> anyhow::Result<(Vec<Stake>, StakeEvents)> {
    let ChainConfig { chain_nodes, node_account, port, exposed_address, nonce } = config;
    let (mut chain_events_tx, stake_events) = futures::channel::mpsc::channel(0);
    let account = if node_account.starts_with("//") {
        chain_api::dev_keypair(&node_account)
    } else {
        chain_api::mnemonic_keypair(&node_account)
    };

    let mut others = chain_nodes.0.into_iter();

    let (node_list, client) = 'a: {
        for node in others.by_ref() {
            let Ok(client) = chain_api::Client::with_signer(&node, account.clone()).await else {
                continue;
            };
            let Ok(node_list) = client.list_nodes().await else { continue };
            break 'a (node_list, client);
        }
        anyhow::bail!("failed to fetch node list");
    };

    let mut stream = client.node_contract_event_stream().await;
    tokio::spawn(async move {
        loop {
            if let Ok(mut stream) = stream {
                let mut stream = std::pin::pin!(stream);
                while let Some(event) = stream.next().await {
                    _ = chain_events_tx.send(event).await;
                }
            }

            let Some(next) = others.next() else {
                log::error!("failed to reconnect to chain");
                std::process::exit(1);
            };

            let Ok(client) = chain_api::Client::with_signer(&next, account.clone()).await else {
                stream = Err(chain_api::Error::Other("failed to reconnect to chain".into()));
                continue;
            };

            stream = client.node_contract_event_stream().await;
        }
    });

    if is_new {
        let nonce = client.get_nonce().await.context("fetching nonce")? + nonce;
        client
            .join(keys.to_stored(), (exposed_address, port).into(), nonce)
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
        node_list: Vec<Stake>,
        stake_events: StakeEvents,
    ) -> anyhow::Result<Self> {
        let NodeConfig { port, ws_port, boot_nodes, idle_timeout, .. } = config;

        let local_key = libp2p::identity::Keypair::ed25519_from_bytes(keys.sign.pre_quantum())
            .context("deriving ed signature")?;
        let peer_id = local_key.public().to_peer_id();
        log::info!("peer id: {}", peer_id);

        let (sender, receiver) = topology_wrapper::channel();
        let behaviour = Behaviour {
            key_share: topology_wrapper::new(
                key_share::Behaviour::new(keys.enc.public_key()),
                sender.clone(),
            ),
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
                (p, StreamMuxerBox::new(topology_wrapper::muxer::new(m, sender)))
            }
            futures::future::Either::Right((p, m)) => {
                (p, StreamMuxerBox::new(topology_wrapper::muxer::new(m, sender)))
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
            .map(|stake| {
                let pk = unpack_node_id(stake.id)?;
                let addr = unpack_node_addr(stake.addr);
                Ok(Route::new(pk, addr))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        swarm.behaviour_mut().dht.table.bulk_insert(node_data);

        for boot_node in boot_nodes.0 {
            swarm.dial(boot_node).context("dialing a boot peer")?;
        }

        let (request_event_sink, request_events) = mpsc::channel(100);

        Ok(Self {
            context: Box::leak(Box::new(OwnedContext {
                profiles: Default::default(),
                online: Default::default(),
                chats: Default::default(),
                request_event_sink,
                local_peer_id: *swarm.local_peer_id(),
            })),
            swarm,
            request_events,
            clients: Default::default(),
            stake_events,
            client_router: handlers::router!(
                api::chat => {
                    rpcs::CREATE_CHAT => create.repl();
                    rpcs::ADD_MEMBER => add_member.repl().restore();
                    rpcs::KICK_MEMBER => kick_member.repl().restore();
                    rpcs::SEND_MESSAGE => send_message.repl().restore();
                    rpcs::FETCH_MESSAGES => fetch_messages.restore();
                    rpcs::FETCH_MEMBERS => fetch_members.restore();
                };
                api::profile => {
                    rpcs::CREATE_PROFILE => create.repl();
                    rpcs::SEND_MAIL => send_mail.repl().restore();
                    rpcs::READ_MAIL => read_mail.repl().restore();
                    rpcs::INSERT_TO_VAULT => insert_to_vault.repl().restore();
                    rpcs::REMOVE_FROM_VAULT => remove_from_vault.repl().restore();
                    rpcs::FETCH_PROFILE => fetch_keys.restore();
                    rpcs::FETCH_VAULT => fetch_vault.restore();
                    rpcs::FETCH_VAULT_KEY => fetch_vault_key.restore();
                };
            ),
            server_router: handlers::router!(
                api::chat => {
                    rpcs::CREATE_CHAT => create;
                    rpcs::ADD_MEMBER => add_member.restore();
                    rpcs::KICK_MEMBER => kick_member.restore();
                    rpcs::SEND_BLOCK => handle_message_block.restore().no_resp();
                    rpcs::FETCH_CHAT_DATA => fetch_chat_data;
                    rpcs::SEND_MESSAGE => send_message.restore();
                    rpcs::VOTE_BLOCK => vote.restore().no_resp();
                };
                api::profile => {
                    rpcs::CREATE_PROFILE => create;
                    rpcs::SEND_MAIL => send_mail.restore();
                    rpcs::READ_MAIL => read_mail.restore();
                    rpcs::INSERT_TO_VAULT => insert_to_vault.restore();
                    rpcs::REMOVE_FROM_VAULT => remove_from_vault.restore();
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
                fs::write(path, nk.to_bytes())?;
                return Ok((nk, true));
            }
            Err(e) => return Err(e),
        };

        let Some(nk) = NodeKeys::decode(&mut file.as_slice()) else {
            return Err(io::Error::other("invalid key file"));
        };

        Ok((nk, false))
    }

    fn handle_event(&mut self, event: SE) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Rpc(rpc::Event::Request(peer, id, body))) => {
                let Some((prefix, topic, body)) =
                    <(u8, Topic, Reminder)>::decode(&mut body.as_slice())
                else {
                    log::warn!("received invalid internal rpc request");
                    return;
                };

                let Some(missing_topic) = self.validate_topic(Some(topic)) else {
                    log::warn!("received request with invalid topic: {:?}", topic);
                    return;
                };

                self.server_router.handle(api::State {
                    req: Request { prefix, id, topic: Some(topic), body },
                    swarm: &mut self.swarm,
                    location: OnlineLocation::Remote(peer),
                    context: self.context,
                    missing_topic,
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
            SwarmEvent::Behaviour(_ev) => {}
            e => log::debug!("{e:?}"),
        }
    }

    fn handle_client_message(&mut self, id: PathId, req: io::Result<Vec<u8>>) {
        let req = match req {
            Ok(req) => req,
            Err(e) => {
                log::warn!("failed to read from client: {}", e);
                return;
            }
        };

        let Some(req) = chat_spec::Request::decode(&mut req.as_slice()) else {
            log::warn!("failed to decode client request: {:?}", req);
            return;
        };

        let Some(missing_topic) = self.validate_topic(req.topic) else {
            log::warn!("client sent message with invalid topic: {:?}", req.topic);
            return;
        };

        log::info!("received message from client: {:?} {:?}", req.id, req.prefix);

        if req.prefix == rpcs::SUBSCRIBE {
            let client = self.clients.iter_mut().find(|c| c.id == id).expect("no you don't");
            if let Some(topic) = req.topic
                && missing_topic.is_none()
            {
                log::info!("client subscribed to topic: {:?}", topic);
                match topic {
                    Topic::Profile(identity) => client.profile_sub = Some((identity, req.id)),
                    Topic::Chat(chat) => _ = client.subscriptions.insert(chat, req.id),
                }
                _ = client.inner.write((req.id, Ok::<(), ChatError>(())));
            } else {
                _ = client.inner.write((req.id, Err::<(), ChatError>(ChatError::NotFound)));
            }
        } else if req.prefix == rpcs::UNSUBSCRIBE {
            let client = self.clients.iter_mut().find(|c| c.id == id).expect("no you don't");
            if let Some(topic) = req.topic
                && missing_topic.is_none()
            {
                log::info!("client unsubscribed from topic: {:?}", topic);
                match topic {
                    Topic::Profile(_) => client.profile_sub = None,
                    Topic::Chat(chat) => _ = client.subscriptions.remove(&chat),
                }
            }
        } else {
            self.client_router.handle(api::State {
                req,
                swarm: &mut self.swarm,
                location: OnlineLocation::Local(id),
                context: self.context,
                missing_topic,
            });
        }
    }

    fn validate_topic(&mut self, topic: Option<Topic>) -> Option<Option<MissingTopic>> {
        let Some(topic) = topic else {
            return Some(None);
        };

        let us = *self.swarm.local_peer_id();
        if self
            .swarm
            .behaviour_mut()
            .dht
            .table
            .closest(topic.as_bytes())
            .take(REPLICATION_FACTOR.get() + 1)
            .all(|r| r.peer_id() != us)
        {
            return None;
        }

        Some(match topic {
            Topic::Profile(prf) if !self.context.profiles.contains_key(&prf) => {
                Some(MissingTopic::Profile(prf))
            }
            Topic::Chat(name) if let Entry::Vacant(v) = self.context.chats.entry(name) => {
                Some(MissingTopic::Chat {
                    name,
                    lock: v.insert(Default::default()).clone().try_write_owned().unwrap(),
                })
            }
            _ => None,
        })
    }

    fn handle_stake_event(&mut self, event: Result<chain_api::StakeEvent, chain_api::Error>) {
        let event = match event {
            Ok(event) => event,
            Err(e) => {
                log::warn!("failed to read from chain: {}", e);
                return;
            }
        };

        match event {
            chain_api::StakeEvent::Joined { identity, addr } => {
                let Ok(pk) = unpack_node_id(identity) else {
                    log::warn!("invalid node id");
                    return;
                };
                log::debug!("node joined the network: {pk:?}");
                let route = Route::new(pk, unpack_node_addr(addr));
                self.swarm.behaviour_mut().dht.table.insert(route);
            }
            chain_api::StakeEvent::Reclaimed { identity } => {
                let Ok(pk) = unpack_node_id(identity) else {
                    log::warn!("invalid node id");
                    return;
                };
                log::debug!("node left the network: {pk:?}");
                self.swarm
                    .behaviour_mut()
                    .dht
                    .table
                    .remove(libp2p::identity::PublicKey::from(pk).to_peer_id());
            }
            chain_api::StakeEvent::AddrChanged { identity, addr } => {
                let Ok(pk) = unpack_node_id(identity) else {
                    log::warn!("invalid node id");
                    return;
                };
                log::debug!("node changed address: {pk:?}");
                let route = Route::new(pk, unpack_node_addr(addr));
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
            handlers::Response::DontRespond(_) => {}
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
        topic: Topic,
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
                    } else {
                        log::warn!("failed to send rpc to {:?}: {}", peer, e);
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
            handlers::Response::DontRespond(_) => {}
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

            while let Poll::Ready((res, (OnlineLocation::Local(peer), id))) =
                self.client_router.poll(cx)
            {
                self.handle_client_router_response(peer, id, res);
                did_something = true;
            }

            while let Poll::Ready((res, (OnlineLocation::Remote(peer), id))) =
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
pub struct Behaviour {
    onion: topology_wrapper::Behaviour<onion::Behaviour>,
    dht: dht::Behaviour,
    rpc: topology_wrapper::Behaviour<rpc::Behaviour>,
    report: topology_wrapper::report::Behaviour,
    key_share: topology_wrapper::Behaviour<key_share::Behaviour>,
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
    subscriptions: HashMap<ChatName, CallId>,
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
        let (input_s, input_r) = mpsc::channel(100);
        let (output_s, output_r) = mpsc::channel(100);
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
    ReplRpc(Topic, Vec<u8>, Option<mpsc::Sender<(PeerId, rpc::Result<Vec<u8>>)>>),
}

pub struct OwnedContext {
    profiles: DashMap<Identity, Profile>,
    online: DashMap<Identity, OnlineLocation>,
    chats: DashMap<ChatName, Arc<RwLock<Chat>>>,
    request_event_sink: mpsc::Sender<RequestEvent>,
    local_peer_id: PeerId,
}

fn rpc<'a>(prefix: u8, topic: impl Into<Topic>, msg: impl Codec<'a>) -> Vec<u8> {
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

    async fn repl_rpc_no_resp<'a>(&self, topic: impl Into<Topic>, prefix: u8, msg: impl Codec<'a>) {
        let topic = topic.into();
        _ = self
            .request_event_sink
            .clone()
            .send(RequestEvent::ReplRpc(topic, rpc(prefix, topic, msg), None))
            .await;
    }

    async fn _send_rpc_no_resp<'a>(
        &self,
        topic: impl Into<Topic>,
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

    async fn repl_rpc<'a>(
        &self,
        topic: impl Into<Topic>,
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
        topic: impl Into<Topic>,
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
