use {
    super::{codec, CallId, Codec, Handler, HandlerResult, ProtocolResult, Scope, TryUnwrap},
    chat_spec::{
        unpack_messages_ref, BlockNumber, FetchFullProfile, FetchLatestBlock, Identity,
        PossibleTopic, Protocol, ReplVec, ToPossibleTopic, REPLICATION_FACTOR,
    },
    component_utils::{FindAndRemove, Reminder},
    std::{collections::hash_map::Entry, marker::PhantomData},
};

fn compute_message_owerflow_tolerance(median: usize) -> usize {
    (median / 8).min(10).max(2)
}

pub type Retry<H> = RetryBase<H, <H as Handler>::Event>;

pub struct Restoring {
    topic: PossibleTopic,
    pending: ReplVec<CallId>,
    request: Vec<u8>,
    messages: Vec<u8>,
    block_data: ReplVec<(BlockNumber, usize)>,
}

pub enum RetryBase<H, E> {
    Handling(H, PossibleTopic, PhantomData<fn(&E)>),
    Restoring(Restoring),
}

impl<H, E> Handler for RetryBase<H, E>
where
    H: Handler,
    for<'a> <H::Protocol as Protocol>::Request<'a>: ToPossibleTopic,
    for<'a> &'a E: TryUnwrap<&'a rpc::Event> + TryUnwrap<&'a H::Event>,
    H::Event: 'static,
{
    type Event = E;
    type Protocol = NotFound<H::Protocol>;

    fn execute<'a>(
        sc: Scope<'a>,
        req: <Self::Protocol as Protocol>::Request<'_>,
    ) -> HandlerResult<'a, Self> {
        let topic = req.to_possible_topic();

        crate::ensure!(sc.cx.is_valid_topic(topic), Ok(NotFoundError::NotFound));

        let mut packet = [0u8; std::mem::size_of::<(u8, Identity)>() + 1];
        match topic {
            PossibleTopic::Profile(identity) if sc.cx.storage.profiles.contains_key(&identity) => {
                return H::execute(sc, req)
                    .map_err(|h| Self::Handling(h, topic, PhantomData))
                    .map(|r| r.map_err(NotFoundError::Inner))
            }
            PossibleTopic::Profile(identity) => {
                FetchFullProfile::rpc(identity)
                    .encode(&mut packet.as_mut_slice())
                    .expect("always big enough");
            }
            PossibleTopic::Chat(name) if sc.cx.storage.chats.contains_key(&name) => {
                return H::execute(sc, req)
                    .map_err(|h| Self::Handling(h, topic, PhantomData))
                    .map(|r| r.map_err(NotFoundError::Inner))
            }
            PossibleTopic::Chat(name) => {
                //sc.cx.storage.chats.insert(name);
                FetchLatestBlock::rpc(name)
                    .encode(&mut packet.as_mut_slice())
                    .expect("always big enough");
            }
        }

        let us = *sc.cx.swarm.local_peer_id();
        let beh = sc.cx.swarm.behaviour_mut();
        let pending = crate::other_replicators_for(&beh.dht.table, topic, us)
            .filter_map(|peer| beh.rpc.request(peer, packet).ok())
            .collect();

        Err(Self::Restoring(Restoring {
            topic,
            pending,
            request: req.to_bytes(),
            messages: Default::default(),
            block_data: Default::default(),
        }))
    }

    fn resume<'a>(self, sc: Scope<'a>, event: &'a Self::Event) -> HandlerResult<'a, Self> {
        match self {
            Self::Handling(h, topic, ph) => {
                crate::ensure!(let Ok(event) = TryUnwrap::<&'a H::Event>::try_unwrap(event), Self::Handling(h, topic, ph));
                H::resume(h, sc, event)
                    .map_err(|h| Self::Handling(h, topic, ph))
                    .map(|e| e.map_err(NotFoundError::Inner))
            }
            Self::Restoring(mut r) => {
                crate::ensure!(let Ok(&rpc::Event::Response(_, call, ref res)) = event.try_unwrap(), Self::Restoring(r));
                crate::ensure!(
                    r.pending.find_and_remove(|&c| c == call).is_some(),
                    Self::Restoring(r)
                );

                match res {
                    Ok((request, ..)) => match r.topic {
                        PossibleTopic::Profile(identity) => 'a: {
                            let Some(Ok(profile)) = ProtocolResult::<'a, FetchFullProfile>::decode(
                                &mut request.as_slice(),
                            ) else {
                                break 'a;
                            };

                            if crypto::hash::from_raw(&profile.sign) != identity {
                                break 'a;
                            }

                            if !profile.is_valid() {
                                break 'a;
                            }

                            let entry = sc.cx.storage.profiles.entry(identity);
                            if let Entry::Occupied(existing) = &entry
                                && existing.get().vault_version >= profile.vault_version
                            {
                                break 'a;
                            }

                            entry.insert_entry(profile.into());
                        }
                        PossibleTopic::Chat(_name) => 'a: {
                            let Some(Ok((block_number, Reminder(current_block)))) =
                                ProtocolResult::<'a, FetchLatestBlock>::decode(
                                    &mut request.as_slice(),
                                )
                            else {
                                break 'a;
                            };

                            r.messages.extend_from_slice(current_block);
                            let end = r.messages.len();
                            r.block_data.push((block_number, end));
                        }
                    },
                    Err(_) => {
                        todo!()
                    }
                }

                crate::ensure!(r.pending.is_empty(), Self::Restoring(r));

                if let PossibleTopic::Chat(_name) = r.topic
                    && r.block_data.len() > REPLICATION_FACTOR.get() / 2
                {
                    sc.cx.res.hashes.clear();
                    let mut message_bounds = r
                        .block_data
                        .iter()
                        .scan(0, |start, &(_, end)| {
                            let prev = sc.cx.res.hashes.len();
                            unpack_messages_ref(&r.messages[*start..end])
                                .map(crypto::hash::from_slice)
                                .collect_into(&mut sc.cx.res.hashes);
                            *start = end;
                            Some(prev..sc.cx.res.hashes.len())
                        })
                        .enumerate()
                        .collect::<ReplVec<_>>();

                    message_bounds.sort_by_key(|(_, range)| range.len());
                    let median = message_bounds[message_bounds.len() / 2].1.len();
                    let tolerance = compute_message_owerflow_tolerance(median);

                    message_bounds.retain(|(_, range)| range.len().abs_diff(median) <= tolerance);

                    message_bounds
                        .iter()
                        .cloned()
                        .scan(&mut sc.cx.res.hashes[..], |slc, (_, r)| slc.take_mut(..r.len()))
                        .for_each(<[_]>::sort_unstable);
                }

                let req = <Self::Protocol as Protocol>::Request::decode(&mut r.request.as_slice())
                    .expect("always valid");
                H::execute(sc, req)
                    .map_err(|h| Self::Handling(h, r.topic, PhantomData))
                    .map(|r| r.map_err(NotFoundError::Inner))
            }
        }
    }
}

pub struct NotFound<T: Protocol>(T);

impl<T: Protocol> Protocol for NotFound<T> {
    type Error = NotFoundError<T::Error>;
    type Request<'a> = T::Request<'a>;
    type Response<'a> = T::Response<'a>;

    const PREFIX: u8 = T::PREFIX;
}

#[derive(Debug, thiserror::Error)]
pub enum NotFoundError<T> {
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Inner(T),
}

impl<'a, T: Codec<'a>> Codec<'a> for NotFoundError<T> {
    fn encode(&self, buf: &mut impl codec::Buffer) -> Option<()> {
        match self {
            Self::NotFound => buf.push(0),
            Self::Inner(e) => e.encode(buf),
        }
    }

    fn decode(buf: &mut &'a [u8]) -> Option<Self> {
        match buf.take_first()? {
            0 => Some(Self::NotFound),
            _ => Some(Self::Inner(T::decode(buf)?)),
        }
    }
}
