use {
    crate::{chain_node, min_nodes, MailVariants, RawResponse, UserKeys, Vault},
    anyhow::Context,
    chain_api::NodeIdentity,
    chat_spec::*,
    codec::{Decode, DecodeOwned, Encode},
    crypto::{
        enc,
        proof::{Nonce, Proof},
    },
    dht::Route,
    instant::Duration,
    libp2p::{
        core::upgrade::Version,
        futures::{
            channel::{mpsc, oneshot},
            AsyncReadExt, AsyncWriteExt, FutureExt, SinkExt, StreamExt,
        },
        swarm::{NetworkBehaviour, SwarmEvent},
        *,
    },
    onion::{EncryptedStream, PathId},
    opfusk::{PeerIdExt, ToPeerId},
    rand::{rngs::OsRng, seq::IteratorRandom},
    std::{
        cell::RefCell,
        collections::{BTreeMap, HashMap},
        convert::Infallible,
        future::Future,
        io,
        ops::DerefMut,
        pin,
        rc::Rc,
        task::Poll,
    },
};

pub type CallId = [u8; 4];

fn next_call_id() -> CallId {
    static mut COUNTER: u32 = 0;
    unsafe {
        let id = COUNTER.to_be_bytes();
        COUNTER = COUNTER.wrapping_add(1);
        id
    }
}

pub struct Node {
    swarm: Swarm<Behaviour>,
    pending_streams: HashMap<PathId, oneshot::Sender<(NodeIdentity, EncryptedStream)>>,
    requests: mpsc::Receiver<NodeRequest>,
}

impl Node {
    pub async fn new(
        keys: UserKeys,
        mut wboot_phase: impl FnMut(BootPhase),
    ) -> anyhow::Result<(Self, Vault, NodeHandle, Nonce, Nonce)> {
        macro_rules! set_state { ($($t:tt)*) => {_ = wboot_phase(BootPhase::$($t)*)}; }

        set_state!(FetchNodesAndProfile);

        let mut swarm = libp2p::Swarm::new(
            libp2p::websocket_websys::Transport::default()
                .upgrade(Version::V1)
                .authenticate(opfusk::Config::new(OsRng, keys.sign))
                .multiplex(libp2p::yamux::Config::default())
                .boxed(),
            Behaviour {
                onion: onion::Config::default()
                    .keep_alive_interval(Duration::from_secs(100))
                    .build(),
                ..Default::default()
            },
            keys.sign.to_peer_id(),
            libp2p::swarm::Config::with_wasm_executor()
                .with_idle_connection_timeout(std::time::Duration::from_secs(10)),
        );

        let (commands_rx, requests) = mpsc::channel(10);
        let handle = NodeHandle::new(commands_rx, swarm.behaviour_mut().chat_dht.table);
        let chain_api = chain_node(keys.name).await?;
        let node_request = chain_api.list_chat_nodes();
        let satelite_request = chain_api.list_satelite_nodes();
        let profile_request = chain_api.get_profile_by_name(username_to_raw(keys.name));
        let (node_data, satelite_data, profile_hash) =
            futures::try_join!(node_request, satelite_request, profile_request)?;
        let profile_hash = profile_hash.context("profile not found")?;
        let profile = keys.to_identity();

        anyhow::ensure!(
            profile_hash.sign == profile.sign && profile_hash.enc == profile.enc,
            "profile hash does not match our account"
        );

        set_state!(InitiateConnection);

        let node_count = node_data.len();
        let tolerance = 0;
        set_state!(CollecringKeys(
            node_count - swarm.behaviour_mut().key_share.keys.len() - tolerance
        ));

        let nodes = node_data.into_iter().map(|(id, addr)| {
            let addr = chain_api::unpack_addr_offset(addr, 1);
            Route::new(id, addr.with(multiaddr::Protocol::Ws("/".into())))
        });
        swarm.behaviour_mut().chat_dht.table.write().bulk_insert(nodes);

        let satelites = satelite_data.into_iter().map(|(id, addr)| {
            let addr = chain_api::unpack_addr_offset(addr, 1);
            Route::new(id, addr.with(multiaddr::Protocol::Ws("/".into())))
        });
        swarm.behaviour_mut().satelite_dht.table.write().bulk_insert(satelites);

        let routes = swarm
            .behaviour_mut()
            .chat_dht
            .table
            .read()
            .iter()
            .map(Route::peer_id)
            .collect::<Vec<_>>();
        for route in routes {
            _ = swarm.dial(route);
        }

        _ = crate::timeout(
            async {
                loop {
                    match swarm.select_next_some().await {
                        SwarmEvent::Behaviour(BehaviourEvent::KeyShare(..)) => {
                            let remining =
                                node_count - swarm.behaviour_mut().key_share.keys.len() - tolerance;
                            set_state!(CollecringKeys(remining));
                            if remining == 0 || swarm.behaviour_mut().key_share.keys.len() >= 8 {
                                break;
                            }
                        }
                        e => log::debug!("{:?}", e),
                    }
                }
            },
            Duration::from_secs(10),
        )
        .await;

        let nodes = &swarm.behaviour_mut().key_share.keys;
        anyhow::ensure!(
            nodes.len() >= min_nodes(),
            "not enough nodes in network, needed {}, got {}",
            min_nodes(),
            nodes.len(),
        );

        let beh = swarm.behaviour_mut();

        set_state!(ProfileOpen);
        let route = pick_route(profile_hash.sign, &beh.key_share.keys, beh.chat_dht.table)
            .context("cannot find route to profile")?;
        let pid = swarm.behaviour_mut().onion.open_path(route);
        let (mut profile_stream, profile_stream_peer) = loop {
            match swarm.select_next_some().await {
                SwarmEvent::Behaviour(BehaviourEvent::Onion(onion::Event::OutboundStream(
                    stream,
                    id,
                ))) if id == pid => break stream.context("opening profile route")?,
                SwarmEvent::ConnectionClosed {
                    peer_id,
                    connection_id,
                    endpoint,
                    num_established,
                    cause,
                } => {
                    log::debug!(
                        "connection closed: {:?} {:?} {:?} {:?} {}",
                        peer_id,
                        connection_id,
                        endpoint,
                        num_established,
                        cause.unwrap()
                    );
                }
                e => log::debug!("{:?}", e),
            }
        };

        set_state!(VaultLoad);

        let (mut vault_nonce, mail_action, vault) =
            match crate::send_request::<(Nonce, Nonce, BTreeMap<crypto::Hash, Vec<u8>>)>(
                &mut profile_stream,
                rpcs::FETCH_VAULT,
                Topic::Profile(profile_hash.sign),
                (),
            )
            .await
            {
                Ok((vn, m, v)) => (vn + 1, m + 1, v),
                Err(e) => {
                    log::debug!("cannot access vault: {e} {:?}", profile_hash.sign);
                    Default::default()
                }
            };

        log::debug!("{:?}", vault);

        let vault = if vault.is_empty() && vault_nonce == 0 {
            set_state!(ProfileCreate);
            let proof = Proof::new(&keys.sign, &mut vault_nonce, crypto::Hash::default(), OsRng);
            crate::send_request(
                &mut profile_stream,
                rpcs::CREATE_PROFILE,
                Topic::Profile(profile_hash.sign),
                (proof, "", keys.enc.public_key()),
            )
            .await
            .context("creating account")?;

            Vault::default()
        } else {
            Vault::deserialize(vault, keys.vault)
        };
        let _ = vault.theme.apply();

        let id = profile_stream_peer.to_hash();
        let sub = Subscription::new(profile_stream, handle.clone(), id);
        handle.subs.borrow_mut().insert(id, sub);

        set_state!(ChatRun);

        Ok((
            Self { swarm, pending_streams: Default::default(), requests },
            vault,
            handle,
            vault_nonce,
            mail_action.max(1),
        ))
    }

    fn swarm_event(&mut self, event: SE) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Onion(onion::Event::OutboundStream(
                stream,
                id,
            ))) => {
                if let Some(tx) = self.pending_streams.remove(&id) {
                    if let Ok((stream, id)) = stream {
                        tx.send((id.to_hash(), stream)).ok();
                    }
                }
            }
            e => log::debug!("{:?}", e),
        }
    }

    fn request(&mut self, req: NodeRequest) {
        match req {
            NodeRequest::Subscribe(topic, tx) => {
                let beh = self.swarm.behaviour_mut();
                let route = pick_route(topic, &beh.key_share.keys, beh.chat_dht.table).unwrap();
                let id = beh.onion.open_path(route);
                self.pending_streams.insert(id, tx);
            }
        }
    }
}

impl Future for Node {
    type Output = Result<Infallible, ()>;

    fn poll(
        mut self: pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<Infallible, ()>> {
        use component_utils::field as f;

        while component_utils::Selector::new(self.deref_mut(), cx)
            .stream(f!(mut swarm), Self::swarm_event)
            .stream(f!(mut requests), Self::request)
            .done()
        {}

        Poll::Pending
    }
}

fn pick_route(
    topic: impl Into<Topic>,
    nodes: &HashMap<PeerId, enc::PublicKey>,
    table: dht::SharedRoutingTable,
) -> Option<[(onion::PublicKey, PeerId); 3]> {
    let rng = &mut rand::thread_rng();
    let repls = table.read().closest::<{ REPLICATION_FACTOR.get() + 1 }>(topic.into().as_bytes());
    let entry = repls
        .into_iter()
        .map(|id| id.to_peer_id())
        .filter_map(|id| nodes.get(&id).map(|&k| (k, id)))
        .choose(rng)?;

    let mut multiple = nodes
        .iter()
        .map(|(&a, &b)| (b, a))
        .filter(|(_, id)| *id != entry.1)
        .choose_multiple(rng, 2);
    multiple.insert(0, entry);
    multiple.try_into().ok()
}

#[allow(deprecated)]
type SE = libp2p::swarm::SwarmEvent<<Behaviour as NetworkBehaviour>::ToSwarm>;

enum SubscriptionRequest {
    Chat(ChatName, mpsc::Sender<ChatEvent>),
    Profile(Identity, mpsc::Sender<MailVariants>),
    Request(u8, Topic, Vec<u8>, oneshot::Sender<RawResponse>),
}

pub enum NodeRequest {
    Subscribe(Topic, oneshot::Sender<(NodeIdentity, EncryptedStream)>),
}

#[derive(Clone)]
pub struct NodeHandle {
    subs: Rc<RefCell<HashMap<NodeIdentity, Subscription>>>,
    requests: mpsc::Sender<NodeRequest>,
    dht: dht::SharedRoutingTable,
}

impl NodeHandle {
    pub fn new(requests: mpsc::Sender<NodeRequest>, dht: dht::SharedRoutingTable) -> Self {
        Self { requests, subs: Default::default(), dht }
    }

    pub async fn subscription_for(&mut self, topic: impl Into<Topic>) -> io::Result<Subscription> {
        let topic = topic.into();
        let replicatios =
            self.dht.read().closest::<{ REPLICATION_FACTOR.get() + 1 }>(topic.as_bytes());

        for repl in replicatios {
            if let Some(sub) = self.subs.borrow().get(&NodeIdentity::from(repl)) {
                return Ok(sub.clone());
            }
        }

        let (tx, rx) = oneshot::channel();
        self.requests
            .send(NodeRequest::Subscribe(topic, tx))
            .await
            .map_err(|_| io::ErrorKind::ConnectionReset)?;
        let (node, stream) = rx.await.map_err(|_| io::ErrorKind::ConnectionAborted)?;
        let sub = Subscription::new(stream, self.clone(), node);
        self.subs.borrow_mut().insert(node, sub.clone());
        Ok(sub)
    }
}

#[derive(Clone)]
pub struct Subscription {
    requests: mpsc::Sender<SubscriptionRequest>,
}

impl Subscription {
    pub fn new(stream: EncryptedStream, subscriptions: NodeHandle, id: NodeIdentity) -> Self {
        let (requests, inner_requests) = mpsc::channel(10);

        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = Self::run(stream, inner_requests).await {
                log::error!("subscription error: {e}");
            }

            subscriptions.subs.borrow_mut().remove(&id);
        });

        Self { requests }
    }

    pub async fn subscribe_to_chat(&mut self, chat: ChatName) -> Option<mpsc::Receiver<ChatEvent>> {
        let (tx, rx) = mpsc::channel(10);
        self.requests.send(SubscriptionRequest::Chat(chat, tx)).await.ok()?;
        Some(rx)
    }

    pub async fn subscribe_to_profile(
        &mut self,
        id: Identity,
    ) -> Option<mpsc::Receiver<MailVariants>> {
        let (tx, rx) = mpsc::channel(10);
        self.requests.send(SubscriptionRequest::Profile(id, tx)).await.ok()?;
        Some(rx)
    }

    pub async fn request<R: DecodeOwned>(
        &mut self,
        prefix: u8,
        topic: impl Into<Topic>,
        body: impl Encode,
    ) -> anyhow::Result<R> {
        crate::timeout(
            self.request_low::<Result<R, ChatError>>(prefix, topic, body),
            Duration::from_secs(3),
        )
        .await
        .context("timeout")??
        .map_err(Into::into)
    }

    pub async fn request_low<R: DecodeOwned>(
        &mut self,
        prefix: u8,
        topic: impl Into<Topic>,
        body: impl Encode,
    ) -> anyhow::Result<R> {
        let (tx, rx) = oneshot::channel();
        self.requests
            .send(SubscriptionRequest::Request(prefix, topic.into(), body.to_bytes(), tx))
            .await?;
        R::decode(&mut rx.await?.as_slice()).context("received invalid response")
    }

    async fn run(
        mut stream: EncryptedStream,
        mut requests: mpsc::Receiver<SubscriptionRequest>,
    ) -> io::Result<!> {
        enum RegisteredCall {
            ChatSub(ChatName, mpsc::Sender<ChatEvent>),
            Request(oneshot::Sender<RawResponse>),
            ProfileSub(Identity, mpsc::Sender<MailVariants>),
        }

        type Calls = HashMap<CallId, RegisteredCall>;

        async fn handle_request(
            stream: &mut EncryptedStream,
            req: SubscriptionRequest,
            subs: &mut Calls,
        ) -> io::Result<()> {
            // FIXME: Pass this tupple instead
            let (prefix, topic, body, rc) = match req {
                SubscriptionRequest::Chat(chat, tx) => {
                    (rpcs::SUBSCRIBE, Topic::Chat(chat), vec![], RegisteredCall::ChatSub(chat, tx))
                }
                SubscriptionRequest::Profile(id, tx) => (
                    rpcs::SUBSCRIBE,
                    Topic::Profile(id),
                    vec![],
                    RegisteredCall::ProfileSub(id, tx),
                ),
                SubscriptionRequest::Request(prefic, topic, body, tx) => {
                    (prefic, topic, body, RegisteredCall::Request(tx))
                }
            };

            let call_id = next_call_id();
            let len = (body.len() as u32).to_be_bytes();
            let header = RequestHeader { prefix, call_id, topic: topic.compress(), len };
            stream.write_all(header.as_bytes()).await?;
            stream.write_all(&body).await?;
            subs.insert(call_id, rc);

            Ok(())
        }

        async fn handle_response(
            stream: &mut EncryptedStream,
            res: io::Result<()>,
            buf: [u8; std::mem::size_of::<ResponseHeader>()],
            subs: &mut Calls,
        ) -> io::Result<()> {
            res?;

            let header = ResponseHeader::from_array(buf);
            let len = header.get_len();

            let mut data = vec![0u8; len];
            stream.read_exact(&mut data).await?;

            let Some(rc) = subs.remove(&header.call_id) else {
                log::error!("unexpected response");
                return Ok(());
            };

            match rc {
                RegisteredCall::ChatSub(chat, mut ch) => {
                    if let Some(ev) = ChatEvent::decode(&mut data.as_slice()) {
                        if ch.send(ev).await.is_err() {
                            let header = RequestHeader {
                                prefix: rpcs::UNSUBSCRIBE,
                                call_id: next_call_id(),
                                topic: Topic::Chat(chat).compress(),
                                len: [0; 4],
                            };
                            stream.write_all(header.as_bytes()).await?;
                        }
                    }
                    subs.insert(header.call_id, RegisteredCall::ChatSub(chat, ch));
                }
                RegisteredCall::Request(resp) => {
                    if resp.send(data).is_err() {
                        log::error!("cannot send response, receiver was dropped");
                    }
                }
                RegisteredCall::ProfileSub(id, mut ch) => {
                    if let Some(ev) = MailVariants::decode(&mut data.as_slice()) {
                        if ch.send(ev).await.is_err() {
                            let header = RequestHeader {
                                prefix: rpcs::UNSUBSCRIBE,
                                call_id: next_call_id(),
                                topic: Topic::Profile(id).compress(),
                                len: [0; 4],
                            };
                            stream.write_all(header.as_bytes()).await?;
                        }
                    }
                    subs.insert(header.call_id, RegisteredCall::ProfileSub(id, ch));
                }
            }

            Ok(())
        }

        let mut chat_subs = Calls::new();
        let mut buf = [0u8; std::mem::size_of::<ResponseHeader>()];
        loop {
            futures::select! {
                req = requests.select_next_some() => handle_request(&mut stream, req, &mut chat_subs).await?,
                res = stream.read_exact(&mut buf).fuse() => handle_response(&mut stream, res, buf, &mut chat_subs).await?,
            }
        }
    }
}

#[derive(libp2p::swarm::NetworkBehaviour, Default)]
struct Behaviour {
    onion: onion::Behaviour,
    key_share: onion::key_share::Behaviour,
    chat_dht: dht::Behaviour,
    satelite_dht: dht::Behaviour,
    storage_dht: dht::Behaviour,
    streaming: streaming::Behaviour,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[repr(u8)]
pub enum BootPhase {
    #[error("fetching nodes and profile from chain...")]
    FetchNodesAndProfile,
    #[error("initiating orion connection...")]
    InitiateConnection,
    #[error("collecting server keys... ({0} left)")]
    CollecringKeys(usize),
    #[error("opening route to profile...")]
    ProfileOpen,
    #[error("loading vault...")]
    VaultLoad,
    #[error("creating new profile...")]
    ProfileCreate,
    #[error("searching chats...")]
    ChatSearch,
    #[error("ready")]
    ChatRun,
}

impl BootPhase {
    pub fn discriminant(&self) -> u8 {
        // SAFETY: Because `Self` is marked `repr(u8)`, its layout is a `repr(C)` `union`
        // between `repr(C)` structs, each of which has the `u8` discriminant as its first
        // field, so we can read the discriminant without offsetting the pointer.
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
}
