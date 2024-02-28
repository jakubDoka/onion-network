use {
    crate::{EncryptedStream, PathId},
    aes_gcm::aead::OsRng,
    component_utils::AsocStream,
    dht::Route,
    futures::{stream::SelectAll, FutureExt, StreamExt},
    libp2p::{
        core::{multiaddr::Protocol, upgrade::Version, Transport},
        identity::{ed25519, Keypair, PeerId},
        swarm::{NetworkBehaviour, SwarmEvent},
    },
    rand::seq::SliceRandom,
    std::{collections::HashSet, io, net::Ipv4Addr, pin::Pin, time::Duration, usize},
};

const CONNECTION_TIMEOUT: Duration = Duration::from_millis(1000);

fn init() {
    let _ = env_logger::builder().is_test(true).try_init();
}

fn setup_nodes<const COUNT: usize>(
    ports: [u16; COUNT],
) -> [libp2p::swarm::Swarm<crate::Behaviour>; COUNT] {
    init();
    ports.map(|port| {
        let keypair = Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id();
        let secret = crate::KeyPair::new(OsRng);
        let transport = libp2p::tcp::tokio::Transport::default()
            .upgrade(Version::V1)
            .authenticate(libp2p::noise::Config::new(&keypair).unwrap())
            .multiplex(libp2p::yamux::Config::default())
            .boxed();
        let mut swarm = libp2p::swarm::Swarm::new(
            transport,
            crate::Behaviour::new(
                crate::Config::new(Some(secret), peer_id)
                    .keep_alive_interval(CONNECTION_TIMEOUT)
                    .dial(false),
            ),
            peer_id,
            libp2p::swarm::Config::with_tokio_executor()
                .with_idle_connection_timeout(CONNECTION_TIMEOUT),
        );
        swarm.add_external_address(
            libp2p::core::Multiaddr::empty()
                .with(Protocol::Ip4([0, 0, 0, 0].into()))
                .with(Protocol::Tcp(port)),
        );

        swarm
            .listen_on(
                libp2p::core::Multiaddr::empty()
                    .with(Protocol::Ip4([0, 0, 0, 0].into()))
                    .with(Protocol::Tcp(port)),
            )
            .unwrap();

        ports.map(|other| {
            if other >= port {
                return;
            }

            swarm
                .dial(
                    libp2p::core::Multiaddr::empty()
                        .with(Protocol::Ip4([127, 0, 0, 1].into()))
                        .with(Protocol::Tcp(other)),
                )
                .unwrap();
        });

        swarm
    })
}

async fn open_path(
    swarms: &mut [libp2p::swarm::Swarm<crate::Behaviour>],
) -> (EncryptedStream, EncryptedStream) {
    let Ok([_, path @ ..]): Result<[_; 4], _> = swarms
        .iter()
        .map(|s| (s.behaviour().config().secret.clone(), s.behaviour().config().current_peer_id))
        .collect::<Vec<_>>()
        .try_into()
    else {
        panic!("failed to create path")
    };

    swarms[0].behaviour_mut().open_path(path.map(|(k, i)| (k.unwrap().public_key(), i)));

    let mut input = None;
    let mut output = None;
    loop {
        let (e, id, ..) = futures::future::select_all(swarms.iter_mut().map(|s| s.next())).await;
        match e.unwrap() {
            SwarmEvent::Behaviour(crate::Event::InboundStream(s, ..)) => input = Some(s),
            SwarmEvent::Behaviour(crate::Event::OutboundStream(s, ..)) => {
                output = Some(s.unwrap().0)
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
async fn test_routing() {
    let mut swarms = setup_nodes([8800, 8801, 8802, 8803]);
    let (mut input, mut output) = open_path(&mut swarms).await;

    input.write_bytes(b"hello").unwrap();
    let r = loop {
        let events = futures::future::select_all(swarms.iter_mut().map(|s| s.next()));
        let e = futures::select! {
            (e, ..) = events.fuse() => e,
            _ = input.select_next_some() => continue,
            r = output.select_next_some() => break r,
        };
        log::debug!("{:?}", e.unwrap());
    };

    assert_eq!(&r.unwrap(), b"hello");
}

#[tokio::test]
async fn test_timeout() {
    let mut swarms = setup_nodes([8804, 8805, 8806, 8807]);

    swarms.reverse();
    swarms.array_chunks_mut().for_each(|[a, b]| std::mem::swap(a, b));

    let (mut input, mut output) = open_path(&mut swarms).await;

    input.write(b"hello").unwrap();

    let mut disconnected = 0;
    let mut timeout = Box::pin(tokio::time::sleep(CONNECTION_TIMEOUT * 1));

    while disconnected != 6 {
        let events = futures::future::select_all(swarms.iter_mut().map(|s| s.next()));
        let (e, i) = futures::select! {
            (e, i, ..) = events.fuse() => (e, i),
            _ = Pin::new(&mut timeout).fuse() => panic!("{disconnected} nodes disconnected"),
            r = output.select_next_some() => {
                let msg = r.unwrap();
                input.write(&msg).unwrap();
                continue;
            },
        };

        match e.unwrap() {
            SwarmEvent::Behaviour(crate::Event::SearchRequest(to)) => {
                swarms[i].behaviour_mut().report_unreachable(to);
            }
            SwarmEvent::ConnectionClosed { .. } => disconnected += 1,
            e => log::info!("{e:?}"),
        }
    }
}

#[tokio::test]
async fn test_missing_route() {
    async fn perform(index: usize) {
        let mut swarms = setup_nodes([8808, 8809, 8810, 8811].map(|p| p + index as u16 * 4));
        let Ok([_, mut path @ ..]): Result<[_; 4], _> = swarms
            .iter()
            .map(|s| {
                (s.behaviour().config().secret.clone(), s.behaviour().config().current_peer_id)
            })
            .collect::<Vec<_>>()
            .try_into()
        else {
            panic!("failed to create path")
        };

        path[index].1 = PeerId::random();

        swarms[0].behaviour_mut().open_path(path.map(|(k, i)| (k.unwrap().public_key(), i)));

        loop {
            let (e, id, ..) =
                futures::future::select_all(swarms.iter_mut().map(|s| s.next())).await;
            match e.unwrap() {
                SwarmEvent::Behaviour(crate::Event::SearchRequest(to)) => {
                    swarms[id].behaviour_mut().report_unreachable(to);
                }
                SwarmEvent::Behaviour(crate::Event::OutboundStream(r, ..)) => {
                    r.unwrap_err();
                    break;
                }
                e => log::debug!("{id} {e:?}"),
            }
        }
    }

    futures::future::join_all((0..3).map(perform)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn settle_down() {
    init();
    let server_count = 10;
    let client_count = 20;
    let max_open_streams_server = 100;
    let concurrent_routes = 4;
    let max_open_streams_client = 4;
    let first_port = 8900;
    let spacing = Duration::from_millis(0);
    let keep_alive_interval = Duration::from_secs(5);

    let kps = (0..server_count).map(|_| ed25519::Keypair::generate()).collect::<Vec<_>>();
    let pks = kps.iter().map(|k| k.public()).collect::<Vec<_>>();

    let mut router = dht::Behaviour::default();
    router.table.bulk_insert(pks.into_iter().enumerate().map(|(i, k)| {
        let addr = libp2p::core::Multiaddr::empty()
            .with(Protocol::Ip4(Ipv4Addr::LOCALHOST))
            .with(Protocol::Tcp(first_port + i as u16));
        Route::new(k, addr)
    }));

    let (swarms, node_data): (Vec<_>, Vec<_>) = kps
        .into_iter()
        .map(Keypair::from)
        .enumerate()
        .map(|(i, kp)| {
            let peer_id = kp.public().to_peer_id();
            let secret = crate::KeyPair::new(OsRng);

            let transport = libp2p::tcp::tokio::Transport::default()
                .upgrade(Version::V1)
                .authenticate(libp2p::noise::Config::new(&kp).unwrap())
                .multiplex(libp2p::yamux::Config::default())
                .boxed();

            let beh = SDBehaviour {
                onion: crate::Behaviour::new(
                    crate::Config::new(Some(secret.clone()), peer_id)
                        .keep_alive_interval(keep_alive_interval),
                ),
                dht: router.clone(),
            };

            let mut swarm = libp2p::swarm::Swarm::new(
                transport,
                beh,
                peer_id,
                libp2p::swarm::Config::with_tokio_executor()
                    .with_idle_connection_timeout(keep_alive_interval),
            );

            swarm
                .listen_on(
                    libp2p::core::Multiaddr::empty()
                        .with(Protocol::Ip4([0, 0, 0, 0].into()))
                        .with(Protocol::Tcp(first_port + i as u16)),
                )
                .unwrap();

            (swarm, (peer_id, secret.public_key()))
        })
        .unzip();

    fn handle_packet(
        id: PathId,
        packet: io::Result<Vec<u8>>,
        streams: &mut SelectAll<AsocStream<PathId, EncryptedStream>>,
        counter: &mut usize,
    ) {
        let Ok(packet) = packet.inspect_err(|e| log::error!("closing stream with error: {e}"))
        else {
            return;
        };

        *counter += 1;
        if *counter % 1000 == 0 {
            log::info!("received {counter} packets");
        }

        let stream = streams.iter_mut().find(|s| s.assoc == id).unwrap();
        stream.inner.write_bytes(&packet).unwrap();
    }

    tokio::spawn(async move {
        for mut swarm in swarms {
            tokio::time::sleep(spacing).await;
            tokio::spawn(async move {
                use {crate::Event as OE, SDBehaviourEvent as BE, SwarmEvent as SE};

                let mut streams = SelectAll::<AsocStream<PathId, EncryptedStream>>::new();
                let mut counter = 0;
                loop {
                    let ev = futures::select! {
                        ev = swarm.select_next_some() => ev,
                        (id, packet) = streams.select_next_some() => { handle_packet(id, packet, &mut streams, &mut counter); continue; },
                    };

                    match ev {
                        SE::Behaviour(BE::Onion(OE::InboundStream(stream, pid))) => {
                            if streams.len() > max_open_streams_server {
                                log::info!("too many open streams");
                                continue;
                            }

                            streams.push(AsocStream::new(stream, pid));
                        }
                        e => log::debug!("{e:?}"),
                    }
                }
            });
        }
    });

    let clients = (0..client_count)
        .map(|_| {
            let kp = Keypair::generate_ed25519();
            let peer_id = kp.public().to_peer_id();

            let transport = libp2p::tcp::tokio::Transport::default()
                .upgrade(Version::V1)
                .authenticate(libp2p::noise::Config::new(&kp).unwrap())
                .multiplex(libp2p::yamux::Config::default())
                .boxed();

            let beh = SDBehaviour {
                onion: crate::Behaviour::new(
                    crate::Config::new(None, peer_id).keep_alive_interval(Duration::from_secs(5)),
                ),
                dht: router.clone(),
            };

            libp2p::swarm::Swarm::new(
                transport,
                beh,
                peer_id,
                libp2p::swarm::Config::with_tokio_executor()
                    .with_idle_connection_timeout(keep_alive_interval),
            )
        })
        .collect::<SelectAll<_>>();

    for mut swarm in clients {
        tokio::time::sleep(spacing).await;
        let node_data = node_data.clone();
        tokio::spawn(async move {
            use {crate::Event as OE, SDBehaviourEvent as BE, SwarmEvent as SE};

            let mut streams = SelectAll::<AsocStream<PathId, EncryptedStream>>::new();
            let mut pending_routes = HashSet::new();
            let mut counter = 0;
            loop {
                if pending_routes.len() < concurrent_routes
                    && streams.len() < max_open_streams_client
                {
                    let mut rng = &mut rand::thread_rng();
                    let to_dail: [_; 3] = node_data
                        .choose_multiple(&mut rng, node_data.len())
                        .map(|(id, pk)| (*pk, *id))
                        .take(3)
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap();
                    let id = swarm.behaviour_mut().onion.open_path(to_dail);
                    pending_routes.insert(id);
                }

                let ev = futures::select! {
                    (id, packet) = streams.select_next_some() => {
                        handle_packet(id, packet, &mut streams, &mut counter);
                        continue;
                    },
                    ev = swarm.select_next_some() => ev,
                };

                match ev {
                    SE::Behaviour(BE::Onion(OE::OutboundStream(stream, id, ..))) => {
                        if let Ok((mut stream, ..)) = stream {
                            stream.write_bytes(b"hello").unwrap();
                            streams.push(AsocStream::new(stream, id));
                        } else {
                            log::error!("failed to open stream {}", stream.unwrap_err());
                        }
                        pending_routes.remove(&id);
                    }
                    e => log::debug!("{e:?}"),
                }
            }
        });
    }

    std::future::pending().await
}

#[derive(NetworkBehaviour)]
struct SDBehaviour {
    onion: crate::Behaviour,
    dht: dht::Behaviour,
}
