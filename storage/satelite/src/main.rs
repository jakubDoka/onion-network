#![feature(slice_take)]
#![feature(never_type)]
use {
    self::storage::Storage,
    anyhow::Context as _,
    codec::Codec,
    component_utils::futures::StreamExt,
    libp2p::swarm::{NetworkBehaviour, SwarmEvent},
    rpc::CallId,
    std::{future::Future, net::Ipv4Addr},
    storage_spec::rpcs,
};

mod client;
mod node;
mod storage;

config::env_config! {
    struct Config {
        port: u16 = "8080",
        external_ip: Ipv4Addr = "127.0.0.1",
        identity_ed: config::Hex = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        metabase_root: String = "metabase",
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env();
    let db = Storage::new(&config.metabase_root)?;
    let satelite = Satelite::new(config, db)?;
    satelite.await?
}

pub struct Satelite {
    swarm: libp2p::Swarm<Behaviour>,
    router: handlers::Router<RouterContext>,
    pk: OurPk,
    context: Context,
}

impl Satelite {
    fn new(config: Config, store: Storage) -> anyhow::Result<Self> {
        let identity: libp2p::identity::ed25519::Keypair =
            libp2p::identity::ed25519::Keypair::try_from_bytes(&mut config.identity_ed.to_bytes())
                .context("invalid identity")?;

        let swarm = libp2p::SwarmBuilder::with_existing_identity(identity.clone().into())
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

        Ok(Self {
            swarm,
            pk: identity.public(),
            context: Box::leak(Box::new(OwnedContext { store })),
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
                    rpcs::GET_GC_META => get_gc_meta;
                };
            },
        })
    }

    fn swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        let beh = match event {
            SwarmEvent::Behaviour(beh) => beh,
            _ => return,
        };

        match beh {
            BehaviourEvent::Rpc(rpc::Event::Request(peer, id, body)) => {
                let Some(req) = storage_spec::Request::decode(&mut body.as_slice()) else {
                    log::warn!("invalid request from {:?}", peer);
                    return;
                };

                self.router.handle(State { req, id, context: self.context, pk: self.pk.clone() });
            }
            _ => {}
        }
    }

    fn router_event(&mut self, _event: (handlers::Response, CallId)) {}
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
    type RequestMeta = CallId;
    type State<'a> = State<'a>;

    fn prefix(state: &Self::State<'_>) -> usize {
        state.req.prefix as usize
    }

    fn meta(state: &Self::State<'_>) -> Self::RequestMeta {
        state.id
    }
}

struct State<'a> {
    req: storage_spec::Request<'a>,
    id: CallId,
    context: Context,
    pk: OurPk,
}

impl handlers::Context for State<'_> {
    fn request_body(&self) -> &[u8] {
        self.req.body.0
    }
}

struct OwnedContext {
    store: Storage,
}

type Context = &'static OwnedContext;
type OurPk = libp2p::identity::ed25519::PublicKey;

handlers::quick_impl_from_request! { State<'_> => [
    Context => |state| state.context,
    OurPk => |state| state.pk.clone(),
]}
