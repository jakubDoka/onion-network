#![feature(if_let_guard)]
#![feature(slice_take)]
#![feature(never_type)]
use {
    self::storage::Storage,
    anyhow::Context as _,
    chain_api::{Mnemonic, NodeKeys},
    codec::Codec,
    component_utils::futures::StreamExt,
    libp2p::{
        core::ConnectedPoint,
        multiaddr::Protocol,
        swarm::{NetworkBehaviour, SwarmEvent},
        Multiaddr, PeerId,
    },
    rpc::CallId,
    std::{future::Future, net::Ipv4Addr},
    storage_spec::rpcs,
};

mod client;
mod node;
mod storage;

type Context = &'static OwnedContext;

config::env_config! {
    struct Config {
        port: u16 = "8080",
        external_ip: Ipv4Addr = "127.0.0.1",
        metabase_root: String = "metabase",
        mnemonic: Mnemonic,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env();
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
        let identity: libp2p::identity::ed25519::Keypair =
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
            } if let Some(pk) = dht::try_peer_id_to_ed(peer_id) => {
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
        while let std::task::Poll::Ready(Some(event)) = self.swarm.poll_next_unpin(cx) {
            self.swarm_event(event);
        }

        while let std::task::Poll::Ready(event) = self.router.poll(cx) {
            self.router_event(event);
        }

        std::task::Poll::Pending
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
