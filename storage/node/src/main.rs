#![feature(map_try_insert)]
#![feature(type_alias_impl_trait)]
#![feature(result_flattening)]
#![feature(never_type)]
#![feature(slice_take)]

use {
    anyhow::Context as _,
    chain_api::{Mnemonic, NodeKeys, NodeVec, SateliteStakeEvent, StakeEvents},
    codec::Codec,
    component_utils::PacketReader,
    dht::Route,
    libp2p::{
        core::{muxing::StreamMuxerBox, upgrade::Version},
        futures::{
            channel::{mpsc, oneshot},
            future::Either,
            stream::FuturesUnordered,
            FutureExt, SinkExt,
        },
        multiaddr::Protocol,
        swarm::{NetworkBehaviour, SwarmEvent},
        Multiaddr, PeerId,
    },
    opfusk::ToPeerId,
    rand_core::OsRng,
    rpc::CallId,
    std::{
        collections::{hash_map, HashMap},
        future::Future,
        io,
        net::Ipv4Addr,
        ops::DerefMut,
        task::Poll,
        time::Duration,
    },
    storage_spec::{rpcs, StreamKind},
};

mod client;
mod satelite;
mod storage;

type Context = &'static OwnedContext;

config::env_config! {
    struct Config {
        /// Port to listen on
        port: u16,
        /// List of satelites to register at
        satelites: config::List<config::Hex>,
        /// Mnemonic to derive keys from
        mnemonic: Mnemonic,
        /// Directory to store data in
        storage_dir: String,
        /// Maximum disk usage in GB
        disk_limit_gb: u64,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let storage = storage::Storage::new(&config.storage_dir)?;
    let keys = NodeKeys::from_mnemonic(&config.mnemonic);
    let (satelites, events) =
        chain_api::EnvConfig::from_env()?.connect_satelite(&keys, false).await?;
    let satelite = Node::new(config, storage, keys, events, satelites)?;
    satelite.await?
}

struct Node {
    swarm: libp2p::Swarm<Behaviour>,
    context: Context,
    router: handlers::Router<RouterContext>,
    request_events: mpsc::Receiver<RequestEvent>,
    pending_streams:
        HashMap<(CallId, PeerId), Result<oneshot::Sender<libp2p::Stream>, libp2p::Stream>>,
    stake_events: StakeEvents<SateliteStakeEvent>,
    stream_negots: FuturesUnordered<StreamIdentification>,
}

impl Node {
    fn new(
        config: Config,
        storage: storage::Storage,
        keys: NodeKeys,
        stake_events: StakeEvents<SateliteStakeEvent>,
        satelites: NodeVec,
    ) -> anyhow::Result<Self> {
        use libp2p::core::Transport;

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
                .map(|option, _| match option {
                    Either::Left((peer, stream)) => (peer, StreamMuxerBox::new(stream)),
                    Either::Right((peer, stream)) => (peer, StreamMuxerBox::new(stream)),
                })
                .boxed(),
            Behaviour {
                dht: dht::Behaviour::new(chain_api::filter_incoming),
                ..Default::default()
            },
            keys.sign.to_peer_id(),
            libp2p::swarm::Config::with_tokio_executor()
                .with_idle_connection_timeout(Duration::from_secs(10)),
        );

        swarm
            .listen_on(
                Multiaddr::empty()
                    .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
                    .with(Protocol::Tcp(config.port)),
            )
            .context("oprning listener")?;

        let node_data =
            satelites.into_iter().map(|(id, s)| Route::new(id, chain_api::unpack_addr(s)));

        swarm.behaviour_mut().dht.table.bulk_insert(node_data);

        let (sd, rc) = mpsc::channel(100);

        Ok(Self {
            swarm,
            context: Box::leak(Box::new(OwnedContext { request_events: sd, keys, storage })),
            router: handlers::router! {
                client => {
                    rpcs::STORE_FILE => store_file;
                    rpcs::READ_FILE => read_file;
                };
                satelite => {
                    rpcs::GET_PIECE_PROOF => get_piece;
                };
            },
            request_events: rc,
            pending_streams: Default::default(),
            stake_events,
            stream_negots: Default::default(),
        })
    }

    fn swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        let beh = match event {
            SwarmEvent::Behaviour(beh) => beh,
            _ => return,
        };

        match beh {
            BehaviourEvent::Rpc(rpc::Event::Request(origin, id, body)) => {
                let Some(req) = storage_spec::Request::decode(&mut body.as_slice()) else {
                    log::warn!("invalid request from {:?}", origin);
                    return;
                };

                self.router.handle(State { req, id, origin, context: self.context });
            }
            BehaviourEvent::Streaming(streaming::Event::IncomingStream(pid, stream)) => {
                self.stream_negots.push(identify_stream(pid, stream));
            }
            _ => {}
        }
    }

    fn router_event(&mut self, (resp, (call, peer)): (handlers::Response, (CallId, PeerId))) {
        match resp {
            handlers::Response::Success(msg) | handlers::Response::Failure(msg) => {
                self.swarm.behaviour_mut().rpc.respond(peer, call, msg);
            }
            handlers::Response::DontRespond(_) => {}
        }
    }

    fn request_event(&mut self, event: RequestEvent) {
        match event {
            RequestEvent::ExpectStream(peer, id, chan) => {
                match self.pending_streams.entry((id, peer)) {
                    hash_map::Entry::Occupied(o) => {
                        let Err(stream) = o.remove() else {
                            log::error!("we are already expecting stream with this is, which is impossible but ok");
                            return;
                        };
                        _ = chan.send(stream);
                    }
                    hash_map::Entry::Vacant(v) => _ = v.insert(Ok(chan)),
                }
            }
        }
    }

    fn stream_negot(&mut self, (kind, stream, peer): (StreamKind, libp2p::Stream, PeerId)) {
        match kind {
            StreamKind::RequestStream(cid) => match self.pending_streams.entry((cid, peer)) {
                hash_map::Entry::Occupied(o) => {
                    let Ok(sender) = o.remove() else {
                        log::warn!("duplicate stream from remote");
                        return;
                    };
                    _ = sender.send(stream);
                }
                hash_map::Entry::Vacant(v) => _ = v.insert(Err(stream)),
            },
        }
    }
}

impl Future for Node {
    type Output = anyhow::Result<!>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Self::Output> {
        use component_utils::field as f;

        let log_negot = |_: &mut Self, e| log::warn!("stream negotiation failed: {e}");
        let log_stake = |_: &mut Self, e| log::error!("stake event stream failure: {e}");

        while component_utils::Selector::new(self.deref_mut(), cx)
            .stream(f!(mut swarm), Self::swarm_event)
            .stream(f!(mut router), Self::router_event)
            .stream(f!(mut request_events), Self::request_event)
            .try_stream(f!(mut stream_negots), Self::stream_negot, log_negot)
            .try_stream(f!(mut stake_events), chain_api::stake_event, log_stake)
            .done()
        {}

        Poll::Pending
    }
}

impl AsMut<dht::Behaviour> for Node {
    fn as_mut(&mut self) -> &mut dht::Behaviour {
        &mut self.swarm.behaviour_mut().dht
    }
}

#[derive(NetworkBehaviour, Default)]
struct Behaviour {
    rpc: rpc::Behaviour,
    streaming: streaming::Behaviour,
    dht: dht::Behaviour,
}

enum RouterContext {}

impl handlers::RouterContext for RouterContext {
    type RequestMeta = (CallId, PeerId);
    type State<'a> = State<'a>;

    fn prefix(state: &Self::State<'_>) -> usize {
        state.req.prefix as usize
    }

    fn meta(state: &Self::State<'_>) -> Self::RequestMeta {
        (state.id, state.origin)
    }
}

struct State<'a> {
    req: storage_spec::Request<'a>,
    origin: PeerId,
    context: Context,
    id: CallId,
}

impl handlers::Context for State<'_> {
    fn request_body(&self) -> &[u8] {
        self.req.body.0
    }
}

handlers::quick_impl_from_request!(State<'_> => [
    Context => |state| state.context,
    CallId => |state| state.id,
    PeerId => |state| state.origin,
]);

struct OwnedContext {
    storage: storage::Storage,
    request_events: mpsc::Sender<RequestEvent>,
    keys: NodeKeys,
}

impl OwnedContext {
    async fn establish_stream(&self, peer: PeerId, cid: CallId) -> anyhow::Result<libp2p::Stream> {
        let (sd, rc) = oneshot::channel();
        self.request_events
            .clone()
            .send(RequestEvent::ExpectStream(peer, cid, sd))
            .await
            .context("sending stream expect requets")?;
        // TODO: add timeout
        rc.await.context("receiving the stream")
    }
}

enum RequestEvent {
    ExpectStream(PeerId, CallId, oneshot::Sender<libp2p::Stream>),
}

type StreamIdentification = impl Future<Output = io::Result<(StreamKind, libp2p::Stream, PeerId)>>;

fn identify_stream(peer: PeerId, mut stream: libp2p::Stream) -> StreamIdentification {
    let task = async move {
        let kind = PacketReader::default().next_packet_as::<StreamKind>(&mut stream).await?;
        Ok((kind, stream, peer))
    };
    tokio::time::timeout(Duration::from_secs(10), task)
        .map(|v| v.map_err(io::Error::other).flatten())
}
