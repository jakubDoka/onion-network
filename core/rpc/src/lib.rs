#![feature(extract_if)]
#![feature(let_chains)]
#![feature(iter_collect_into)]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(macro_metavar_expr)]

use {
    component_utils::{Codec, FindAndRemove, PacketReader, PacketWriter, Reminder},
    libp2p::{
        futures::{stream::SelectAll, StreamExt},
        swarm::{ConnectionId, NetworkBehaviour, StreamUpgradeError},
        PeerId,
    },
    std::{io, sync::Arc, task::Poll, time::Duration},
};

component_utils::decl_stream_protocol!(PROTOCOL_NAME = "rpc");

pub struct Stream {
    writer: PacketWriter,
    reader: PacketReader,
    inner: Option<libp2p::Stream>,
    peer: PeerId,
    last_packet: std::time::Instant,
}

type IsRequest = bool;

impl libp2p::futures::Stream for Stream {
    type Item = (PeerId, io::Result<(CallId, Vec<u8>, IsRequest)>);

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let Some(stream) = this.inner.as_mut() else {
            return Poll::Ready(None);
        };

        let f = this.writer.poll(cx, stream);
        if let Poll::Ready(Err(e)) = f {
            this.inner.take();
            return Poll::Ready(Some((this.peer, Err(e))));
        }

        let read = match libp2p::futures::ready!(this.reader.poll_packet(cx, stream)) {
            Ok(r) => r,
            Err(err) => {
                this.inner.take();
                return Poll::Ready(Some((this.peer, Err(err))));
            }
        };

        let Some((call, is_request, Reminder(payload))) = <_>::decode(&mut &*read) else {
            this.inner.take();
            log::warn!("invalid packet from {}, {:?}", this.peer, read);
            return Poll::Ready(Some((this.peer, Err(io::ErrorKind::InvalidData.into()))));
        };

        this.last_packet = std::time::Instant::now();
        Poll::Ready(Some((this.peer, Ok((call, payload.to_vec(), is_request)))))
    }
}

impl Stream {
    pub fn write(&mut self, call: CallId, payload: &[u8], is_request: bool) -> io::Result<()> {
        self.last_packet = std::time::Instant::now();
        self.writer
            .write_packet(&(call, is_request, Reminder(payload)))
            .ok_or(io::ErrorKind::OutOfMemory)?;
        Ok(())
    }

    pub fn close(&mut self) {
        self.inner.take();
    }

    fn new(peer: PeerId, stream: libp2p::Stream, buffer_size: usize) -> Self {
        Self {
            writer: PacketWriter::new(buffer_size),
            reader: PacketReader::default(),
            inner: Some(stream),
            peer,
            last_packet: std::time::Instant::now(),
        }
    }
}

#[derive(Default)]
pub struct Behaviour {
    config: Config,
    streams: SelectAll<Stream>,
    pending_requests: Vec<(PeerId, CallId, Vec<u8>, std::time::Instant)>,
    ongoing_requests: Vec<(CallId, PeerId, std::time::Instant)>,
    pending_repsonses: Vec<(PeerId, CallId, Vec<u8>)>,
    events: Vec<Event>,

    streaming: streaming::Behaviour,
}

impl Behaviour {
    pub fn request(
        &mut self,
        peer: PeerId,
        packet: impl AsRef<[u8]> + Into<Vec<u8>>,
    ) -> io::Result<CallId> {
        let call = CallId::new();
        if let Some(stream) = self.streams.iter_mut().find(|s| s.peer == peer) {
            self.ongoing_requests.push((call, peer, std::time::Instant::now()));
            stream.write(call, packet.as_ref(), true)?;
        } else if !self.streaming.is_resolving_stream_for(peer) {
            self.streaming.create_stream(peer);
            self.pending_requests.push((peer, call, packet.into(), std::time::Instant::now()));
        }
        Ok(call)
    }

    pub fn respond(
        &mut self,
        peer: PeerId,
        call: CallId,
        payload: impl AsRef<[u8]> + Into<Vec<u8>>,
    ) {
        if let Some(stream) = self.streams.iter_mut().find(|s| peer == s.peer) {
            _ = stream.write(call, payload.as_ref(), false);
        } else if !self.streaming.is_resolving_stream_for(peer) {
            self.streaming.create_stream(peer);
            self.pending_repsonses.push((peer, call, payload.into()));
        }
    }

    fn clean_failed_requests(&mut self, failed: PeerId, error: streaming::Error) {
        let error = Arc::new(error);
        self.pending_repsonses.retain(|(p, ..)| *p != failed);
        self.ongoing_requests
            .extract_if(|(_, p, ..)| *p == failed)
            .map(|(c, p, ..)| (c, p))
            .chain(
                self.pending_requests.extract_if(|(p, ..)| *p == failed).map(|(p, c, ..)| (c, p)),
            )
            .map(|(c, p)| Event::Response(p, c, Err(error.clone())))
            .collect_into(&mut self.events);
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = streaming::Handler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &libp2p::Multiaddr,
        _: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(streaming::Handler::new(|| PROTOCOL_NAME))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &libp2p::Multiaddr,
        _: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(streaming::Handler::new(|| PROTOCOL_NAME))
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        self.streaming.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.streaming.on_connection_handler_event(peer_id, connection_id, event);
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        while let Poll::Ready(Some((pid, res))) = self.streams.poll_next_unpin(cx) {
            match res {
                Ok((cid, content, false)) => {
                    let Some((_, peer, time)) =
                        self.ongoing_requests.find_and_remove(|(c, ..)| *c == cid)
                    else {
                        log::warn!("unexpected response {:?}", cid);
                        continue;
                    };
                    if pid != peer {
                        log::warn!("unexpected response {:?} from {:?}", cid, peer);
                        continue;
                    }
                    return Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(Event::Response(
                        pid,
                        cid,
                        Ok((content, time.elapsed())),
                    )));
                }
                Ok((cid, content, true)) => {
                    return Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(Event::Request(
                        pid, cid, content,
                    )));
                }
                Err(e) => self.clean_failed_requests(pid, StreamUpgradeError::Io(e)),
            }
        }

        loop {
            let ev = std::task::ready!(self.streaming.poll(cx));

            let libp2p::swarm::ToSwarm::GenerateEvent(ev) = ev else {
                return Poll::Ready(ev.map_out(|_| unreachable!()));
            };

            match ev {
                streaming::Event::IncomingStream(p, s)
                | streaming::Event::OutgoingStream(p, Ok(s)) => {
                    let mut stream = Stream::new(p, s, self.config.buffer_size);

                    for (peer, call, payload, time) in
                        self.pending_requests.extract_if(|(op, ..)| *op == p)
                    {
                        if let Err(err) = stream.write(call, &payload, true) {
                            self.events.push(Event::Response(
                                peer,
                                call,
                                Err(StreamUpgradeError::Io(err).into()),
                            ));
                        } else {
                            self.ongoing_requests.push((call, peer, time));
                        }
                    }

                    for (_, call, payload) in self.pending_repsonses.extract_if(|(op, ..)| *op == p)
                    {
                        _ = stream.write(call, &payload, false);
                    }

                    self.streams.push(stream);
                }
                streaming::Event::OutgoingStream(p, Err(err)) => {
                    let err = Arc::new(err);
                    for (peer, call, _) in self.pending_repsonses.extract_if(|(op, ..)| *op == p) {
                        self.events.push(Event::Response(peer, call, Err(err.clone())));
                    }
                }
            }
        }
    }
}

component_utils::gen_config! {
    ;;
    max_cached_connections: usize = 10,
    buffer_size: usize = 1 << 14,
    request_timeout: std::time::Duration = std::time::Duration::from_secs(10),
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

component_utils::gen_unique_id!(pub CallId);

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
#[allow(clippy::type_complexity)]
pub enum Event {
    Response(PeerId, CallId, Result<(Vec<u8>, Duration), Arc<streaming::Error>>),
    Request(PeerId, CallId, Vec<u8>),
}

pub struct Response {
    pub peer: PeerId,
    pub call: CallId,
    pub payload: Vec<u8>,
    pub roundtrip_time: Duration,
}

#[cfg(test)]
mod test {
    use {
        super::*,
        dht::Route,
        libp2p::{
            futures::{stream::FuturesUnordered, StreamExt},
            identity::{Keypair, PublicKey},
            multiaddr::Protocol,
            Multiaddr, Transport,
        },
        std::net::Ipv4Addr,
    };

    #[derive(NetworkBehaviour, Default)]
    struct TestBehatiour {
        rpc: Behaviour,
        dht: dht::Behaviour,
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_random_rpc_calls() {
        env_logger::init();

        let pks =
            (0..10).map(|_| libp2p::identity::ed25519::Keypair::generate()).collect::<Vec<_>>();
        let public_keys = pks.iter().map(|kp| kp.public()).collect::<Vec<_>>();
        let peer_ids =
            pks.iter().map(|kp| PublicKey::from(kp.public()).to_peer_id()).collect::<Vec<_>>();
        let servers = pks.into_iter().map(Keypair::from).enumerate().map(|(i, kp)| {
            let beh = TestBehatiour::default();
            let transport = libp2p::tcp::tokio::Transport::new(libp2p::tcp::Config::default())
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(libp2p::noise::Config::new(&kp).unwrap())
                .multiplex(libp2p::yamux::Config::default())
                .boxed();
            let mut swarm = libp2p::Swarm::new(
                transport,
                beh,
                kp.public().to_peer_id(),
                libp2p::swarm::Config::with_tokio_executor()
                    .with_idle_connection_timeout(Duration::from_secs(10)),
            );

            swarm
                .listen_on(
                    Multiaddr::empty()
                        .with(Protocol::Ip4(Ipv4Addr::LOCALHOST))
                        .with(Protocol::Tcp(3000 + i as u16)),
                )
                .unwrap();

            for (j, pk) in public_keys.iter().enumerate() {
                swarm.behaviour_mut().dht.table.insert(Route::new(
                    pk.clone(),
                    Multiaddr::empty()
                        .with(Protocol::Ip4(Ipv4Addr::LOCALHOST))
                        .with(Protocol::Tcp(3000 + j as u16)),
                ));
            }

            swarm
        });

        async fn run_server(mut swarm: libp2p::Swarm<TestBehatiour>, mut all_peers: Vec<PeerId>) {
            all_peers.retain(|p| p != swarm.local_peer_id());
            let max_pending_requests = 10;
            let mut pending_request_count = 0;
            let mut iteration = 0;
            let mut total_requests = 0;
            while total_requests < 30000 {
                if max_pending_requests > pending_request_count {
                    let peer_id = all_peers[iteration % all_peers.len()];
                    swarm.behaviour_mut().rpc.request(peer_id, [0, 0]).unwrap();
                    pending_request_count += 1;
                    total_requests += 1;
                }

                let e = libp2p::futures::select! {
                    e = swarm.select_next_some() => e,
                };

                if total_requests % 5000 == 0 {
                    log::info!("total requests: {}", total_requests);
                }

                match e {
                    libp2p::swarm::SwarmEvent::Behaviour(TestBehatiourEvent::Rpc(
                        Event::Request(peer, callid, stream),
                    )) => {
                        swarm.behaviour_mut().rpc.respond(peer, callid, stream);
                    }
                    libp2p::swarm::SwarmEvent::Behaviour(TestBehatiourEvent::Rpc(
                        Event::Response(.., Ok(_)),
                    )) => {
                        pending_request_count -= 1;
                    }
                    libp2p::swarm::SwarmEvent::Behaviour(TestBehatiourEvent::Rpc(
                        Event::Response(.., Err(e)),
                    )) => {
                        log::error!("error: {:?}", e);
                    }
                    e => {
                        log::info!("event: {:?}", e);
                    }
                }

                iteration += 1;
            }
        }

        servers
            .map(|s| run_server(s, peer_ids.clone()))
            .collect::<FuturesUnordered<_>>()
            .next()
            .await;
    }
}
