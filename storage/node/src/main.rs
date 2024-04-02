#![feature(never_type)]
use {
    anyhow::Context,
    libp2p::{futures::StreamExt, swarm::NetworkBehaviour},
    std::{future::Future, net::Ipv4Addr},
};

config::env_config! {
    struct Config {
        port: u16 = "8080",
        external_ip: Ipv4Addr = "127.0.0.1",
        identity_ed: config::Hex = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env();
    let satelite = Node::new(config)?;
    satelite.await?
}

struct Node {
    swarm: libp2p::Swarm<Behaviour>,
}

impl Node {
    fn new(config: Config) -> anyhow::Result<Self> {
        let identity: libp2p::identity::Keypair =
            libp2p::identity::ed25519::Keypair::try_from_bytes(&mut config.identity_ed.to_bytes())
                .context("invalid identity")?
                .into();

        let swarm = libp2p::SwarmBuilder::with_existing_identity(identity)
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

        Ok(Self { swarm })
    }

    fn swarm_event(&mut self, _event: libp2p::swarm::SwarmEvent<BehaviourEvent>) {}
}

impl Future for Node {
    type Output = anyhow::Result<!>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        while let std::task::Poll::Ready(Some(event)) = self.swarm.poll_next_unpin(cx) {
            self.swarm_event(event);
        }

        std::task::Poll::Pending
    }
}

#[derive(NetworkBehaviour, Default)]
struct Behaviour {
    rpc: rpc::Behaviour,
    streaming: streaming::Behaviour,
    dht: dht::Behaviour,
}
