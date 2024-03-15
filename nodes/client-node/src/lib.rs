#![feature(slice_take)]

use {
    anyhow::Context,
    argon2::Argon2,
    chain_api::{ContractId, RawUserName, TransactionHandler, UserIdentity},
    chat_spec::*,
    component_utils::{Codec, FindAndRemove, LinearMap, Reminder},
    crypto::{
        decrypt,
        enc::{self, ChoosenCiphertext, Ciphertext},
        sign, FixedAesPayload, Serialized, TransmutationCircle,
    },
    dht::Route,
    libp2p::{
        core::upgrade::Version,
        futures::{
            channel::{mpsc, oneshot},
            SinkExt, StreamExt,
        },
        identity::ed25519,
        swarm::{NetworkBehaviour, SwarmEvent},
        *,
    },
    onion::{EncryptedStream, PathId, SharedSecret},
    rand::{rngs::OsRng, seq::IteratorRandom, CryptoRng, RngCore},
    std::{
        collections::{HashMap, HashSet},
        convert::{identity, Infallible},
        future::Future,
        io,
        net::IpAddr,
        pin::{self, pin},
        str::FromStr,
        task::Poll,
        time::Duration,
    },
    web_sys::{
        wasm_bindgen::{self, closure::Closure, JsCast, JsValue},
        window,
    },
};

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub type SubscriptionMessage = Vec<u8>;

pub type RawResponse = Vec<u8>;

pub type MessageContent = String;

pub struct JoinRequestPayload {
    pub name: RawUserName,
    pub chat: RawChatName,
    pub identity: Identity,
}

crypto::impl_transmute!(JoinRequestPayload,);

#[derive(Default, Codec)]
pub struct Vault {
    pub chats: LinearMap<ChatName, ChatMeta>,
    pub hardened_chats: LinearMap<ChatName, HardenedChatMeta>,
    pub theme: Theme,
}

#[derive(Codec)]
pub struct ChatMeta {
    pub secret: crypto::SharedSecret,
    #[codec(skip)]
    pub action_no: Nonce,
}

impl Default for ChatMeta {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatMeta {
    pub fn new() -> Self {
        Self::from_secret(crypto::new_secret(OsRng))
    }

    pub fn from_secret(secret: SharedSecret) -> Self {
        Self { secret, action_no: Default::default() }
    }
}

#[derive(Default, Codec)]
pub struct HardenedChatMeta {
    pub members: LinearMap<UserName, MemberMeta>,
}

#[derive(Clone, Copy, Codec)]
pub struct MemberMeta {
    pub secret: crypto::SharedSecret,
    pub identity: crypto::Hash,
}

#[derive(Codec)]
pub struct RawChatMessage<'a> {
    pub sender: UserName,
    pub content: &'a str,
}

#[derive(Codec)]
pub enum MailVariants<'a> {
    ChatInvite {
        chat: ChatName,
        cp: Serialized<ChoosenCiphertext>,
    },
    HardenedJoinRequest {
        cp: Serialized<Ciphertext>,
        payload: [u8; std::mem::size_of::<
            FixedAesPayload<{ std::mem::size_of::<JoinRequestPayload>() }>,
        >()],
    },
    HardenedChatMessage {
        nonce: Nonce,
        chat: crypto::Hash,
        content: Reminder<'a>,
    },
    HardenedChatInvite {
        cp: Serialized<Ciphertext>,
        payload: Reminder<'a>,
    },
}

#[derive(Codec)]
pub struct HardenedChatInvitePayload {
    pub chat: ChatName,
    pub inviter: UserName,
    pub inviter_id: Identity,
    pub members: Vec<UserName>,
}

pub fn try_set_color(name: &str, value: u32) -> Result<(), JsValue> {
    web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .body()
        .ok_or("no body")?
        .style()
        .set_property(name, &format!("#{:08x}", value))
}

pub fn try_load_color_from_style(name: &str) -> Result<u32, JsValue> {
    u32::from_str_radix(
        web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .body()
            .ok_or("no body")?
            .style()
            .get_property_value(name)?
            .strip_prefix('#')
            .ok_or("expected # to start the color")?,
        16,
    )
    .map_err(|e| e.to_string().into())
}

macro_rules! gen_theme {
    ($(
        $name:ident: $value:literal,
    )*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Codec)]
        pub struct Theme { $(
            pub $name: u32,
        )* }

        impl Theme {
            pub fn apply(self) -> Result<(), JsValue> {
                $(try_set_color(concat!("--", stringify!($name), "-color"), self.$name)?;)*
                Ok(())
            }

            pub fn from_current() -> Result<Self, JsValue> {
                Ok(Self { $(
                    $name: try_load_color_from_style(concat!("--", stringify!($name), "-color"))?,
                )* })
            }

            pub const KEYS: &'static [&'static str] = &[$(stringify!($name),)*];
        }

        impl Default for Theme {
            fn default() -> Self {
                Self { $( $name: $value,)* }
            }
        }
    };
}

gen_theme! {
    primary: 0x0000_00ff,
    secondary: 0x3333_33ff,
    highlight: 0xffff_ffff,
    font: 0xffff_ffff,
    error: 0xff00_00ff,
}

pub struct Node {
    swarm: Swarm<Behaviour>,
    subscriptions: futures::stream::SelectAll<Subscription>,
    pending_requests: LinearMap<CallId, oneshot::Sender<RawResponse>>,
    pending_topic_search: LinearMap<PathId, Vec<RequestInit>>,
    requests: RequestStream,
}

impl Node {
    pub async fn new(
        keys: UserKeys,
        mut wboot_phase: impl FnMut(BootPhase),
    ) -> anyhow::Result<(Self, Vault, RequestDispatch, Nonce, Nonce)> {
        macro_rules! set_state { ($($t:tt)*) => {_ = wboot_phase(BootPhase::$($t)*)}; }

        set_state!(FetchNodesAndProfile);

        let (mut request_dispatch, commands) = RequestDispatch::new();
        let chain_api = chain_node(keys.name).await?;
        let node_request = chain_api.list(node_contract());
        let profile_request =
            chain_api.get_profile_by_name(user_contract(), username_to_raw(keys.name));
        let (node_data, profile_hash) = futures::try_join!(node_request, profile_request)?;
        let profile_hash = profile_hash.context("profile not found")?;
        let profile = keys.to_identity();

        anyhow::ensure!(
            profile_hash.sign == profile.sign && profile_hash.enc == profile.enc,
            "profile hash does not match our account"
        );

        set_state!(InitiateConnection);

        let keypair = identity::Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id();

        let behaviour = Behaviour {
            onion: onion::Behaviour::new(
                onion::Config::new(None, peer_id).keep_alive_interval(Duration::from_secs(100)),
            ),
            key_share: onion::key_share::Behaviour::default(),
            dht: dht::Behaviour::default(),
        };
        let transport = websocket_websys::Transport::new(100)
            .upgrade(Version::V1)
            .authenticate(noise::Config::new(&keypair).unwrap())
            .multiplex(yamux::Config::default())
            .boxed();

        let mut swarm = swarm::Swarm::new(
            transport,
            behaviour,
            peer_id,
            libp2p::swarm::Config::with_wasm_executor()
                .with_idle_connection_timeout(Duration::from_secs(2)),
        );

        fn unpack_node_id(id: sign::Ed) -> anyhow::Result<ed25519::PublicKey> {
            libp2p::identity::ed25519::PublicKey::try_from_bytes(&id)
                .context("deriving ed signature")
        }

        fn unpack_node_addr(addr: chain_api::NodeAddress) -> Multiaddr {
            let (addr, port) = addr.into();
            Multiaddr::empty()
                .with(match addr {
                    IpAddr::V4(ip) => multiaddr::Protocol::Ip4(ip),
                    IpAddr::V6(ip) => multiaddr::Protocol::Ip6(ip),
                })
                .with(multiaddr::Protocol::Tcp(port + 100))
                .with(multiaddr::Protocol::Ws("/".into()))
        }

        let node_count = node_data.len();
        let tolerance = 0;
        set_state!(CollecringKeys(
            node_count - swarm.behaviour_mut().key_share.keys.len() - tolerance
        ));

        let nodes = node_data
            .into_iter()
            .map(|(node, ip)| {
                let id = unpack_node_id(node.id).unwrap();
                let addr = unpack_node_addr(ip);
                Ok(Route::new(id, addr))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        swarm.behaviour_mut().dht.table.bulk_insert(nodes);

        let routes = swarm.behaviour_mut().dht.table.iter().map(Route::peer_id).collect::<Vec<_>>();
        for route in routes {
            _ = swarm.dial(route);
        }

        loop {
            // TODO: add timeout instead
            match swarm.select_next_some().await {
                SwarmEvent::Behaviour(BehaviourEvent::KeyShare(..)) => {
                    let remining =
                        node_count - swarm.behaviour_mut().key_share.keys.len() - tolerance;
                    set_state!(CollecringKeys(remining));
                    if remining == 0 {
                        break;
                    }
                }
                e => log::debug!("{:?}", e),
            }
        }

        let nodes = &swarm.behaviour_mut().key_share.keys;
        anyhow::ensure!(
            nodes.len() >= min_nodes(),
            "not enough nodes in network, needed {}, got {}",
            min_nodes(),
            nodes.len(),
        );

        let members = swarm
            .behaviour()
            .dht
            .table
            .closest(profile_hash.sign.as_ref())
            .take(REPLICATION_FACTOR.get() + 1);

        set_state!(ProfileOpen);
        let pick = members.choose(&mut rand::thread_rng()).unwrap().peer_id();
        let route = pick_route(&swarm.behaviour_mut().key_share.keys, pick);
        let pid = swarm.behaviour_mut().onion.open_path(route);
        let ((mut profile_stream, ..), profile_stream_id, profile_stream_peer) = loop {
            match swarm.select_next_some().await {
                SwarmEvent::Behaviour(BehaviourEvent::Onion(onion::Event::OutboundStream(
                    stream,
                    id,
                ))) if id == pid => break (stream.context("opening profile route")?, id, pick),
                e => log::debug!("{:?}", e),
            }
        };

        set_state!(VaultLoad);
        let (mut vault_nonce, mail_action, Reminder(vault)) = match request_dispatch
            .dispatch_direct::<(Nonce, Nonce, Reminder)>(
                &mut profile_stream,
                rpcs::FETCH_VAULT,
                Topic::Profile(profile_hash.sign),
                (),
            )
            .await
        {
            Ok((vn, m, v)) => (vn + 1, m + 1, v),
            Err(e) => {
                log::info!("cannot access vault: {e} {:?}", profile_hash.sign);
                Default::default()
            }
        };

        let vault = if vault.is_empty() && vault_nonce == 0 {
            set_state!(ProfileCreate);
            let proof = Proof::new(&keys.sign, &mut vault_nonce, &[][..], OsRng);
            request_dispatch
                .dispatch_direct(
                    &mut profile_stream,
                    rpcs::CREATE_PROFILE,
                    Topic::Profile(profile_hash.sign),
                    &(proof, keys.enc.public_key().into_bytes()),
                )
                .await
                .context("creating account")?;

            Default::default()
        } else {
            let mut vault = vault.to_vec();
            decrypt(&mut vault, keys.vault)
                .and_then(|v| Vault::decode(&mut &*v))
                .unwrap_or_default()
        };
        let _ = vault.theme.apply();

        set_state!(ChatSearch);

        let mut profile_sub = Subscription {
            id: profile_stream_id,
            peer_id: profile_stream_peer,
            topics: [Topic::Profile(profile_hash.sign)].into(),
            subscriptions: Default::default(),
            stream: profile_stream,
        };

        let mut topology = HashMap::<PeerId, HashSet<ChatName>>::new();
        let iter = vault.chats.keys().copied().flat_map(|c| {
            swarm
                .behaviour()
                .dht
                .table
                .closest(c.as_bytes())
                .take(REPLICATION_FACTOR.get() + 1)
                .map(move |peer| (peer.peer_id(), c))
        });
        for (peer, chat) in iter {
            if peer == profile_stream_peer {
                profile_sub.topics.push(chat.into());
                continue;
            }

            topology.entry(peer).or_default().insert(chat);
        }

        let mut topology = topology.into_iter().collect::<Vec<_>>();
        topology.sort_by_key(|(_, v)| v.len());
        let mut to_connect = vec![];
        let mut seen = HashSet::new();
        while seen.len() < vault.chats.len() {
            let (peer, mut chats) = topology.pop().unwrap();
            chats.retain(|&c| seen.insert(c));
            if chats.is_empty() {
                continue;
            }
            to_connect.push((peer, chats));
        }

        let mut awaiting = to_connect
            .into_iter()
            .map(|(pick, set)| {
                let route = pick_route(&swarm.behaviour_mut().key_share.keys, pick);
                let pid = swarm.behaviour_mut().onion.open_path(route);
                (pid, pick, set)
            })
            .collect::<Vec<_>>();

        let mut subscriptions = futures::stream::SelectAll::new();
        subscriptions.push(profile_sub);
        while !awaiting.is_empty() {
            let ((stream, got_peer_id), subs, peer_id, id) = loop {
                match swarm.select_next_some().await {
                    SwarmEvent::Behaviour(BehaviourEvent::Onion(onion::Event::OutboundStream(
                        stream,
                        id,
                    ))) => {
                        if let Some((.., peer_id, subs)) =
                            awaiting.find_and_remove(|&(i, ..)| i == id)
                        {
                            break (
                                stream.context("opening chat subscription route")?,
                                subs,
                                peer_id,
                                id,
                            );
                        }
                    }
                    e => log::debug!("{:?}", e),
                }
            };
            debug_assert!(peer_id == got_peer_id);

            subscriptions.push(Subscription {
                id,
                peer_id,
                topics: subs.into_iter().map(Topic::Chat).collect(),
                subscriptions: Default::default(),
                stream,
            });
        }

        set_state!(ChatRun);

        Ok((
            Self {
                swarm,
                subscriptions,
                pending_requests: Default::default(),
                pending_topic_search: Default::default(),
                requests: commands,
            },
            vault,
            request_dispatch,
            vault_nonce,
            mail_action.max(1),
        ))
    }

    fn handle_topic_search(&mut self, command: RequestInit) {
        let search_key = command.topic();
        if let Some((_, l)) = self
            .pending_topic_search
            .iter_mut()
            .find(|(_, v)| v.iter().any(|c| c.topic() == search_key))
        {
            log::debug!("search already in progress");
            l.push(command);
            return;
        }

        let peers = self
            .swarm
            .behaviour()
            .dht
            .table
            .closest(search_key.as_bytes())
            .take(REPLICATION_FACTOR.get() + 1)
            .map(Route::peer_id)
            .collect::<Vec<_>>();

        if let Some(sub) = self.subscriptions.iter_mut().find(|s| peers.contains(&s.peer_id)) {
            log::debug!("shortcut topic found");
            sub.topics.push(search_key);
            self.handle_command(command);
            return;
        }

        let Some(pick) = peers.into_iter().choose(&mut rand::thread_rng()) else {
            log::error!("search response does not contain any peers");
            return;
        };

        let path = pick_route(&self.swarm.behaviour().key_share.keys, pick);
        let pid = self.swarm.behaviour_mut().onion.open_path(path);
        self.pending_topic_search.insert(pid, vec![command]);
    }

    fn handle_request(&mut self, req: RawRequest) {
        let Some(sub) = self
            .subscriptions
            .iter_mut()
            .find(|s| req.topic.as_ref().map_or(true, |t| s.topics.contains(t)))
        else {
            self.handle_topic_search(RequestInit::Request(req));
            return;
        };

        let request = chat_spec::Request {
            prefix: req.prefix,
            id: req.id,
            topic: req.topic,
            body: Reminder(&req.payload),
        };

        sub.stream.write(request).unwrap();
        self.pending_requests.insert(req.id, req.channel);
        log::debug!("request sent, {:?}", req.id);
    }

    fn handle_subscription_request(&mut self, sub: SubscriptionInit) {
        log::info!("Subscribing to {:?}", sub.topic);
        let Some(subs) = self.subscriptions.iter_mut().find(|s| s.topics.contains(&sub.topic))
        else {
            self.handle_topic_search(RequestInit::Subscription(sub));
            return;
        };

        log::info!("Creating Subsctiption request to {:?}", sub.topic);
        let request = chat_spec::Request {
            prefix: rpcs::SUBSCRIBE,
            id: sub.id,
            topic: Some(sub.topic),
            body: Reminder(&[]),
        };

        subs.stream.write(request).unwrap();
        subs.subscriptions.insert(sub.id, sub.channel);
        log::debug!("subscription request sent, {:?}", sub.id);
    }

    fn handle_command(&mut self, command: RequestInit) {
        match command {
            RequestInit::Request(req) => self.handle_request(req),
            RequestInit::Subscription(sub) => self.handle_subscription_request(sub),
            RequestInit::EndSubscription(topic) => {
                let Some(sub) = self.subscriptions.iter_mut().find(|s| s.topics.contains(&topic))
                else {
                    log::error!("cannot find subscription to end");
                    return;
                };

                let request = chat_spec::Request {
                    prefix: rpcs::UNSUBSCRIBE,
                    id: CallId::new(),
                    topic: Some(topic),
                    body: Reminder(&[]),
                };

                sub.stream.write(request).unwrap();
                self.subscriptions
                    .iter_mut()
                    .find_map(|s| {
                        let index = s.topics.iter().position(|t| t == &topic)?;
                        s.topics.swap_remove(index);
                        Some(())
                    })
                    .expect("channel to exist");
            }
        }
    }

    fn handle_swarm_event(&mut self, event: SE) {
        match event {
            SwarmEvent::Behaviour(BehaviourEvent::Onion(onion::Event::OutboundStream(
                stream,
                id,
            ))) => {
                if let Some(req) = self.pending_topic_search.remove(&id) {
                    let (stream, peer_id) = stream.expect("TODO: somehow report this error");
                    self.subscriptions.push(Subscription {
                        id,
                        peer_id,
                        topics: [req[0].topic().to_owned()].into(),
                        subscriptions: Default::default(),
                        stream,
                    });
                    req.into_iter().for_each(|r| self.handle_command(r));
                }
            }
            e => log::debug!("{:?}", e),
        }
    }

    fn handle_subscription_response(&mut self, (id, request): (PathId, io::Result<Vec<u8>>)) {
        let Ok(msg) = request.inspect_err(|e| log::error!("chat subscription error: {e}")) else {
            return;
        };

        let Some((cid, Reminder(content))) = <_>::decode(&mut &*msg) else {
            log::error!("invalid chat subscription response");
            return;
        };

        if let Some(channel) = self.pending_requests.remove(&cid) {
            log::debug!("response recieved, {:?}", cid);
            _ = channel.send(content.to_owned());
            return;
        }

        if let Some(channel) = self
            .subscriptions
            .iter_mut()
            .find(|s| s.id == id)
            .and_then(|s| s.subscriptions.get_mut(&cid))
        {
            if channel.try_send(content.to_owned()).is_err() {
                self.subscriptions
                    .iter_mut()
                    .find_map(|s| s.subscriptions.remove(&cid))
                    .expect("channel to exist");
            }
            return;
        }

        log::error!("request does not exits even though we recieived it {:?}", cid);
    }
}

impl Future for Node {
    type Output = Result<Infallible, ()>;

    fn poll(
        mut self: pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<Infallible, ()>> {
        let mut changed = true;
        while std::mem::take(&mut changed) {
            while let Poll::Ready(next) = self.requests.poll_next_unpin(cx) {
                self.handle_command(next.ok_or(())?);
                changed = true;
            }

            while let Poll::Ready(next) = self.subscriptions.poll_next_unpin(cx) {
                self.handle_subscription_response(next.unwrap());
                changed = true;
            }

            while let Poll::Ready(next) = self.swarm.poll_next_unpin(cx) {
                self.handle_swarm_event(next.unwrap());
                changed = true;
            }
        }

        Poll::Pending
    }
}

fn pick_route(
    nodes: &HashMap<PeerId, enc::PublicKey>,
    target: PeerId,
) -> [(onion::PublicKey, PeerId); 3] {
    assert!(nodes.len() >= 2);
    let mut rng = rand::thread_rng();
    let mut picked = nodes
        .iter()
        .filter(|(p, _)| **p != target)
        .map(|(p, ud)| (*ud, *p))
        .choose_multiple(&mut rng, 2);
    picked.insert(0, (*nodes.get(&target).unwrap(), target));
    picked.try_into().unwrap()
}

#[allow(deprecated)]
type SE = libp2p::swarm::SwarmEvent<<Behaviour as NetworkBehaviour>::ToSwarm>;

struct Subscription {
    id: PathId,
    peer_id: PeerId,
    topics: Vec<Topic>,
    subscriptions: LinearMap<CallId, mpsc::Sender<SubscriptionMessage>>,
    stream: EncryptedStream,
}

impl futures::Stream for Subscription {
    type Item = (PathId, <EncryptedStream as futures::Stream>::Item);

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.stream.poll_next_unpin(cx).map(|opt| opt.map(|v| (self.id, v)))
    }
}

#[derive(libp2p::swarm::NetworkBehaviour)]
struct Behaviour {
    onion: onion::Behaviour,
    key_share: onion::key_share::Behaviour,
    dht: dht::Behaviour,
}

#[derive(Clone)]
pub struct RequestDispatch {
    buffer: Vec<u8>,
    sink: mpsc::Sender<RequestInit>,
}

pub fn proof_topic<T>(p: &Proof<T>) -> Topic {
    crypto::hash::from_slice(&p.pk).into()
}

impl RequestDispatch {
    pub fn new() -> (Self, RequestStream) {
        let (sink, stream) = mpsc::channel(5);
        (Self { buffer: Vec::new(), sink }, stream)
    }

    async fn dispatch<'a, R: Codec<'a>>(
        &'a mut self,
        prefix: u8,
        topic: impl Into<Option<Topic>>,
        request: impl Codec<'_>,
    ) -> Result<R> {
        let id = CallId::new();
        let (tx, rx) = oneshot::channel();
        self.sink
            .send(RequestInit::Request(RawRequest {
                id,
                topic: topic.into(),
                prefix,
                payload: request.to_bytes(),
                channel: tx,
            }))
            .await
            .map_err(|_| ChatError::ChannelClosed)?;
        self.buffer =
            crate::timeout(rx, REQUEST_TIMEOUT).await?.map_err(|_| ChatError::ChannelClosed)?;
        Result::<R>::decode(&mut &self.buffer[..])
            .ok_or(ChatError::InvalidResponse)
            .and_then(identity)
    }

    pub async fn add_member(
        &mut self,
        proof: Proof<ChatName>,
        member: Identity,
        config: Member,
    ) -> Result<()> {
        let msg = (&proof, member, config);
        self.dispatch(rpcs::ADD_MEMBER, Topic::Chat(proof.context), msg).await
    }

    pub async fn kick_member(&mut self, proof: Proof<ChatName>, identity: Identity) -> Result<()> {
        self.dispatch(rpcs::KICK_MEMBER, Topic::Chat(proof.context), (proof, identity)).await
    }

    pub async fn send_message<'a>(
        &'a mut self,
        proof: Proof<Reminder<'_>>,
        name: ChatName,
    ) -> Result<()> {
        self.dispatch(rpcs::SEND_MESSAGE, Topic::Chat(name), proof).await
    }

    pub async fn fetch_messages(
        &mut self,
        name: ChatName,
        cursor: Cursor,
    ) -> Result<(Cursor, Reminder)> {
        self.dispatch(rpcs::FETCH_MESSAGES, Topic::Chat(name), cursor).await
    }

    pub async fn create_chat(&mut self, name: ChatName, me: Identity) -> Result<()> {
        self.dispatch(rpcs::CREATE_CHAT, Topic::Chat(name), me).await
    }

    pub async fn set_vault<'a>(&'a mut self, proof: Proof<Reminder<'_>>) -> Result<()> {
        self.dispatch(rpcs::SET_VAULT, proof_topic(&proof), proof).await
    }

    pub async fn fetch_keys(&mut self, identity: Identity) -> Result<FetchProfileResp> {
        self.dispatch(rpcs::FETCH_PROFILE, Topic::Profile(identity), ()).await
    }

    pub async fn send_mail<'a>(&'a mut self, to: Identity, mail: impl Codec<'_>) -> Result<()> {
        self.dispatch(rpcs::SEND_MAIL, Topic::Profile(to), mail).await.or_else(ChatError::recover)
    }

    pub async fn read_mail(&mut self, proof: Proof<Mail>) -> Result<Reminder> {
        self.dispatch(rpcs::READ_MAIL, proof_topic(&proof), proof).await
    }

    pub async fn fetch_members(
        &mut self,
        name: ChatName,
        from: Identity,
        limit: u32,
    ) -> Result<Vec<(Identity, Member)>> {
        self.dispatch(rpcs::FETCH_MEMBERS, Topic::Chat(name), (from, limit)).await
    }

    pub async fn fetch_my_member(&mut self, name: ChatName, me: Identity) -> Result<Member> {
        let mebers = self.fetch_members(name, me, 1).await?;
        mebers
            .into_iter()
            .next()
            .and_then(|(id, m)| (id == me).then_some(m))
            .ok_or(ChatError::NotMember)
    }

    async fn dispatch_direct<'a, R: Codec<'a>>(
        &'a mut self,
        stream: &mut EncryptedStream,
        prefix: u8,
        topic: impl Into<Option<Topic>>,
        request: impl Codec<'_>,
    ) -> Result<R> {
        stream
            .write(chat_spec::Request {
                prefix,
                id: CallId::new(),
                topic: topic.into(),
                body: Reminder(&request.to_bytes()),
            })
            .ok_or(ChatError::MessageOverload)?;

        self.buffer = crate::timeout(stream.next(), REQUEST_TIMEOUT)
            .await?
            .ok_or(ChatError::ChannelClosed)?
            .map_err(|_| ChatError::ChannelClosed)?;

        <(CallId, Result<R>)>::decode(&mut &self.buffer[..])
            .ok_or(ChatError::InvalidResponse)
            .and_then(|(_, r)| r)
    }

    pub async fn subscribe(
        &mut self,
        topic: impl Into<Topic>,
    ) -> Result<mpsc::Receiver<SubscriptionMessage>> {
        let (tx, mut rx) = mpsc::channel(0);
        let id = CallId::new();
        let topic: Topic = topic.into();
        self.sink
            .try_send(RequestInit::Subscription(SubscriptionInit { id, topic, channel: tx }))
            .map_err(|_| ChatError::ChannelClosed)?;

        let init =
            crate::timeout(rx.next(), REQUEST_TIMEOUT).await?.ok_or(ChatError::ChannelClosed)?;
        Result::<()>::decode(&mut &init[..]).ok_or(ChatError::InvalidResponse)??;

        Ok(rx)
    }

    pub fn unsubscribe(&mut self, topic: impl Into<Topic>) {
        let topic: Topic = topic.into();
        self.sink.try_send(RequestInit::EndSubscription(topic)).unwrap();
    }
}

pub type RequestStream = mpsc::Receiver<RequestInit>;

pub enum RequestInit {
    Request(RawRequest),
    Subscription(SubscriptionInit),
    EndSubscription(Topic),
}

impl RequestInit {
    pub fn topic(&self) -> Topic {
        match self {
            Self::Request(r) => r.topic.unwrap(),
            Self::Subscription(s) => s.topic,
            Self::EndSubscription(t) => *t,
        }
    }
}

pub struct SubscriptionInit {
    pub id: CallId,
    pub topic: Topic,
    pub channel: mpsc::Sender<SubscriptionMessage>,
}

pub struct RawRequest {
    pub id: CallId,
    pub topic: Option<Topic>,
    pub prefix: u8,
    pub payload: Vec<u8>,
    pub channel: oneshot::Sender<RawResponse>,
}

#[derive(Clone)]
pub struct UserKeys {
    pub name: UserName,
    pub sign: sign::Keypair,
    pub enc: enc::Keypair,
    pub vault: crypto::SharedSecret,
}

impl UserKeys {
    pub fn new(name: UserName, password: &str) -> Self {
        struct Entropy<'a>(&'a [u8]);

        impl RngCore for Entropy<'_> {
            fn next_u32(&mut self) -> u32 {
                unimplemented!()
            }

            fn next_u64(&mut self) -> u64 {
                unimplemented!()
            }

            fn fill_bytes(&mut self, dest: &mut [u8]) {
                let data = self.0.take(..dest.len()).expect("not enough entropy");
                dest.copy_from_slice(data);
            }

            fn try_fill_bytes(&mut self, _: &mut [u8]) -> Result<(), rand::Error> {
                unimplemented!()
            }
        }

        impl CryptoRng for Entropy<'_> {}

        const VALUT: usize = 32;
        const ENC: usize = 64 + 32;
        const SIGN: usize = 32 + 48;
        let mut bytes = [0; VALUT + ENC + SIGN];
        Argon2::default()
            .hash_password_into(password.as_bytes(), &username_to_raw(name), &mut bytes)
            .unwrap();

        let mut entropy = Entropy(&bytes);

        let sign = sign::Keypair::new(&mut entropy);
        let enc = enc::Keypair::new(&mut entropy);
        let vault = crypto::new_secret(&mut entropy);
        Self { name, sign, enc, vault }
    }

    pub fn identity_hash(&self) -> Identity {
        crypto::hash::new(&self.sign.public_key())
    }

    pub fn to_identity(&self) -> UserIdentity {
        UserIdentity {
            sign: crypto::hash::new(&self.sign.public_key()),
            enc: crypto::hash::new(&self.enc.public_key()),
        }
    }
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

pub async fn fetch_profile(
    my_name: UserName,
    name: UserName,
) -> Result<UserIdentity, anyhow::Error> {
    let client = chain_node(my_name).await?;
    match client.get_profile_by_name(user_contract(), username_to_raw(name)).await {
        Ok(Some(u)) => Ok(u),
        Ok(None) => anyhow::bail!("user {name} does not exist"),
        Err(e) => anyhow::bail!("failed to fetch user: {e}"),
    }
}

pub async fn chain_node(name: UserName) -> Result<chain_api::Client<WebSigner>, chain_api::Error> {
    component_utils::build_env!(CHAIN_NODE);
    chain_api::Client::with_signer(CHAIN_NODE, WebSigner(name)).await
}

pub fn user_contract() -> ContractId {
    component_utils::build_env!(USER_CONTRACT);
    ContractId::from_str(USER_CONTRACT).unwrap()
}

pub fn node_contract() -> ContractId {
    component_utils::build_env!(NODE_CONTRACT);
    ContractId::from_str(NODE_CONTRACT).unwrap()
}

pub fn min_nodes() -> usize {
    component_utils::build_env!(MIN_NODES);
    MIN_NODES.parse().unwrap()
}

async fn sign_with_wallet(payload: &str) -> Result<Vec<u8>, JsValue> {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(catch, js_namespace = integration)]
        async fn sign(data: &str) -> Result<JsValue, JsValue>;
    }

    let sig = sign(payload).await?;
    let sig = sig.as_string().ok_or("user did something very wrong")?;
    let sig = sig.trim_start_matches("0x01");
    hex::decode(sig).map_err(|e| e.to_string().into())
}

async fn get_account_id(name: &str) -> Result<String, JsValue> {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(catch, js_namespace = integration)]
        async fn address(name: &str) -> Result<JsValue, JsValue>;
    }

    let id = address(name).await?;
    id.as_string().ok_or("user, pleas stop").map_err(Into::into)
}

pub struct WebSigner(pub UserName);

impl TransactionHandler for WebSigner {
    async fn account_id_async(&self) -> Result<chain_api::AccountId, chain_api::Error> {
        let id = get_account_id(&self.0)
            .await
            .map_err(|e| chain_api::Error::Other(e.as_string().unwrap_or_default()))?;
        chain_api::AccountId::from_str(&id)
            .map_err(|e| chain_api::Error::Other(format!("invalid id received: {e}")))
    }

    async fn handle(
        &self,
        inner: &chain_api::InnerClient,
        call: impl chain_api::TxPayload,
        nonce: chain_api::Nonce,
    ) -> Result<(), chain_api::Error> {
        let account_id = self.account_id_async().await?;
        let genesis_hash = chain_api::encode_then_hex(&inner.client.genesis_hash());
        // These numbers aren't SCALE encoded; their bytes are just converted to hex:
        let spec_version =
            chain_api::to_hex(inner.client.runtime_version().spec_version.to_be_bytes());
        let transaction_version =
            chain_api::to_hex(inner.client.runtime_version().transaction_version.to_be_bytes());
        let nonce_enc = chain_api::to_hex(nonce.to_be_bytes());
        let mortality_checkpoint = chain_api::encode_then_hex(&inner.client.genesis_hash());
        let era = chain_api::immortal_era();
        let method = chain_api::to_hex(call.encode_call_data(&inner.client.metadata())?);
        let signed_extensions: Vec<String> = inner
            .client
            .metadata()
            .extrinsic()
            .signed_extensions()
            .iter()
            .map(|e| e.identifier().to_string())
            .collect();
        let tip = chain_api::encode_tip(0u128);
        let payload = chain_api::json!({
            "specVersion": spec_version,
            "transactionVersion": transaction_version,
            "address": account_id.to_string(),
            "blockHash": mortality_checkpoint,
            "blockNumber": "0x00000000",
            "era": era,
            "genesisHash": genesis_hash,
            "method": method,
            "nonce": nonce_enc,
            "signedExtensions": signed_extensions,
            "tip": tip,
            "version": 4,
        });

        let signature = sign_with_wallet(&payload.to_string())
            .await
            .map_err(|e| chain_api::Error::Other(e.as_string().unwrap_or_default()))?;

        let signature = signature
            .try_into()
            .map_err(|_| chain_api::Error::Other("signature has invalid size".into()))
            .map(chain_api::new_signature)?;

        let tx = inner.client.tx();

        tx.validate(&call)?;
        let unsigned_payload =
            tx.create_partial_signed_with_nonce(&call, nonce, Default::default())?;

        let progress = unsigned_payload
            .sign_with_address_and_signature(&account_id.into(), &signature.into())
            .submit_and_watch()
            .await?;

        chain_api::wait_for_in_block(progress).await.map(drop)
    }
}

pub async fn timeout<F: Future>(f: F, duration: Duration) -> Result<F::Output, ChatError> {
    let mut fut = pin!(f);
    let mut callback = None::<(Closure<dyn FnMut()>, i32)>;
    let until = instant::Instant::now() + duration;
    std::future::poll_fn(|cx| {
        if let Poll::Ready(v) = fut.as_mut().poll(cx) {
            if let Some((_cl, handle)) = callback.take() {
                window().unwrap().clear_timeout_with_handle(handle);
            }

            return Poll::Ready(Ok(v));
        }

        if until < instant::Instant::now() {
            return Poll::Ready(Err(ChatError::Timeout));
        }

        if callback.is_none() {
            let waker = cx.waker().clone();
            let handler = Closure::once(move || waker.wake());
            let handle = window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    handler.as_ref().unchecked_ref(),
                    duration.as_millis() as i32,
                )
                .unwrap();
            callback = Some((handler, handle));
        }

        Poll::Pending
    })
    .await
}
