use {
    crypto::rand_core::OsRng,
    dht::Route,
    libp2p::{
        core::upgrade::Version, futures::StreamExt, multiaddr, swarm::NetworkBehaviour,
        websocket_websys, PeerId, Transport,
    },
    macroquad::prelude::*,
    opfusk::ToPeerId as _,
    std::{
        cell::RefCell,
        collections::{BTreeMap, HashSet},
        mem,
    },
    wasm_bindgen_futures::spawn_local,
};

#[derive(Default)]
struct Nodes {
    inner: Vec<Option<Node>>,
    free: Vec<usize>,
}

impl Nodes {
    fn push(&mut self, node: Node) -> usize {
        if let Some(index) = self.free.pop() {
            self.inner[index] = Some(node);
            return index;
        }

        self.inner.push(Some(node));
        self.inner.len() - 1
    }

    fn remove(&mut self, index: usize) {
        self.inner[index] = None;
        self.free.push(index);
    }

    fn get(&self, index: usize) -> Option<&Node> {
        self.inner[index].as_ref()
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut Node> {
        self.inner[index].as_mut()
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut Node)> + '_ {
        self.inner.iter_mut().enumerate().filter_map(|(i, node)| Some((i, node.as_mut()?)))
    }

    // fn len(&self) -> usize {
    //     self.inner.len()
    // }

    fn retain(&mut self, mut keep: impl FnMut(&mut Node) -> bool) {
        for (i, node) in self.inner.iter_mut().enumerate() {
            let Some(nd) = node else {
                continue;
            };
            if !keep(nd) {
                *node = None;
                self.free.push(i);
            }
        }
    }

    fn iter(&self) -> impl Iterator<Item = (usize, &Node)> + '_ {
        self.inner.iter().enumerate().filter_map(|(i, node)| Some((i, node.as_ref()?)))
    }
}

#[derive(Debug, Clone, Copy)]
struct Node {
    pid: PeerId,
    position: Vec2,
    velocity: Vec2,
    brightness: f32,
    reached: bool,
}

impl Node {
    const FRICION: f32 = 0.1;
    const LINE_THICKNESS: f32 = 5.0;
    const MIN_LINE_BRIGHTNESS: f32 = 0.1;
    const MIN_NODE_BRIGHTNESS: f32 = 0.5;
    const NODE_SIZE: f32 = 20.0;

    fn new(x: f32, y: f32, pid: PeerId) -> Self {
        Self {
            pid,
            position: Vec2::new(x, y),
            velocity: Vec2::from_angle(rand::gen_range(0.0, 2.0 * std::f32::consts::PI)) * 300.0,
            brightness: 1.0,
            reached: true,
        }
    }

    fn apply_forces(
        &mut self,
        other: &mut Self,
        time: f32,
        balanced_distance: f32,
        dont_atract: bool,
        force_strength: f32,
    ) {
        let diff = other.position - self.position;
        let dist_sq = diff.length_squared();
        let force = balanced_distance.mul_add(-balanced_distance, dist_sq).max(-1000.0);
        if force > 0.0 && dont_atract {
            return;
        }
        let normalized = diff.try_normalize().unwrap_or(Vec2::new(1.0, 0.0));
        self.velocity += normalized * force * time * force_strength;
        other.velocity -= normalized * force * time * force_strength;
    }

    fn update(&mut self, time: f32) {
        self.position += self.velocity * time;
        self.velocity *= 1.0 - Self::FRICION;
        self.brightness = time.mul_add(-0.8, self.brightness).max(Self::MIN_NODE_BRIGHTNESS);
    }

    fn draw(&self, is_client: bool) {
        let color =
            if is_client { Color::from_hex(0x001a_af72) } else { Color::from_hex(0x00cc_0000) };
        draw_circle(
            self.position.x,
            self.position.y,
            Self::NODE_SIZE * self.brightness * Self::MIN_NODE_BRIGHTNESS.recip(),
            color,
        );
    }

    fn draw_connection(
        &self,
        other: &Self,
        color: Color,
        index: usize,
        total_protocols: usize,
        brightness: f32,
    ) {
        let dir = other.position - self.position;
        let offset = (total_protocols - index) as f32 - total_protocols as f32 / 2.0;
        let dir = Vec2::new(dir.y, -dir.x);
        let offset = dir.normalize() * offset * Self::LINE_THICKNESS;
        draw_line(
            self.position.x + offset.x,
            self.position.y + offset.y,
            other.position.x + offset.x,
            other.position.y + offset.y,
            Self::LINE_THICKNESS * brightness * Self::MIN_LINE_BRIGHTNESS.recip(),
            Color::new(color.r, color.g, color.b, brightness),
        );
    }
}

#[derive(Debug)]
struct Protocol {
    name: String,
    color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    start: usize,
    end: usize,
    protocol: usize,
    connection: usize,
}

type EdgeVaule = f32;

struct World {
    nodes: Nodes,
    servers: HashSet<PeerId>,
    edges: BTreeMap<Edge, EdgeVaule>,
    protocols: Vec<Protocol>,
    center_node: Node,
}

impl Default for World {
    fn default() -> Self {
        Self {
            nodes: Nodes::default(),
            servers: HashSet::new(),
            edges: BTreeMap::new(),
            protocols: vec![
                Protocol { name: "/onion/rot/0.1.0".into(), color: Color::from_hex(0x0066_00cc) },
                Protocol { name: "/onion/ksr/0.1.0".into(), color: Color::from_hex(0x00cc_ccff) },
            ],
            center_node: Node::new(0.0, 0.0, PeerId::random()),
        }
    }
}

#[allow(dead_code)]
impl World {
    fn add_node(&mut self, node: Node) -> usize {
        self.nodes.push(node)
    }

    fn add_protocol(&mut self, protocol: &str) -> usize {
        if let Some(index) = self.protocols.iter().position(|p| p.name == protocol) {
            return index;
        }

        let index = self.protocols.len();
        let rand_config = rand::gen_range(u32::MIN, u32::MAX);
        self.protocols
            .push(Protocol { name: protocol.to_owned(), color: Color::from_hex(rand_config) });
        index
    }

    fn add_edge(&mut self, edge: Edge, brightness: f32) {
        debug_assert!(edge.start != edge.end);
        self.edges.insert(edge, brightness);
    }

    fn remove_edge(&mut self, edge: Edge) {
        debug_assert!(edge.start != edge.end);
        self.edges.remove(&edge);
    }

    fn remove_node(&mut self, index: usize) {
        self.nodes.remove(index);
    }

    fn update(&mut self, time: f32) {
        self.nodes.iter_mut().for_each(|(_, node)| {
            node.reached = false;
        });
        self.edges.retain(|edge, brightness| {
            *brightness = time.mul_add(-2.0, *brightness).max(Node::MIN_LINE_BRIGHTNESS);
            self.nodes.get_mut(edge.start).map(|n| n.reached = true).is_some()
                && self.nodes.get_mut(edge.end).map(|n| n.reached = true).is_some()
        });

        let mut iter = self.nodes.inner.iter_mut();
        while let Some(node) = iter.by_ref().find_map(Option::as_mut) {
            let other = mem::take(&mut iter).into_slice();
            for other in other.iter_mut().filter_map(Option::as_mut) {
                node.apply_forces(other, time, 150.0, true, 3.0);
            }
            iter = other.iter_mut();
        }

        let (cx, cy) = (screen_width() / 2., screen_height() / 2.);
        self.center_node.position = Vec2::new(cx, cy);
        for (_, node) in self.nodes.iter_mut() {
            node.apply_forces(&mut self.center_node, time, 0.0, false, 0.01);
        }
        for (_, node) in self.nodes.iter_mut() {
            node.update(time);
        }
        self.nodes.retain(|node| node.reached || self.servers.contains(&node.pid));
    }

    fn draw(&self) {
        for (edge, &brightness) in &self.edges {
            let (Some(start), Some(end)) = (self.nodes.get(edge.start), self.nodes.get(edge.end))
            else {
                continue;
            };
            let protocol = &self.protocols[edge.protocol];
            start.draw_connection(
                end,
                protocol.color,
                edge.protocol,
                self.protocols.len(),
                brightness,
            );
        }

        for (_, node) in self.nodes.iter() {
            let is_client = !self.servers.contains(&node.pid);
            node.draw(is_client);
        }

        let mut cursor = 30.0;

        for protocol in &self.protocols {
            draw_text(&protocol.name, 10.0, cursor, 30.0, protocol.color);
            cursor += 30.0;
        }
    }
}

#[derive(Clone, Copy)]
struct WorldRc(&'static RefCell<World>);

impl Default for WorldRc {
    fn default() -> Self {
        Self(Box::leak(Box::new(RefCell::new(World::default()))))
    }
}

fn by_peer_id(nodes: &Nodes, peer: PeerId) -> Option<usize> {
    nodes.iter().find_map(|(id, node)| (node.pid == peer).then_some(id))
}

impl topology_wrapper::World for WorldRc {
    fn handle_update(&mut self, peer: PeerId, update: topology_wrapper::Update) {
        let mut s = self.0.borrow_mut();

        let (width, height) = (screen_width() / 2.0, screen_height() / 2.0);

        let index = by_peer_id(&s.nodes, peer)
            .unwrap_or_else(|| s.add_node(Node::new(width, height, peer)));
        let other = by_peer_id(&s.nodes, update.peer.0)
            .unwrap_or_else(|| s.add_node(Node::new(width, height, update.peer.0)));

        if index == other {
            return;
        }

        use topology_wrapper::Event as E;
        let (protocol, brightness) = match update.event {
            E::Stream(p) => (p, 0.5),
            E::Packet(p) => (p, 1.0),
            E::Closed(p) => {
                let protocol = s.add_protocol(p);
                s.edges.retain(|edge, _| {
                    (edge.start != index || edge.end != other)
                        && (edge.start != other || edge.end != index)
                        || edge.connection != update.connection
                        || edge.protocol != protocol
                });
                return;
            }
            E::Disconnected => {
                s.edges.retain(|edge, _| {
                    (edge.start != index || edge.end != other)
                        && (edge.start != other || edge.end != index)
                        || edge.connection != update.connection
                });
                return;
            }
        };

        let protocol = s.add_protocol(protocol);
        if let Some(node) = s.nodes.get_mut(index) {
            node.brightness = 1.0;
        }
        if let Some(node) = s.nodes.get_mut(other) {
            node.brightness = 1.0;
        }
        s.add_edge(
            Edge {
                start: index.max(other),
                end: index.min(other),
                protocol,
                connection: update.connection,
            },
            brightness,
        );
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    collector: topology_wrapper::collector::Behaviour<WorldRc>,
    dht: dht::Behaviour,
}

fn chain_node() -> String {
    // TODO: handle multiple nodes
    component_utils::build_env!(pub CHAIN_NODES);
    CHAIN_NODES.to_string()
}

#[macroquad::main("Topology-Vis")]
async fn main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Info
    })
    .unwrap();

    let kp = crypto::sign::Keypair::new(OsRng);
    let peer_id = kp.to_peer_id();
    let transport = websocket_websys::Transport::default()
        .upgrade(Version::V1)
        .authenticate(opfusk::Config::new(OsRng, kp))
        .multiplex(libp2p::yamux::Config::default())
        .boxed();
    let world = WorldRc::default();
    let behaviour = Behaviour {
        collector: topology_wrapper::collector::Behaviour::new(peer_id, world),
        dht: dht::Behaviour::default(),
    };
    let mut swarm = libp2p::Swarm::new(
        transport,
        behaviour,
        peer_id,
        libp2p::swarm::Config::with_wasm_executor(),
    );

    spawn_local(async move {
        let nodes = chain_api::Client::with_signer(&chain_node(), ())
            .await
            .unwrap()
            .list_chat_nodes()
            .await
            .unwrap();
        log::info!("detected {} nodes", nodes.len());
        for (id, node) in nodes {
            let ip = node.addr;

            let addr =
                chain_api::unpack_node_addr_offset(ip, 1).with(multiaddr::Protocol::Ws("/".into()));
            let route = Route::new(id.sign, addr);
            let peer_id = route.peer_id();
            swarm.behaviour_mut().dht.table.insert(route);
            swarm.behaviour_mut().collector.world_mut().0.borrow_mut().servers.insert(peer_id);
            swarm.behaviour_mut().collector.world_mut().0.borrow_mut().nodes.push(Node::new(
                screen_width() / 2.,
                screen_height() / 2.,
                peer_id,
            ));
            _ = swarm.dial(peer_id);
        }

        log::info!("Starting swarm");
        loop {
            let e = swarm.select_next_some().await;
            log::debug!("{:?}", e);
        }
    });

    let mut dragged_node = None;
    loop {
        {
            let mut world = world.0.borrow_mut();

            if is_mouse_button_pressed(MouseButton::Left) {
                let (x, y) = mouse_position();

                dragged_node = world.nodes.iter_mut().find_map(|(id, node)| {
                    let diff = Vec2::new(x, y) - node.position;
                    (diff.length_squared() < Node::NODE_SIZE * Node::NODE_SIZE).then_some(id)
                });
            }

            if is_mouse_button_released(MouseButton::Left) {
                dragged_node = None;
            }

            if let Some(dragged_node) = dragged_node {
                let (x, y) = mouse_position();
                if let Some(node) = world.nodes.get_mut(dragged_node) {
                    node.position = Vec2::new(x, y);
                }
            }

            world.update(get_frame_time().min(0.1));

            if let Some(dragged_node) = dragged_node {
                let (x, y) = mouse_position();
                if let Some(node) = world.nodes.get_mut(dragged_node) {
                    node.position = Vec2::new(x, y);
                }
            }

            clear_background(Color::from_hex(0x0000_0022));
            world.draw();
        }

        next_frame().await;
    }
}
