#![feature(map_try_insert)]
#![feature(type_alias_impl_trait)]
#![feature(result_flattening)]
#![feature(never_type)]
#![feature(slice_take)]

use {
    anyhow::Context as _,
    chain_api::{Mnemonic, NodeKeys},
    codec::Codec,
    component_utils::PacketReader,
    libp2p::{
        futures::{
            channel::{mpsc, oneshot},
            stream::FuturesUnordered,
            FutureExt, SinkExt,
        },
        multiaddr::Protocol,
        swarm::{NetworkBehaviour, SwarmEvent},
        Multiaddr, PeerId,
    },
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
        port: u16,
        satelites: config::List<config::Hex>,
        mnemonic: Mnemonic,
        storage_dir: String,
        disk_limit_gb: u64,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let storage = storage::Storage::new(&config.storage_dir)?;
    let keys = NodeKeys::from_mnemonic(&config.mnemonic);
    let satelite = Node::new(config, storage, keys)?;
    satelite.await?
}

struct Node {
    swarm: libp2p::Swarm<Behaviour>,
    context: Context,
    router: handlers::Router<RouterContext>,
    request_events: mpsc::Receiver<RequestEvent>,
    pending_streams:
        HashMap<(CallId, PeerId), Result<oneshot::Sender<libp2p::Stream>, libp2p::Stream>>,
    stream_negots: FuturesUnordered<StreamIdentification>,
}

impl Node {
    fn new(config: Config, storage: storage::Storage, keys: NodeKeys) -> anyhow::Result<Self> {
        let identity =
            libp2p::identity::ed25519::Keypair::try_from_bytes(&mut keys.sign.pre_quantum())
                .context("invalid identity")?;

        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(identity.clone().into())
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )?
            .with_behaviour(|_| Behaviour::default())?
            .with_swarm_config(|c| {
                c.with_idle_connection_timeout(std::time::Duration::from_secs(60))
            })
            .build();

        swarm
            .listen_on(
                Multiaddr::empty()
                    .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
                    .with(Protocol::Tcp(config.port)),
            )
            .context("oprning listener")?;

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

    fn stream_negot(&mut self, event: io::Result<(StreamKind, libp2p::Stream, PeerId)>) {
        let Ok((kind, stream, peer)) =
            event.inspect_err(|e| log::warn!("stream negotiation failed: {e:#}"))
        else {
            return;
        };

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
        use handlers::field as f;

        while handlers::Selector::new(self.deref_mut(), cx)
            .stream(f!(mut swarm), Self::swarm_event)
            .stream(f!(mut router), Self::router_event)
            .stream(f!(mut request_events), Self::request_event)
            .stream(f!(mut stream_negots), Self::stream_negot)
            .done()
        {}

        Poll::Pending
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
