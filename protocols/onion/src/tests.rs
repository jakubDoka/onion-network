use {
    crate::EncryptedStream,
    aes_gcm::aead::OsRng,
    futures::{future::Either, AsyncReadExt, AsyncWriteExt, FutureExt, StreamExt},
    libp2p::{
        core::{multiaddr::Protocol, upgrade::Version, Transport},
        identity::PeerId,
        swarm::{NetworkBehaviour, SwarmEvent},
    },
    opfusk::ToPeerId,
    std::{net::Ipv4Addr, time::Duration},
};

const CONNECTION_TIMEOUT: Duration = Duration::from_millis(1000);

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[derive(NetworkBehaviour)]
struct Beh {
    onion: crate::Behaviour,
    dht: dht::Behaviour,
}

fn setup_nodes<const COUNT: usize>(ports: [u16; COUNT]) -> [libp2p::swarm::Swarm<Beh>; COUNT] {
    init();

    let keys = (0..COUNT).map(|_| crypto::sign::Keypair::new(OsRng)).collect::<Vec<_>>();

    let mut keys_iter = keys.iter();
    ports.map(|port| {
        let keypair = keys_iter.next().unwrap().clone();
        let peer_id = keypair.to_peer_id();
        let secret = crate::Keypair::new(OsRng);
        let transport = libp2p::tcp::tokio::Transport::default()
            .upgrade(Version::V1)
            .authenticate(opfusk::Config::new(OsRng, keypair))
            .multiplex(libp2p::yamux::Config::default())
            .boxed();
        let mut swarm = libp2p::swarm::Swarm::new(
            transport,
            Beh {
                onion: crate::Behaviour::new(
                    crate::Config::new(Some(secret.clone()), peer_id)
                        .keep_alive_interval(CONNECTION_TIMEOUT),
                ),
                dht: dht::Behaviour::default(),
            },
            peer_id,
            libp2p::swarm::Config::with_tokio_executor()
                .with_idle_connection_timeout(CONNECTION_TIMEOUT),
        );

        swarm
            .listen_on(
                libp2p::core::Multiaddr::empty()
                    .with(Protocol::Ip4([0, 0, 0, 0].into()))
                    .with(Protocol::Tcp(port)),
            )
            .unwrap();

        for (port, key) in ports.iter().zip(keys.iter()) {
            let addr = libp2p::core::Multiaddr::empty()
                .with(Protocol::Ip4(Ipv4Addr::LOCALHOST))
                .with(Protocol::Tcp(*port));
            let router = dht::Route::new(key.public_key().identity(), addr);
            swarm.behaviour_mut().dht.table.write().insert(router);
        }

        swarm
    })
}

async fn open_path(swarms: &mut [libp2p::swarm::Swarm<Beh>]) -> (EncryptedStream, EncryptedStream) {
    let Ok([_, path @ ..]): Result<[_; 4], _> = swarms
        .iter()
        .map(|s| {
            (
                s.behaviour().onion.config().secret.clone(),
                s.behaviour().onion.config().current_peer_id,
            )
        })
        .collect::<Vec<_>>()
        .try_into()
    else {
        panic!("failed to create path")
    };

    swarms[0].behaviour_mut().onion.open_path(path.map(|(k, i)| (k.unwrap().public_key(), i)));

    let mut input = None;
    let mut output = None;
    loop {
        let (e, id, ..) = futures::future::select_all(swarms.iter_mut().map(|s| s.next())).await;
        match e.unwrap() {
            SwarmEvent::Behaviour(BehEvent::Onion(crate::Event::InboundStream(s, ..))) => {
                input = Some(s)
            }
            SwarmEvent::Behaviour(BehEvent::Onion(crate::Event::OutboundStream(s, ..))) => {
                output = Some(s.unwrap())
            }
            e => log::debug!("{id} {e:?}"),
        }

        if input.is_some() && output.is_some() {
            let (Some(input), Some(output)) = (input, output) else {
                unreachable!();
            };
            break (input, output);
        }
    }
}

#[tokio::test]
async fn test_routing_big_p() {
    let mut swarms = setup_nodes([8700, 8701, 8702, 8703]);
    let (mut input, mut output) = open_path(&mut swarms).await;

    let iters = 100;
    const SIZE: usize = 1024 * 32;

    tokio::spawn(async move {
        let buf = [0; SIZE];
        for _ in 0..iters {
            input.write_all(&buf).await.unwrap();
        }
        input.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await; // I really cant do much about this
    });

    tokio::spawn(async move {
        loop {
            futures::future::select_all(swarms.iter_mut().map(|s| s.next())).await;
        }
    });

    let mut buf = [0; SIZE];
    for i in 0..iters {
        output.read_exact(&mut buf).await.inspect_err(|_| println!("{i}")).unwrap();
    }
}

#[tokio::test]
async fn test_routing() {
    let mut swarms = setup_nodes([8800, 8801, 8802, 8803]);
    let (mut input, mut output) = open_path(&mut swarms).await;

    input.write_all(b"hello").await.unwrap();
    log::info!("sent hello");
    let mut buf = [0; 5];
    let mut flushed = false;
    loop {
        let flush = if flushed {
            Either::Right(futures::future::pending())
        } else {
            Either::Left(input.flush())
        };
        let events = futures::future::select_all(swarms.iter_mut().map(|s| s.next()));
        let e = futures::select! {
            (e, ..) = events.fuse() => e,
            _ = flush.fuse() => {
                flushed = true;
                continue;
            },
            r = output.read_exact(&mut buf).fuse() => break r.unwrap(),
        };
        log::info!("{:?}", e.unwrap());
    }

    assert_eq!(&buf, b"hello");
}

#[tokio::test]
async fn test_missing_route() {
    async fn perform(index: usize) {
        let mut swarms = setup_nodes([8808, 8809, 8810, 8811].map(|p| p + index as u16 * 4));
        let Ok([_, mut path @ ..]): Result<[_; 4], _> = swarms
            .iter()
            .map(|s| {
                (
                    s.behaviour().onion.config().secret.clone().unwrap().public_key(),
                    s.behaviour().onion.config().current_peer_id,
                )
            })
            .collect::<Vec<_>>()
            .try_into()
        else {
            panic!("failed to create path")
        };

        path[index].1 = PeerId::random();

        swarms[0].behaviour_mut().onion.open_path(path);

        loop {
            let (e, id, ..) =
                futures::future::select_all(swarms.iter_mut().map(|s| s.next())).await;
            match e.unwrap() {
                SwarmEvent::Behaviour(BehEvent::Onion(crate::Event::OutboundStream(r, ..))) => {
                    r.unwrap_err();
                    break;
                }
                e => log::debug!("{id} {e:?}"),
            }
        }
    }

    futures::future::join_all((0..3).map(perform)).await;
}
