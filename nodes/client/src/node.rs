use {
    crate::{protocol::*, BootPhase, UserKeys},
    anyhow::Context,
    chain_api::RawUserName,
    chat_spec::{
        rpcs, username_to_raw, CallId, ChatName, Identity, Nonce, Proof, RawChatName, Topic,
        UserName, REPLICATION_FACTOR,
    },
    component_utils::{Codec, FindAndRemove, LinearMap, Reminder},
    crypto::{
        decrypt,
        enc::{self, ChoosenCiphertext, Ciphertext},
        sign, FixedAesPayload, Serialized, TransmutationCircle,
    },
    dht::Route,
    leptos::signal_prelude::*,
    libp2p::{
        core::upgrade::Version,
        futures::{SinkExt, StreamExt},
        identity::ed25519,
        swarm::{NetworkBehaviour, SwarmEvent},
        *,
    },
    onion::{EncryptedStream, PathId, SharedSecret},
    rand::{rngs::OsRng, seq::IteratorRandom},
    std::{
        collections::{HashMap, HashSet},
        io,
        net::IpAddr,
        task::Poll,
        time::Duration,
    },
    web_sys::wasm_bindgen::JsValue,
};

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
pub enum Mail<'a> {
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

impl ChatMeta {
    pub fn new() -> Self {
        Self::from_secret(crypto::new_secret(OsRng))
    }

    pub fn from_secret(secret: SharedSecret) -> Self {
        Self { secret, action_no: Default::default() }
    }
}

pub fn try_set_color(name: &str, value: u32) -> Result<(), JsValue> {
    leptos::document()
        .body()
        .ok_or("no body")?
        .style()
        .set_property(name, &format!("#{:08x}", value))
}

pub fn try_load_color_from_style(name: &str) -> Result<u32, JsValue> {
    u32::from_str_radix(
        leptos::document()
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
    pending_requests: LinearMap<CallId, libp2p::futures::channel::oneshot::Sender<RawResponse>>,
    pending_topic_search: LinearMap<PathId, Vec<RequestInit>>,
    requests: RequestStream,
}

impl Node {
    pub async fn new(
        keys: UserKeys,
        wboot_phase: WriteSignal<Option<BootPhase>>,
    ) -> anyhow::Result<(Self, Vault, RequestDispatch, Nonce, Nonce)> {
        macro_rules! set_state { ($($t:tt)*) => {wboot_phase(Some(BootPhase::$($t)*))}; }

        set_state!(FetchNodesAndProfile);

        let (mut request_dispatch, commands) = RequestDispatch::new();
        let chain_api = crate::chain::node(keys.name).await?;
        let node_request = chain_api.list(crate::chain::node_contract());
        let profile_request = chain_api
            .get_profile_by_name(crate::chain::user_contract(), username_to_raw(keys.name));
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
            nodes.len() >= crate::chain::min_nodes(),
            "not enough nodes in network, needed {}, got {}",
            crate::chain::min_nodes(),
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

    pub async fn run(mut self) -> anyhow::Result<()> {
        loop {
            futures::select! {
                event = self.swarm.select_next_some() => self.handle_swarm_event(event),
                command = self.requests.select_next_some() => self.handle_command(command),
                (id, response) = self.subscriptions.select_next_some() => self.handle_subscription_response(id, response).await,
            }
        }
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

    async fn handle_subscription_response(&mut self, id: PathId, request: io::Result<Vec<u8>>) {
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
            if channel.send(content.to_owned()).await.is_err() {
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
    subscriptions: LinearMap<CallId, libp2p::futures::channel::mpsc::Sender<SubscriptionMessage>>,
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
