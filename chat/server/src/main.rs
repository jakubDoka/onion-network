#![feature(type_alias_impl_trait)]
#![feature(iter_advance_by)]
#![feature(never_type)]
#![feature(lazy_cell)]
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
#![allow(unused_labels)]

use {
    crate::api::State,
    anyhow::Context as _,
    api::{chat, profile, FullReplGroup},
    arrayvec::ArrayVec,
    chain_api::{ChatStakeEvent, NodeIdentity, NodeKeys, NodeVec, StakeEvents},
    chat_spec::{
        ChatError, ChatName, Identity, Prefix, ReplVec, RequestHeader, ResponseHeader, Topic,
        REPLICATION_FACTOR,
    },
    clap::Parser,
    codec::{DecodeOwned, Encode},
    dashmap::{mapref::entry::Entry, DashMap, DashSet},
    dht::{Route, SharedRoutingTable},
    handlers::CallId,
    libp2p::{
        core::{multiaddr, muxing::StreamMuxerBox, upgrade::Version},
        futures::{
            channel::{mpsc, oneshot},
            future::{Either, JoinAll, TryJoinAll},
            stream::FuturesUnordered,
            AsyncReadExt, AsyncWriteExt, SinkExt, StreamExt, TryFutureExt,
        },
        swarm::{NetworkBehaviour, SwarmEvent},
        Multiaddr, PeerId,
    },
    onion::{key_share, EncryptedStream, PathId},
    opfusk::{PeerIdExt, ToPeerId},
    rand_core::OsRng,
    std::{
        collections::HashMap,
        future::Future,
        io,
        net::Ipv4Addr,
        ops::DerefMut,
        path::PathBuf,
        task::Poll,
        time::{Duration, Instant},
    },
    storage::Storage,
    topology_wrapper::BuildWrapped,
};

mod api;
mod db;
mod reputation;
mod storage;
#[cfg(test)]
mod tests;

const STREAM_POOL_STREAM_LIFETIME: Duration = Duration::from_secs(5);

type Context = &'static OwnedContext;
type SE = libp2p::swarm::SwarmEvent<<Behaviour as NetworkBehaviour>::ToSwarm>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let node_config = NodeConfig::parse();
    let keys = NodeKeys::from_mnemonic(&node_config.chain.mnemonic);
    let (node_list, stake_events) = node_config.chain.clone().connect_chat().await?;
    reputation::Rep::report_at_background(keys.identity(), node_config.chain.clone());

    Server::new(node_config, keys, node_list, stake_events).await?.await;

    Ok(())
}

type PooledStream = impl Future<Output = io::Result<(libp2p::Stream, RequestHeader, NodeIdentity)>>;

fn pool_stream(peer: NodeIdentity, mut stream: libp2p::Stream) -> PooledStream {
    async move {
        let mut buf = [0; std::mem::size_of::<RequestHeader>()];
        tokio::time::timeout(STREAM_POOL_STREAM_LIFETIME, stream.read_exact(&mut buf)).await??;
        Ok((stream, RequestHeader::from_array(buf), peer))
    }
}

#[derive(Parser)]
pub struct NodeConfig {
    /// The port to listen on and publish to chain
    port: u16,
    /// The port to listen on for websocket connections, clients expect `port + 1`
    ws_port: u16,
    /// Idle connection is dropped after ms of inactivity
    idle_timeout: u64,
    /// Rpc request is dropped after ms of delay
    rpc_timeout: u64,
    /// The path to node preserved data
    data_dir: PathBuf,
    #[clap(flatten)]
    chain: chain_api::EnvConfig,
}

struct Server {
    swarm: libp2p::swarm::Swarm<Behaviour>,
    context: Context,
    stake_events: StakeEvents<ChatStakeEvent>,
    stream_requests: mpsc::Receiver<(NodeIdentity, oneshot::Sender<libp2p::Stream>)>,
    penidng_stream_requests: HashMap<NodeIdentity, Vec<oneshot::Sender<libp2p::Stream>>>,
    recycled_streams: mpsc::Receiver<(NodeIdentity, libp2p::Stream)>,
    stream_pool: FuturesUnordered<PooledStream>,
}

impl Server {
    async fn new(
        config: NodeConfig,
        keys: NodeKeys,
        node_list: NodeVec,
        stake_events: StakeEvents<ChatStakeEvent>,
    ) -> anyhow::Result<Self> {
        use libp2p::core::Transport;

        let NodeConfig { port, ws_port, idle_timeout, .. } = config;

        let (sender, receiver) = topology_wrapper::channel();

        let sender_clone = sender.clone();
        let mut swarm = libp2p::Swarm::new(
            libp2p::tcp::tokio::Transport::default()
                .upgrade(Version::V1)
                .authenticate(opfusk::Config::new(OsRng, keys.sign))
                .multiplex(libp2p::yamux::Config::default())
                .or_transport(
                    libp2p::websocket::WsConfig::new(libp2p::tcp::tokio::Transport::default())
                        .upgrade(Version::V1)
                        .authenticate(opfusk::Config::new(OsRng, keys.sign))
                        .multiplex(libp2p::yamux::Config::default()),
                )
                .map(move |option, _| match option {
                    Either::Left((peer, stream)) => (
                        peer,
                        StreamMuxerBox::new(topology_wrapper::muxer::new(
                            stream,
                            sender_clone.clone(),
                        )),
                    ),
                    Either::Right((peer, stream)) => (
                        peer,
                        StreamMuxerBox::new(topology_wrapper::muxer::new(stream, sender_clone)),
                    ),
                })
                .boxed(),
            Behaviour {
                key_share: key_share::Behaviour::new(keys.enc.public_key())
                    .include_in_vis(sender.clone()),
                onion: onion::Config::new(keys.enc.into(), keys.sign.to_peer_id())
                    .max_streams(10)
                    .keep_alive_interval(Duration::from_secs(100))
                    .build()
                    .include_in_vis(sender.clone()),
                dht: dht::Behaviour::new(chain_api::filter_incoming),
                report: topology_wrapper::report::new(receiver),
                streaming: streaming::Behaviour::new(|| chat_spec::PROTO_NAME)
                    .include_in_vis(sender),
            },
            keys.sign.to_peer_id(),
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

        let node_data =
            node_list.into_iter().map(|(id, addr)| Route::new(id, chain_api::unpack_addr(addr)));
        swarm.behaviour_mut().dht.table.write().bulk_insert(node_data);

        let stream_requests = mpsc::channel(100);
        let recycled_streams = mpsc::channel(100);

        let context = Box::leak(Box::new(OwnedContext {
            storage: Storage::new(config.data_dir),
            not_found: Default::default(),
            online: Default::default(),
            chat_subs: Default::default(),
            clients: Default::default(),
            profile_subs: Default::default(),
            ongoing_recovery: Default::default(),
            stream_requests: stream_requests.0,
            recycled_streams: recycled_streams.0,
            stream_cache: StreamCache::default(),
            local_peer_id: swarm.local_peer_id().to_hash(),
            dht: swarm.behaviour_mut().dht.table,
        }));

        Ok(Self {
            context,
            stream_pool: Default::default(),
            swarm,
            stake_events,
            stream_requests: stream_requests.1,
            recycled_streams: recycled_streams.1,
            penidng_stream_requests: Default::default(),
        })
    }

    fn swarm_event(&mut self, event: SE) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Onion(onion::Event::InboundStream(
                stream,
                id,
            ))) => {
                self.context.clients.insert(id, Client::new(stream, self.context, id));
            }
            SwarmEvent::Behaviour(BehaviourEvent::Streaming(streaming::Event::OutgoingStream(
                peer,
                stream,
            ))) => {
                let Some(requests) = self.penidng_stream_requests.get_mut(&peer.to_hash()) else {
                    log::error!("we established stream with {peer} but we dont have any requests");
                    return;
                };

                let Some(sender) = requests.pop() else {
                    log::error!("we established stream with {peer} but we dont have any requests, though the entry exists");
                    return;
                };

                let Ok(stream) = stream.inspect_err(|e| {
                    log::warn!(
                        "peer {peer} is not reachable ({}): {e}",
                        self.swarm.local_peer_id() < &peer
                    )
                }) else {
                    return;
                };

                if sender.send(stream).is_err() {
                    log::warn!("client dropped the stream request");
                }
            }
            SwarmEvent::Behaviour(BehaviourEvent::Streaming(streaming::Event::IncomingStream(
                id,
                stream,
            ))) => {
                let ctx = self.context;
                tokio::spawn(async move {
                    if let Err(err) = Self::server_request(stream, ctx, id).await {
                        log::warn!("failed to handle incoming stream: {err}");
                    }
                });
            }
            SwarmEvent::Behaviour(_ev) => {}
            e => log::debug!("{e:?}"),
        }
    }

    fn stake_event(&mut self, event: ChatStakeEvent) {
        match event {
            ChatStakeEvent::Voted { source, target } => reputation::Rep::get().vote(source, target),
            ev => chain_api::stake_event(self, ev),
        }
    }

    fn stream_request(&mut self, (id, sender): (NodeIdentity, oneshot::Sender<libp2p::Stream>)) {
        self.swarm.behaviour_mut().streaming.create_stream(id.to_peer_id());
        self.penidng_stream_requests.entry(id).or_default().push(sender);
    }

    async fn server_request(mut stream: libp2p::Stream, cx: Context, id: PeerId) -> io::Result<()> {
        let identity = id.to_hash();

        let mut num_buf = [0u8; std::mem::size_of::<RequestHeader>()];
        stream.read_exact(&mut num_buf).await?;
        let header = RequestHeader::from_array(num_buf);

        Self::server_response(cx, (stream, header, identity)).await
    }

    async fn server_response(
        cx: Context,
        (stream, header, identity): (libp2p::Stream, RequestHeader, NodeIdentity),
    ) -> io::Result<()> {
        let len = header.get_len();

        let topic = Topic::decompress(header.topic);
        let repl_group =
            cx.dht.read().closest::<{ REPLICATION_FACTOR.get() + 1 }>(topic.as_bytes());

        if !repl_group.contains(&identity.into()) {
            reputation::Rep::get().rate(identity, 100);
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "not in replication group",
            ));
        }

        let state = State {
            location: OnlineLocation::Remote(identity),
            context: cx,
            id: header.call_id,
            topic,
            prefix: header.prefix,
        };

        api::server_router([0; 4], header.prefix, len, state, stream)
            .await
            .ok_or(io::ErrorKind::Unsupported)??
            .ok_or(io::ErrorKind::ConnectionAborted.into())
            .map(|s| _ = cx.recycled_streams.clone().try_send((identity, s)))
    }
}

impl Future for Server {
    type Output = !;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        use component_utils::field as f;

        let log_stake = |_: &mut Self, e| log::warn!("failed to read stake event: {e}");

        while component_utils::Selector::new(self.deref_mut(), cx)
            .stream(f!(mut swarm), Self::swarm_event)
            .try_stream(f!(mut stake_events), Self::stake_event, log_stake)
            .stream(f!(mut stream_requests), Self::stream_request)
            .stream(f!(mut recycled_streams), |this, (peer, stream)| {
                this.stream_pool.push(pool_stream(peer, stream));
            })
            .try_stream(
                f!(mut stream_pool),
                |s, dt| {
                    let cx = s.context;
                    tokio::spawn(async move {
                        if let Err(e) = Self::server_response(cx, dt).await {
                            log::warn!("failed to handle incoming stream: {e}");
                        }
                    });
                },
                |_, _| {},
            )
            .done()
        {}

        Poll::Pending
    }
}

impl AsMut<dht::Behaviour> for Server {
    fn as_mut(&mut self) -> &mut dht::Behaviour {
        &mut self.swarm.behaviour_mut().dht
    }
}

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    onion: topology_wrapper::Behaviour<onion::Behaviour>,
    dht: dht::Behaviour,
    report: topology_wrapper::report::Behaviour,
    key_share: topology_wrapper::Behaviour<key_share::Behaviour>,
    streaming: topology_wrapper::Behaviour<streaming::Behaviour>,
}

pub enum ClientEvent {
    Sub(CallId, Vec<u8>),
}

pub struct Client {
    events: mpsc::Sender<ClientEvent>,
}

impl Client {
    fn new(stream: EncryptedStream, context: Context, id: PathId) -> Self {
        let (events, recv) = mpsc::channel(30);
        tokio::spawn(async move {
            let Err(e) = run_client(id, stream, recv, context).await else { unreachable!() };
            log::warn!("client task failed with id {id:?}: {e}");
            context.clients.remove(&id);
        });
        Self { events }
    }
}

async fn run_client(
    path_id: PathId,
    mut stream: EncryptedStream,
    mut events: mpsc::Receiver<ClientEvent>,
    cx: Context,
) -> Result<!, io::Error> {
    async fn handle_event(
        stream: &mut EncryptedStream,
        event: ClientEvent,
    ) -> Result<(), io::Error> {
        match event {
            ClientEvent::Sub(cid, data) => {
                let header =
                    ResponseHeader { call_id: cid, len: (data.len() as u32).to_be_bytes() };
                stream.write_all(header.as_bytes()).await?;
                stream.write_all(&data).await?;
                stream.flush().await
            }
        }
    }

    async fn handle_request(
        path_id: PathId,
        stream: EncryptedStream,
        res: io::Result<()>,
        id: [u8; std::mem::size_of::<RequestHeader>()],
        cx: Context,
    ) -> Result<EncryptedStream, io::Error> {
        res?;

        let header = RequestHeader::from_array(id);
        let len = header.get_len();

        if len > u16::MAX as usize {
            log::warn!("peer {path_id:?} tried to send too big message");
            return Err(io::ErrorKind::InvalidInput.into());
        }

        let topic = Topic::decompress(header.topic);

        let repl_rgoup =
            cx.dht.read().closest::<{ REPLICATION_FACTOR.get() + 1 }>(topic.as_bytes());

        if !repl_rgoup.contains(&cx.local_peer_id.into()) {
            log::warn!(
                "peer {path_id:?} tried to access {topic:?} but it is not in the replication group, (prefix: {:?})",
                header.prefix,
            );
            return Err(io::ErrorKind::InvalidInput.into());
        }

        let state = State {
            location: OnlineLocation::Local(path_id),
            context: cx,
            id: header.call_id,
            topic,
            prefix: header.prefix,
        };

        api::client_router(header.call_id, header.prefix, len, state, stream)
            .await
            .ok_or(io::ErrorKind::PermissionDenied)
            .inspect_err(|_| {
                log::warn!("user accessed prefix '{:?}' that us not recognised", header.prefix)
            })??
            .ok_or(io::ErrorKind::ConnectionAborted.into())
    }

    let mut buf = [0u8; std::mem::size_of::<RequestHeader>()];
    loop {
        tokio::select! {
            event = events.select_next_some() => handle_event(&mut stream, event).await?,
            res = stream.read_exact(&mut buf) => {
                stream = handle_request(path_id, stream, res, buf, cx).await?;
                stream.flush().await?;
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OnlineLocation {
    Local(PathId),
    Remote(NodeIdentity),
}

const STREAM_CACHE_SIZE: usize = 10;

#[derive(Default)]
pub struct StreamCache {
    recycled: DashMap<NodeIdentity, ArrayVec<(libp2p::Stream, Instant), STREAM_CACHE_SIZE>>,
}

impl StreamCache {
    fn recycle(&self, peer: NodeIdentity, stream: libp2p::Stream) {
        _ = self.recycled.entry(peer).or_default().try_push((stream, Instant::now()));
    }

    fn reuse(&self, peer: NodeIdentity) -> Option<libp2p::Stream> {
        let mut slot = self.recycled.get_mut(&peer)?;
        slot.retain(|(_, time)| {
            time.elapsed() < STREAM_POOL_STREAM_LIFETIME - Duration::from_secs(2)
        });
        slot.pop().map(|(stream, _)| stream)
    }
}

unsafe impl Sync for StreamCache {}

// TODO: improve recovery mechanism
// TODO: switch to lmdb
// TODO: remove recovery locks, ommit response instead
pub struct OwnedContext {
    storage: Storage,
    not_found: DashSet<Topic>,
    online: DashMap<Identity, OnlineLocation>,
    chat_subs: DashMap<ChatName, HashMap<PathId, CallId>>,
    profile_subs: DashMap<Identity, (PathId, CallId)>,
    ongoing_recovery: DashSet<Topic>,
    clients: DashMap<PathId, Client>,
    stream_requests: mpsc::Sender<(NodeIdentity, oneshot::Sender<libp2p::Stream>)>,
    recycled_streams: mpsc::Sender<(NodeIdentity, libp2p::Stream)>,
    stream_cache: StreamCache,
    local_peer_id: NodeIdentity,
    dht: SharedRoutingTable,
}

impl OwnedContext {
    fn start_recovery(&'static self, topic: Topic) -> bool {
        if !self.ongoing_recovery.insert(topic) {
            return false;
        }

        tokio::spawn(async move {
            let res = match topic {
                Topic::Profile(profile) => profile::recover(self, profile).await,
                Topic::Chat(chat) => chat::recover(self, chat).await,
            };
            if res.is_err() {
                self.not_found.insert(topic);
            }
        });

        self.ongoing_recovery.remove(&topic);

        true
    }

    async fn push_chat_event(&self, topic: ChatName, ev: chat_spec::ChatEvent) {
        let Some(mut subscriptions) = self.chat_subs.get_mut(&topic) else { return };
        subscriptions.retain(|path_id, call_id| {
            let Some(mut client) = self.clients.get_mut(path_id) else { return false };
            let event = ClientEvent::Sub(*call_id, ev.to_bytes());
            client.events.try_send(event).is_ok()
        });
    }

    async fn push_profile_event(&self, for_who: Identity, mail: Vec<u8>) -> bool {
        let Entry::Occupied(ent) = self.profile_subs.entry(for_who) else { return false };
        let (path_id, call_id) = *ent.get();
        let Some(mut client) = self.clients.get_mut(&path_id) else { return false };

        let event = ClientEvent::Sub(call_id, mail.clone());
        if client.events.try_send(event).is_err() {
            ent.remove();
            return false;
        }

        true
    }

    async fn open_stream_with(&self, node: NodeIdentity) -> Option<libp2p::Stream> {
        if let Some(stream) = self.stream_cache.reuse(node) {
            return Some(stream);
        }

        let (sender, recv) = oneshot::channel();
        self.stream_requests.clone().send((node, sender)).await.ok()?;
        recv.await.ok()
    }

    async fn send_rpc<R: DecodeOwned>(
        &self,
        topic: impl Into<Topic>,
        peer: NodeIdentity,
        send_mail: Prefix,
        body: impl Encode,
    ) -> Result<R, ChatError> {
        let topic = topic.into();
        let mut stream = self.open_stream_with(peer).await.ok_or(ChatError::NoReplicator)?;
        let msg = &body.to_bytes();
        let header = RequestHeader {
            prefix: send_mail,
            call_id: [0; 4],
            topic: topic.compress(),
            len: (msg.len() as u32).to_be_bytes(),
        };
        stream.write_all(header.as_bytes()).await?;
        stream.write_all(msg).await?;
        stream.flush().await?;

        if std::mem::size_of::<R>() == 0 {
            self.stream_cache.recycle(peer, stream);
            return Ok(unsafe { std::mem::zeroed() });
        }

        let mut buf = [0; std::mem::size_of::<ResponseHeader>()];
        stream.read_exact(&mut buf).await?;
        let header = ResponseHeader::from_array(buf);
        let mut buf = vec![0; header.get_len()];
        stream.read_exact(&mut buf).await?;
        self.stream_cache.recycle(peer, stream);
        R::decode_exact(&buf).ok_or(ChatError::InvalidResponse)
    }

    async fn repl_rpc<R: DecodeOwned>(
        &self,
        topic: impl Into<Topic>,
        id: Prefix,
        body: impl Encode,
    ) -> Result<ReplVec<(NodeIdentity, R)>, ChatError> {
        self.repl_rpc_low(topic, id, &body.to_bytes()).await
    }

    async fn repl_rpc_low<R: DecodeOwned>(
        &self,
        topic: impl Into<Topic>,
        id: Prefix,
        msg: &[u8],
    ) -> Result<ReplVec<(NodeIdentity, R)>, ChatError> {
        let topic = topic.into();
        let Some(others) = self.get_others(topic) else { return Err(ChatError::NoReplicator) };

        let mut streams = others
            .iter()
            .map(|&peer| self.open_stream_with(peer.into()))
            .collect::<JoinAll<_>>()
            .await
            .into_iter()
            .zip(others.into_iter())
            .filter_map(|(stream, peer)| stream.map(|stream| (peer, stream)))
            .collect::<Vec<_>>();

        let header = RequestHeader {
            prefix: id,
            call_id: [0; 4],
            topic: topic.compress(),
            len: (msg.len() as u32).to_be_bytes(),
        };

        streams
            .iter_mut()
            .map(|(_, stream)| async move {
                stream.write_all(header.as_bytes()).await?;
                stream.write_all(msg).await
            })
            .collect::<JoinAll<_>>()
            .await;

        if std::mem::size_of::<R>() == 0 {
            for (id, stream) in streams {
                self.stream_cache.recycle(id.into(), stream);
            }
            return Ok(ReplVec::default());
        }

        Ok(streams
            .into_iter()
            .map(|(id, mut stream)| {
                tokio::time::timeout(Duration::from_secs(2), async move {
                    let mut buf = [0; std::mem::size_of::<ResponseHeader>()];
                    stream.read_exact(&mut buf).await?;
                    let header = ResponseHeader::from_array(buf);
                    let mut buf = vec![0; header.get_len()];
                    stream.read_exact(&mut buf).await?;
                    self.stream_cache.recycle(id.into(), stream);
                    Ok::<_, ChatError>((
                        id.into(),
                        R::decode_exact(&buf).ok_or(ChatError::InvalidResponse)?,
                    ))
                })
                .map_err(|_| ChatError::Timeout)
                .and_then(std::future::ready)
            })
            .collect::<TryJoinAll<_>>()
            .await?
            .into_iter()
            .collect())
    }

    fn get_others(&self, topic: impl Into<Topic>) -> Option<FullReplGroup> {
        self.get_others_for(topic, self.local_peer_id)
    }

    fn get_others_for(&self, topic: impl Into<Topic>, id: NodeIdentity) -> Option<FullReplGroup> {
        let topic = topic.into();
        let mut others =
            self.dht.read().closest::<{ REPLICATION_FACTOR.get() + 1 }>(topic.as_bytes());
        others.retain(|peer| peer != &id);
        if others.is_full() {
            return None;
        }
        Some(others.into_iter().map(Into::into).collect())
    }

    fn should_recover(&'static self, topic: Topic) -> bool {
        !match topic {
            Topic::Profile(profile) => self.storage.has_profile(profile),
            Topic::Chat(chat) => self.storage.has_chat(chat),
        }
    }
}
