use {
    super::{Handler, TryUnwrap},
    crate::REPLICATION_FACTOR,
    chat_spec::{PossibleTopic, Protocol, ReplError, ReplVec, ToPossibleTopic},
    component_utils::{Codec, FindAndRemove},
    rpc::CallId,
};

pub enum Repl<H> {
    Resolving(H, PossibleTopic, Vec<u8>),
    Replicating { response: Vec<u8>, ongoing: ReplVec<CallId>, matched: usize },
}

impl<H> Repl<H> {
    pub fn new_replicating(
        response: Vec<u8>,
        request: Vec<u8>,
        topic: PossibleTopic,
        cx: crate::Context,
    ) -> Self {
        let us = *cx.swarm.local_peer_id();
        let beh = cx.swarm.behaviour_mut();
        let ongoing = crate::other_replicators_for(&beh.dht.table, topic, us)
            .filter_map(|peer| beh.rpc.request(peer, request.as_slice()).ok())
            .collect();

        Self::Replicating { response, ongoing, matched: 0 }
    }
}

// TODO: This doesnt actually consider that only we are the minority and willing to reconfigure
impl<H> Handler for Repl<H>
where
    H: Handler,
    for<'a> <H::Protocol as Protocol>::Request<'a>: ToPossibleTopic,
    for<'a> &'a H::Event: TryUnwrap<&'a rpc::Event> + TryUnwrap<&'a H::Event>,
{
    type Event = H::Event;
    type Protocol = chat_spec::Repl<H::Protocol>;

    fn execute<'a>(
        mut scope: super::Scope<'a>,
        req: <Self::Protocol as chat_spec::Protocol>::Request<'_>,
    ) -> super::HandlerResult<'a, Self> {
        let topic: PossibleTopic = req.to_possible_topic();

        if !scope.is_valid_topic(topic) {
            return Ok(Err(ReplError::InvalidTopic));
        }

        let request = (<Self::Protocol as Protocol>::PREFIX, &req).to_bytes();
        let response = match H::execute(scope.reborrow(), req) {
            Ok(Ok(r)) => Ok::<_, ()>(r).to_bytes(),
            Ok(Err(e)) => return Ok(Err(ReplError::Inner(e))),
            Err(e) => return Err(Self::Resolving(e, topic, request)),
        };

        Err(Self::new_replicating(response, request, topic, scope.cx))
    }

    fn resume<'a>(
        mut self,
        mut cx: super::Scope<'a>,
        event: &'a Self::Event,
    ) -> super::HandlerResult<'a, Self> {
        let (response, ongoing, matched) = match self {
            Self::Resolving(handler, topic, request) => {
                let response = match handler.resume(
                    cx.reborrow(),
                    event
                        .try_unwrap()
                        .ok()
                        .expect("we always use one of the provided type aliases"),
                ) {
                    Ok(Ok(r)) => Ok::<_, ()>(r).to_bytes(),
                    Ok(Err(e)) => return Ok(Err(ReplError::Inner(e))),
                    Err(h) => return Err(Self::Resolving(h, topic, request)),
                };

                return Err(Self::new_replicating(response, request, topic, cx.cx));
            }
            Self::Replicating { ref response, ref mut ongoing, ref mut matched, .. } => {
                (response, ongoing, matched)
            }
        };

        crate::ensure!(let Ok(rpc::Event::Response(_, call, res)) = TryUnwrap::<&rpc::Event>::try_unwrap(event), self);
        crate::ensure!(ongoing.find_and_remove(|c| c == call).is_some(), self);

        log::debug!("rpc event: {:?}", res);
        match res {
            Ok((remote_resp, _)) => {
                *matched += usize::from(remote_resp.as_slice() == response.as_slice());

                if *matched > REPLICATION_FACTOR.get() / 2 {
                    let Some(resp): Option<Result<_, _>> =
                        Codec::decode(&mut remote_resp.as_slice())
                    else {
                        return Ok(Err(ReplError::InvalidResponse));
                    };

                    return Ok(resp.map_err(ReplError::Inner));
                }
            }
            Err(e) => {
                log::warn!("rpc failed: {}", e);
            }
        }

        if ongoing.len() + *matched < REPLICATION_FACTOR.get() / 2 {
            return Ok(Err(ReplError::NoMajority));
        }

        Err(self)
    }
}
