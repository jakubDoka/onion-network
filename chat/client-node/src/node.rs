use {
    crate::{
        chain::{chain_node, min_nodes},
        timeout, RawRequest, RawResponse, RequestInit, RequestStream, Requests, SubscriptionInit,
        SubscriptionMessage, UserKeys, Vault,
    },
    anyhow::Context,
    chat_spec::*,
    codec::{Codec, Reminder},
    component_utils::FindAndRemove,
    crypto::{
        enc,
        proof::{Nonce, Proof},
        sign,
    },
    dht::Route,
    libp2p::{
        core::upgrade::Version,
        futures::{
            channel::{mpsc, oneshot},
            StreamExt,
        },
        identity::ed25519,
        swarm::{NetworkBehaviour, SwarmEvent},
        *,
    },
    onion::{EncryptedStream, PathId},
    rand::{rngs::OsRng, seq::IteratorRandom},
    std::{
        collections::{BTreeMap, HashMap, HashSet},
        convert::Infallible,
        future::Future,
        io,
        net::IpAddr,
        pin,
        task::Poll,
        time::Duration,
    },
};

pub struct Node {
    swarm: Swarm<Behaviour>,
    subscriptions: futures::stream::SelectAll<Subscription>,
    pending_requests: HashMap<CallId, oneshot::Sender<RawResponse>>,
    pending_topic_search: HashMap<PathId, Vec<RequestInit>>,
    requests: RequestStream,
}

impl Node {
    pub async fn new(
        keys: UserKeys,
        mut wboot_phase: impl FnMut(BootPhase),
    ) -> anyhow::Result<(Self, Vault, Requests, Nonce, Nonce)> {
        macro_rules! set_state { ($($t:tt)*) => {_ = wboot_phase(BootPhase::$($t)*)}; }

        set_state!(FetchNodesAndProfile);

        let (mut request_dispatch, commands) = Requests::new();
        let chain_api = chain_node(keys.name).await?;
        let node_request = chain_api.list_nodes();
        let profile_request = chain_api.get_profile_by_name(username_to_raw(keys.name));
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
        let transport = websocket_websys::Transport::default()
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

        fn unpack_node_id(id: sign::Pre) -> anyhow::Result<ed25519::PublicKey> {
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
            .map(|stake| {
                let id = unpack_node_id(stake.id).unwrap();
                let addr = unpack_node_addr(stake.addr);
                Ok(Route::new(id, addr))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        swarm.behaviour_mut().dht.table.bulk_insert(nodes);

        let routes = swarm.behaviour_mut().dht.table.iter().map(Route::peer_id).collect::<Vec<_>>();
        for route in routes {
            _ = swarm.dial(route);
        }

        _ = timeout(
            async {
                loop {
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
        let (mut vault_nonce, mail_action, vault) = match request_dispatch
            .dispatch_direct::<(Nonce, Nonce, BTreeMap<crypto::Hash, Vec<u8>>)>(
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

        log::info!("{:?}", vault);

        let vault = if vault.is_empty() && vault_nonce == 0 {
            set_state!(ProfileCreate);
            let proof = Proof::new(&keys.sign, &mut vault_nonce, crypto::Hash::default(), OsRng);
            request_dispatch
                .dispatch_direct(
                    &mut profile_stream,
                    rpcs::CREATE_PROFILE,
                    Topic::Profile(profile_hash.sign),
                    &(proof, "", keys.enc.public_key()),
                )
                .await
                .context("creating account")?;

            Vault::default()
        } else {
            Vault::deserialize(vault, keys.vault)
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
                    let Ok((stream, peer_id)) = stream else { return };

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

    pub fn pending_topic_search(&self) -> &HashMap<PathId, Vec<RequestInit>> {
        &self.pending_topic_search
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
    subscriptions: HashMap<CallId, mpsc::Sender<SubscriptionMessage>>,
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
