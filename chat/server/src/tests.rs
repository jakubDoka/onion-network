use {
    super::*,
    chain_api::NodeData,
    chat_spec::*,
    crypto::proof::Proof,
    libp2p::futures::{stream::FuturesUnordered, FutureExt},
    rand_core::OsRng,
    std::{fmt::Debug, usize},
};

type Result<T> = std::result::Result<T, ChatError>;

#[tokio::test]
async fn repopulate_account() {
    _ = env_logger::builder().is_test(true).try_init();

    let mut nodes = create_nodes(REPLICATION_FACTOR.get() + 1);
    let mut user = Account::new();
    let [mut stream, used] = Stream::new_test();
    nodes.iter_mut().next().unwrap().clients.push(used);

    stream.create_profile(&mut nodes, &mut user).await;

    assert_nodes(&nodes, |node| node.context.profiles.contains_key(&user.identity()));

    let target = nodes.iter_mut().next().unwrap();
    target.context.profiles.clear();
    stream
        .test_req_simple(
            &mut nodes,
            rpcs::SEND_MAIL,
            user.identity(),
            (user.identity(), Reminder(&[0xff])),
            Ok(()),
        )
        .await;

    assert_nodes(&nodes, |node| {
        node.context.profiles.iter().any(|p| unpack_mail(&p.mail).next().unwrap() == [0xff])
    });

    let target = nodes.iter_mut().next().unwrap();
    target.context.profiles.clear();
    stream
        .test_req_simple(
            &mut nodes,
            rpcs::FETCH_VAULT,
            user.identity(),
            user.identity(),
            Ok((0u32, 0u32, Reminder(&[]))),
        )
        .await;

    assert_nodes(&nodes, |node| node.context.profiles.contains_key(&user.identity()));
}

#[tokio::test]
async fn direct_messaging() {
    let mut nodes = create_nodes(REPLICATION_FACTOR.get() + 1);

    let mut user = Account::new();
    let mut user2 = Account::new();
    let [mut stream1, used] = Stream::new_test();
    let [mut stream2, used2] = Stream::new_test();

    nodes.iter_mut().next().unwrap().clients.push(used);
    nodes.iter_mut().last().unwrap().clients.push(used2);
    stream1.create_profile(&mut nodes, &mut user).await;
    stream2.create_profile(&mut nodes, &mut user2).await;

    stream1
        .test_req_simple(
            &mut nodes,
            rpcs::SEND_MAIL,
            user2.identity(),
            (user2.identity(), Reminder(&[1])),
            Ok(()),
        )
        .await;

    stream2
        .test_req_simple(
            &mut nodes,
            rpcs::READ_MAIL,
            user2.identity(),
            user2.proof(chat_spec::Mail),
            Ok(Reminder(&[0, 1, 1])),
        )
        .await;

    stream2
        .test_req(
            &mut nodes,
            rpcs::SUBSCRIBE,
            Topic::Profile(user2.identity()),
            Topic::Profile(user2.identity()),
            (),
        )
        .await;

    futures::future::select(
        nodes.next(),
        std::pin::pin!(tokio::time::sleep(Duration::from_millis(100))),
    )
    .await;

    stream1
        .test_req_simple(
            &mut nodes,
            rpcs::SEND_MAIL,
            user2.identity(),
            (user2.identity(), Reminder(&[2])),
            Err::<(), _>(ChatError::SentDirectly),
        )
        .await;

    stream2.expect_event(&mut nodes, Reminder(&[2])).await;

    drop(stream2);

    stream1
        .test_req_simple(
            &mut nodes,
            rpcs::SEND_MAIL,
            user2.identity(),
            (user2.identity(), Reminder(&[3])),
            Ok(()),
        )
        .await;
}

#[tokio::test]
async fn message_block_finalization() {
    _ = env_logger::builder().is_test(true).try_init();

    let mut nodes = create_nodes(REPLICATION_FACTOR.get() + 1);

    let mut user = Account::new();
    let mut user2 = Account::new();
    let [mut stream1, used] = Stream::new_test();
    let [mut stream2, used2] = Stream::new_test();

    nodes.iter_mut().next().unwrap().clients.push(used);
    let target = nodes.iter_mut().last().unwrap();
    target.clients.push(used2);
    let peer = *target.swarm.local_peer_id();
    stream1.create_profile(&mut nodes, &mut user).await;
    stream2.create_profile(&mut nodes, &mut user2).await;

    let chat = ChatName::from("foo").unwrap();

    stream1
        .test_req_simple(&mut nodes, rpcs::CREATE_CHAT, chat, (chat, user.identity()), Ok(()))
        .await;
    stream1
        .test_req_simple(
            &mut nodes,
            rpcs::ADD_MEMBER,
            chat,
            (user.proof(chat), user2.identity()),
            Ok(()),
        )
        .await;

    const MESSAGE_SIZE: usize = 100;
    const MULTIPLIER: usize = 1;

    for i in 0..100 * MULTIPLIER {
        let cons = [i as u8; MESSAGE_SIZE / MULTIPLIER];
        stream1
            .test_req_simple(
                &mut nodes,
                rpcs::SEND_MESSAGE,
                chat,
                user.proof(Reminder(&cons)),
                Ok(()),
            )
            .await;

        log::info!("message sent {i}");

        _ = tokio::time::timeout(Duration::from_millis(30), nodes.next()).await;
    }

    assert_nodes(&nodes, |s| {
        s.context.chats.get(&chat).unwrap().value().try_read().unwrap().number == 2
    });

    let topic = Some(Topic::Chat(chat));

    for i in 0..50 * MULTIPLIER {
        let msg = [i as u8; MESSAGE_SIZE / MULTIPLIER];
        let body = user.proof(Reminder(&msg));
        stream1.inner.write((rpcs::SEND_MESSAGE, CallId::new(), topic, body)).unwrap();
        let msg = [(i as u8).wrapping_mul(6); MESSAGE_SIZE / MULTIPLIER];
        let body = (user2.proof(chat), Reminder(&msg));
        stream2.inner.write((rpcs::SEND_MESSAGE, CallId::new(), topic, body)).unwrap();

        response_simple(&mut nodes, &mut stream1, 1000, Ok(())).await;
        response_simple(&mut nodes, &mut stream2, 1000, Ok(())).await;

        log::info!("message sent {i}");

        //_ = tokio::time::timeout(Duration::from_millis(300), nodes.next()).await;
    }

    assert_nodes(&nodes, |s| {
        s.context.chats.get(&chat).unwrap().value().try_read().unwrap().number == 6
    });

    let target = nodes.iter_mut().find(|s| *s.swarm.local_peer_id() == peer).unwrap();
    target.context.chats.clear();

    stream2
        .test_req_simple(
            &mut nodes,
            rpcs::SEND_MESSAGE,
            chat,
            user.proof(Reminder(&[0xff])),
            Ok(()),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn message_flooding() {
    _ = env_logger::builder().is_test(true).format_timestamp(None).try_init();

    let mut nodes = create_nodes(REPLICATION_FACTOR.get() + 1);

    let mut streams = nodes
        .iter_mut()
        .take(5)
        .map(|node| {
            let [stream, used] = Stream::new_test();
            node.clients.push(used);
            (stream, Account::new())
        })
        .collect::<Vec<_>>();

    for (stream, user) in &mut streams {
        stream.create_profile(&mut nodes, user).await;
    }

    let chat = ChatName::from("foo").unwrap();

    let ((some_stream, some_user), others) = streams.split_first_mut().unwrap();

    some_stream
        .test_req_simple(&mut nodes, rpcs::CREATE_CHAT, chat, some_user.identity(), Ok(()))
        .await;

    for (_, user) in others.iter_mut() {
        some_stream
            .test_req_simple(
                &mut nodes,
                rpcs::ADD_MEMBER,
                chat,
                (some_user.proof(chat), user.identity(), Member::best()),
                Ok(()),
            )
            .await;
        log::info!("member added");
    }

    log::info!("chat created");
    _ = tokio::time::timeout(Duration::from_millis(100), nodes.next()).await;

    const MESSAGE_SIZE: usize = 200;
    //const MODULO: usize = 40;

    for (i, mut node) in nodes.into_iter().enumerate() {
        if i == 0 {
            tokio::task::spawn(async move {
                loop {
                    _ = tokio::time::timeout(Duration::from_secs(1), &mut node).await;
                    node.context.chats.clear();
                    log::info!("cleared");
                }
            });
        } else {
            tokio::task::spawn(node);
        }
    }

    let topic = Some(Topic::Chat(chat));
    for (mut stream, mut user) in streams {
        tokio::task::spawn(async move {
            loop {
                let msg = [0; MESSAGE_SIZE];
                let body = user.proof(Reminder(&msg[0..MESSAGE_SIZE]));
                if stream.inner.write((rpcs::SEND_MESSAGE, CallId::new(), topic, body)).is_none() {
                    break;
                }
                stream.next().await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
    }

    tokio::time::sleep(Duration::from_secs(100)).await;
}

impl Stream {
    async fn test_req<'a, R: Codec<'a> + PartialEq + Debug>(
        &mut self,
        nodes: &mut FuturesUnordered<Server>,
        prefix: u8,
        topic: impl Into<Option<Topic>>,
        body: impl Codec<'a>,
        expected: R,
    ) {
        self.inner.write((prefix, CallId::new(), topic.into(), body)).unwrap();
        response(nodes, self, 1000, expected).await;
    }

    async fn test_req_simple<'a, R: Codec<'a> + PartialEq + Debug>(
        &mut self,
        nodes: &mut FuturesUnordered<Server>,
        prefix: u8,
        topic: impl Into<Topic>,
        body: impl Codec<'a>,
        expected: Result<R>,
    ) {
        self.test_req(nodes, prefix, Some(topic.into()), body, expected).await;
    }

    async fn create_profile(&mut self, nodes: &mut FuturesUnordered<Server>, user: &mut Account) {
        self.test_req(
            nodes,
            rpcs::CREATE_PROFILE,
            Topic::Profile(user.identity()),
            (user.proof(crypto::Hash::default()), "", user.enc.public_key()),
            Ok::<(), ChatError>(()),
        )
        .await;
    }

    async fn expect_event<'a, T: Codec<'a> + PartialEq + Debug>(
        &mut self,
        nodes: &mut FuturesUnordered<Server>,
        expected: T,
    ) {
        futures::select! {
            _ = nodes.select_next_some() => unreachable!(),
            res = self.next().fuse() => {
                let res = res.unwrap().1.unwrap();
                {
                    let (_, resp) = <(CallId, T)>::decode(&mut unsafe { std::mem::transmute(res.as_slice()) }).unwrap();
                    assert_eq!(resp, expected);
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(1000)).fuse() => {
                panic!("timeout")
            }
        }
    }
}

async fn response_simple<'a, R: PartialEq + Debug + Codec<'a>>(
    nodes: &mut FuturesUnordered<Server>,
    stream: &mut Stream,
    tiemout_milis: u64,
    expected: Result<R>,
) {
    response(nodes, stream, tiemout_milis, expected).await;
}

async fn response<'a, R: PartialEq + Debug + Codec<'a>>(
    nodes: &mut FuturesUnordered<Server>,
    stream: &mut Stream,
    tiemout_milis: u64,
    expected: R,
) {
    futures::select! {
        _ = nodes.select_next_some() => unreachable!(),
        res = stream.next().fuse() => {
            let res = res.unwrap().1.unwrap();
            {
                log::debug!("res: {:?} {:?}", res, std::any::type_name::<R>());
                let (_, resp) = <(CallId, R)>::decode(&mut unsafe { std::mem::transmute(res.as_slice()) }).unwrap();
                assert_eq!(resp, expected);
            }
        }
        _ = tokio::time::sleep(Duration::from_millis(tiemout_milis)).fuse() => {
            panic!("timeout")
        }
    }
}

#[track_caller]
fn assert_nodes(nodes: &FuturesUnordered<Server>, mut predicate: impl FnMut(&Server) -> bool) {
    assert!(nodes.iter().filter(|e| predicate(e)).count() > REPLICATION_FACTOR.get() / 2);
}

struct Account {
    sign: crypto::sign::Keypair,
    enc: crypto::enc::Keypair,
    nonce: u64,
}

impl Account {
    fn new() -> Self {
        Self {
            sign: crypto::sign::Keypair::new(OsRng),
            enc: crypto::enc::Keypair::new(OsRng),
            nonce: 0,
        }
    }

    fn proof<'a, T: Codec<'a>>(&mut self, context: T) -> Proof<T> {
        Proof::new(&self.sign, &mut self.nonce, context, OsRng)
    }

    fn identity(&self) -> Identity {
        crypto::hash::new(self.sign.public_key())
    }
}

fn next_node_config() -> NodeConfig {
    static PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    let port = PORT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    NodeConfig {
        port: port * 2 + 5000,
        ws_port: port * 2 + 1 + 5000,
        key_path: Default::default(),
        boot_nodes: config::List::default(),
        idle_timeout: 1000,
        rpc_timeout: 10000,
    }
}

fn create_nodes(count: usize) -> FuturesUnordered<Server> {
    let node_data =
        (0..count).map(|_| (next_node_config(), NodeKeys::default())).collect::<Vec<_>>();

    let nodes = node_data
        .iter()
        .map(|(config, keys)| {
            let NodeData { id, .. } = keys.to_stored();
            Stake {
                id,
                addr: (IpAddr::from(Ipv4Addr::LOCALHOST), config.port).into(),
                ..Stake::fake()
            }
        })
        .collect::<Vec<_>>();

    node_data
        .into_iter()
        .map(|(config, keys)| {
            let (_, rx) = mpsc::channel(1);
            Server::new(config, keys, nodes.clone(), rx).unwrap()
        })
        .collect()
}
