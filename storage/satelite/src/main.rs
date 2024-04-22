#![feature(if_let_guard)]
#![feature(slice_take)]
#![feature(never_type)]
use {
    self::storage::Storage,
    anyhow::Context as _,
    chain_api::{Mnemonic, NodeKeys},
    codec::Codec,
    libp2p::{
        core::{muxing::StreamMuxerBox, upgrade::Version, ConnectedPoint},
        futures::future::Either,
        multiaddr::Protocol,
        swarm::{NetworkBehaviour, SwarmEvent},
        Multiaddr, PeerId,
    },
    opfusk::{PeerIdExt, ToPeerId},
    rand_core::OsRng,
    rpc::CallId,
    std::{future::Future, net::Ipv4Addr, ops::DerefMut, task::Poll, time::Duration},
    storage_spec::rpcs,
};

mod client;
mod node;
mod storage;

type Context = &'static OwnedContext;

config::env_config! {
    struct Config {
        /// Port to listen on and publish to chain
        port: u16,
        /// where metadata is stored
        metabase_root: String,
        /// mnemonic to derive keys from
        mnemonic: Mnemonic,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let db = Storage::new(&config.metabase_root)?;
    let keys = NodeKeys::from_mnemonic(&config.mnemonic);
    let satelite = Satelite::new(config, db, keys)?;
    satelite.await?
}

pub struct Satelite {
    swarm: libp2p::Swarm<Behaviour>,
    router: handlers::Router<RouterContext>,
    context: Context,
}

impl Satelite {
    fn new(config: Config, store: Storage, keys: NodeKeys) -> anyhow::Result<Self> {
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
            Behaviour::default(),
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

        Ok(Self {
            swarm,
            context: Box::leak(Box::new(OwnedContext { store, keys })),
            router: handlers::router! {
                client => {
                    rpcs::REGISTER_CLIENT => register;
                    rpcs::ALLOCATE_FILE => allocate_file;
                    rpcs::DELETE_FILE => delete_file;
                    rpcs::ALLOCAET_BANDWIDTH => allocate_bandwidth;
                    rpcs::GET_FILE_HOLDERS => get_file_holders;
                };
                node => {
                    rpcs::REGISTER_NODE => register;
                    rpcs::HEARTBEAT => heartbeat;
                    rpcs::GET_GC_META => get_gc_meta;
                };
            },
        })
    }

    fn swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        let beh = match event {
            SwarmEvent::Behaviour(beh) => beh,
            SwarmEvent::ConnectionEstablished {
                peer_id,
                endpoint: ConnectedPoint::Listener { send_back_addr, .. },
                ..
            } if let Some(pk) = peer_id.try_to_hash() => {
                self.swarm.behaviour_mut().dht.table.insert(dht::Route::new(pk, send_back_addr));
                return;
            }
            _ => return,
        };

        match beh {
            BehaviourEvent::Rpc(rpc::Event::Request(origin, id, body)) => {
                let Some(req) = storage_spec::Request::decode(&mut body.as_slice()) else {
                    log::warn!("invalid request from {:?}", origin);
                    return;
                };

                let Some(addr) = self.swarm.behaviour().dht.table.get(origin).cloned() else {
                    log::warn!("no route to {:?}", origin);
                    return;
                };

                self.router.handle(State { req, id, origin, context: self.context, addr });
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
}

impl Future for Satelite {
    type Output = anyhow::Result<!>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use component_utils::field as f;

        while component_utils::Selector::new(self.deref_mut(), cx)
            .stream(f!(mut swarm), Self::swarm_event)
            .stream(f!(mut router), Self::router_event)
            .done()
        {}

        Poll::Pending
    }
}

#[derive(NetworkBehaviour, Default)]
pub struct Behaviour {
    dht: dht::Behaviour,
    rpc: rpc::Behaviour,
    streaming: streaming::Behaviour,
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
    id: CallId,
    origin: PeerId,
    addr: Multiaddr,
    context: Context,
}

impl handlers::Context for State<'_> {
    fn request_body(&self) -> &[u8] {
        self.req.body.0
    }
}

struct OwnedContext {
    store: Storage,
    keys: NodeKeys,
}

handlers::quick_impl_from_request! { State<'_> => [
    Context => |state| state.context,
    Multiaddr => |state| state.addr.clone(),
]}
