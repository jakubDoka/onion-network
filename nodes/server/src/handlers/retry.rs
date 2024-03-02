use {
    super::{codec, CallId, Codec, Handler, HandlerResult, ProtocolResult, Scope, TryUnwrap},
    crate::handlers::Chat,
    chat_spec::{
        unpack_messages_ref, BlockNumber, Identity, Member, PossibleTopic, ReplVec,
        ToPossibleTopic, REPLICATION_FACTOR,
    },
    component_utils::{FindAndRemove, Reminder},
    std::{
        borrow::Cow,
        collections::{hash_map::Entry, HashMap},
        marker::PhantomData,
        ops::Range,
        usize,
    },
};

fn compute_message_owerflow_tolerance(median: usize) -> usize {
    (median / 8).min(10).max(2)
}

pub type Retry<H> = RetryBase<H, <H as Handler>::Event>;

struct BlockData {
    block_number: BlockNumber,
    members: HashMap<Identity, Member>,
    byte_range: Range<usize>,
}

pub struct Restoring {
    topic: PossibleTopic,
    pending: ReplVec<CallId>,
    request: Vec<u8>,
    messages: Vec<u8>,
    block_data: ReplVec<BlockData>,
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
                sc.cx.storage.chats.insert(name, Chat::default());
                FetchMinimalChatData::rpc(name)
                    .encode(&mut packet.as_mut_slice())
                    .expect("always big enough");
            }
        }

        let us = *sc.cx.swarm.local_peer_id();
        let beh = sc.cx.swarm.behaviour_mut();
        let pending = crate::other_replicators_for(&beh.dht.table, topic, us)
            .filter_map(|peer| beh.rpc.request(peer, packet).ok())
            .collect();

        log::debug!("retrying {:?} {:?}", topic, pending);

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

                log::debug!("retrying {:?} {:?}", call, r.topic);

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
                            let Some(Ok((
                                block_number,
                                Cow::Owned(members),
                                Reminder(current_block),
                            ))) = ProtocolResult::<'a, FetchMinimalChatData>::decode(
                                &mut request.as_slice(),
                            )
                            else {
                                break 'a;
                            };

                            log::debug!("retrying {:?} {:?}", block_number, current_block);

                            r.messages.extend_from_slice(current_block);
                            let end = r.messages.len();
                            r.block_data.push(BlockData {
                                block_number,
                                members,
                                byte_range: r.messages.len()..end,
                            });
                        }
                    },
                    Err(_) => {
                        todo!()
                    }
                }

                crate::ensure!(r.pending.is_empty(), Self::Restoring(r));

                if let PossibleTopic::Chat(name) = r.topic
                    && r.block_data.len() > REPLICATION_FACTOR.get() / 2
                {
                    // let mut valid_blocks =
                    //     filter_blocks(&mut sc.cx.res.hashes, &r.block_data, &r.messages);
                    // valid_blocks.sort_unstable_by_key(|r| usize::MAX - r.len());
                    // valid_blocks.truncate(1);
                    // remove_invalid_ranges(&mut r.messages, valid_blocks);

                    // let chat = sc.cx.storage.chats.entry(name).or_insert(Chat::recovered(, number, current_block));
                    todo!("decide which servers sent legitimet data");
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

fn remove_invalid_ranges(hashes: &mut Vec<u8>, mut valid_ranges: ReplVec<Range<usize>>) {
    valid_ranges.sort_unstable_by_key(|r| r.start);
    hashes.truncate(valid_ranges.last().map_or(0, |r| r.end));
    for [a, b] in valid_ranges.array_windows() {
        hashes.splice(a.end..b.start, std::iter::empty());
    }
    hashes.splice(..valid_ranges.first().map_or(0, |r| r.start), std::iter::empty());
}

fn filter_blocks(
    hashes: &mut Vec<crypto::Hash>,
    block_data: &[(BlockNumber, HashMap<Identity, Member>, usize)],
    messages: &[u8],
) -> ReplVec<Range<usize>> {
    hashes.clear();
    let mut message_bounds = block_data
        .iter()
        .scan(0, |start, &(.., end)| {
            let prev = hashes.len();
            unpack_messages_ref(&messages[*start..end])
                .map(crypto::hash::from_slice)
                .collect_into(hashes);
            *start = end;
            Some((prev..hashes.len(), *start..end))
        })
        .enumerate()
        .map(|(i, (range, block))| (i, range, block))
        .collect::<ReplVec<_>>();

    message_bounds.sort_by_key(|(_, range, _)| range.len());
    let median = message_bounds[message_bounds.len() / 2].1.len();
    let tolerance = compute_message_owerflow_tolerance(median);

    message_bounds.retain(|(_, range, _)| range.len().abs_diff(median) <= tolerance);

    message_bounds
        .iter()
        .cloned()
        .scan(&mut hashes[..], |slc, (_, r, _)| slc.take_mut(..r.len()))
        .for_each(<[_]>::sort_unstable);

    let mut hash_iters = message_bounds
        .iter()
        .map(|(_, r, _)| hashes[r.clone()].iter().peekable())
        .collect::<ReplVec<_>>();
    let mut missing_hash_conutners =
        message_bounds.iter().map(|(.., r)| (0, r.clone())).collect::<ReplVec<_>>();

    loop {
        let current = hash_iters.iter_mut().map(|i| i.peek().cloned()).collect::<ReplVec<_>>();

        let mut best_count = 0;
        let best_iter_hash = current.iter().find_map(|&hash| {
            let count = current.iter().filter(|&&h| h == hash).count();
            if count > best_count {
                best_count = count;
                hash
            } else {
                None
            }
        });

        let Some(best_iter_hash) = best_iter_hash else {
            for ((counter, ..), iters) in
                missing_hash_conutners.iter_mut().zip(hash_iters.iter_mut())
            {
                *counter += iters.count();
            }
            break;
        };

        for ((&current, (counter, ..)), iter) in
            current.iter().zip(missing_hash_conutners.iter_mut()).zip(hash_iters.iter_mut())
        {
            *counter += (current != Some(best_iter_hash)) as usize;
            while iter.peek() < Some(&best_iter_hash) && iter.next().is_some() {}
        }
    }

    missing_hash_conutners.sort_unstable_by_key(|(c, ..)| *c);
    let (median, ..) = missing_hash_conutners[missing_hash_conutners.len() / 2];
    let tolerance = compute_message_owerflow_tolerance(median);

    missing_hash_conutners
        .into_iter()
        .filter(|(c, ..)| c.abs_diff(median) <= tolerance)
        .map(|(.., r)| r)
        .collect()
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
