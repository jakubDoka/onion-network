use {
    super::*,
    chat_spec::{Proof, *},
    component_utils::{crypto::ToProofContext, Protocol},
    libp2p::futures::{channel::mpsc, stream::FuturesUnordered, FutureExt},
    rand_core::OsRng,
    std::{fmt::Debug, usize},
};

#[tokio::test]
async fn repopulate_account() {
    let mut nodes = create_nodes(REPLICATION_FACTOR.get() + 1);
    let mut user = Account::new();
    let [mut stream, used] = Stream::new_test();
    nodes.iter_mut().next().unwrap().clients.push(used);

    stream.create_user(&mut nodes, &mut user).await;

    assert_nodes(&nodes, |node| node.storage.profiles.contains_key(&user.identity()));

    let target = nodes.iter_mut().next().unwrap();
    target.storage.profiles.clear();
    stream
        .test_req::<chat_spec::SendMail>(&mut nodes, (user.identity(), Reminder(&[0xff])), Ok(()))
        .await;

    assert_nodes(&nodes, |node| {
        node.storage.profiles.values().any(|p| unpack_mail(&p.mail).next().unwrap() == [0xff])
    });

    let target = nodes.iter_mut().next().unwrap();
    target.storage.profiles.clear();
    stream
        .test_req::<chat_spec::FetchVault>(&mut nodes, user.identity(), Ok((0, 0, Reminder(&[]))))
        .await;

    assert_nodes(&nodes, |node| node.storage.profiles.contains_key(&user.identity()));
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
    stream1.create_user(&mut nodes, &mut user).await;
    stream2.create_user(&mut nodes, &mut user2).await;

    stream1
        .test_req::<chat_spec::SendMail>(&mut nodes, (user2.identity(), Reminder(&[1])), Ok(()))
        .await;

    stream2
        .test_req::<chat_spec::ReadMail>(
            &mut nodes,
            user2.proof(chat_spec::Mail),
            Ok(Reminder(&[0, 1, 1])),
        )
        .await;

    stream2.test_req::<chat_spec::Subscribe>(&mut nodes, user2.identity().into(), Ok(())).await;

    futures::future::select(
        nodes.next(),
        std::pin::pin!(tokio::time::sleep(Duration::from_millis(100))),
    )
    .await;

    stream1
        .test_req::<chat_spec::SendMail>(
            &mut nodes,
            (user2.identity(), Reminder(&[2])),
            Err(SendMailError::SentDirectly),
        )
        .await;

    stream2.expect_event(&mut nodes, Reminder(&[2])).await;

    drop(stream2);

    stream1
        .test_req::<chat_spec::SendMail>(&mut nodes, (user2.identity(), Reminder(&[3])), Ok(()))
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
    nodes.iter_mut().last().unwrap().clients.push(used2);
    stream1.create_user(&mut nodes, &mut user).await;
    stream2.create_user(&mut nodes, &mut user2).await;

    let chat = ChatName::from("foo").unwrap();

    stream1.test_req::<CreateChat>(&mut nodes, (chat, user.identity()), Ok(())).await;
    stream1
        .test_req::<PerformChatAction>(
            &mut nodes,
            (user.proof(chat), ChatAction::AddUser(user2.identity())),
            Ok(()),
        )
        .await;

    const MESSAGE_SIZE: usize = 900;
    const MULTIPLIER: usize = 1;

    for i in 0..12 * MULTIPLIER {
        println!("i: {}", i);
        let cons = [i as u8; MESSAGE_SIZE / MULTIPLIER];
        stream1
            .test_req::<PerformChatAction>(
                &mut nodes,
                (user.proof(chat), ChatAction::SendMessage(Reminder(&cons))),
                Ok(()),
            )
            .await;
    }

    assert_nodes(&nodes, |s| s.storage.chats.get(&chat).unwrap().block_number == 2);

    for i in 0..6 * MULTIPLIER {
        // futures::future::select(
        //     nodes.next(),
        //     std::pin::pin!(tokio::time::sleep(Duration::from_millis(10))),
        // )
        // .await;

        println!("i: {}", i);
        let msg = [i as u8; MESSAGE_SIZE / MULTIPLIER];
        let body = (user.proof(chat), ChatAction::SendMessage(Reminder(&msg)));
        stream1.inner.write(PerformChatAction::rpc_id(CallId::whatever(), body)).unwrap();
        let msg = [i as u8 * 2; MESSAGE_SIZE / MULTIPLIER];
        let body = (user2.proof(chat), ChatAction::SendMessage(Reminder(&msg)));
        stream2.inner.write(PerformChatAction::rpc_id(CallId::whatever(), body)).unwrap();

        response::<PerformChatAction>(&mut nodes, &mut stream1, 1000, Ok(())).await;
        response::<PerformChatAction>(&mut nodes, &mut stream2, 1000, Ok(())).await;
    }

    assert_nodes(&nodes, |s| s.storage.chats.get(&chat).unwrap().block_number == 5);

    let target = nodes.iter_mut().next().unwrap();
    target.storage.chats.clear();

    stream1
        .test_req::<PerformChatAction>(
            &mut nodes,
            (user.proof(chat), ChatAction::SendMessage(Reminder(&[0xff]))),
            Ok(()),
        )
        .await;
}

impl Stream {
    async fn test_req<P: Protocol>(
        &mut self,
        nodes: &mut FuturesUnordered<Server>,
        body: P::Request<'_>,
        expected: ProtocolResult<'_, P>,
    ) where
        for<'a> ProtocolResult<'a, chat_spec::Repl<P>>: PartialEq + Debug,
    {
        self.inner.write((P::PREFIX, CallId::whatever(), body)).unwrap();

        response::<P>(nodes, self, 1000, expected).await;
    }

    async fn create_user(&mut self, nodes: &mut FuturesUnordered<Server>, user: &mut Account) {
        self.test_req::<CreateProfile>(
            nodes,
            (user.proof(&[]), user.enc.public_key().into_bytes()),
            Ok(()),
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

async fn response<P: Protocol>(
    nodes: &mut FuturesUnordered<Server>,
    stream: &mut Stream,
    tiemout_milis: u64,
    expected: ProtocolResult<'_, P>,
) where
    for<'a> ProtocolResult<'a, chat_spec::Repl<P>>: PartialEq + Debug,
{
    futures::select! {
        _ = nodes.select_next_some() => unreachable!(),
        res = stream.next().fuse() => {
            let res = res.unwrap().1.unwrap();
            {
                let (_, resp) = <(CallId, ProtocolResult<chat_spec::Repl<P>>)>::decode(&mut unsafe { std::mem::transmute(res.as_slice()) }).unwrap();
                assert_eq!(resp, expected.map_err(chat_spec::ReplError::Inner));
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

    fn proof<T: ToProofContext>(&mut self, context: T) -> Proof<T> {
        Proof::new(&self.sign, &mut self.nonce, context, OsRng)
    }

    fn identity(&self) -> Identity {
        crypto::hash::new(&self.sign.public_key())
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
    }
}

fn create_nodes(count: usize) -> FuturesUnordered<Server> {
    let node_data =
        (0..count).map(|_| (next_node_config(), NodeKeys::default())).collect::<Vec<_>>();

    let nodes = node_data
        .iter()
        .map(|(config, keys)| {
            (keys.to_stored(), (IpAddr::from(Ipv4Addr::LOCALHOST), config.port).into())
        })
        .collect::<Vec<(NodeData, NodeAddress)>>();

    node_data
        .into_iter()
        .map(|(config, keys)| {
            let (_, rx) = mpsc::channel(1);
            Server::new(config, keys, nodes.clone(), rx).unwrap()
        })
        .collect()
}
