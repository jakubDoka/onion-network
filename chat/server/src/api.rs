use {
    crate::{rate_map, reputation::Rated, Context, OnlineLocation},
    chain_api::NodeIdentity,
    chat_spec::{
        rpcs, ChatError, ChatName, GroupVec, Identity, Prefix, ReplVec, Topic, REPLICATION_FACTOR,
    },
    codec::{DecodeOwned, Encode},
    handlers::CallId,
    libp2p::PeerId,
    onion::{EncryptedStream, PathId},
    opfusk::ToPeerId,
    std::future::Future,
};

pub mod chat;
pub mod profile;

type Result<T, E = ChatError> = std::result::Result<T, E>;
pub type ReplGroup = ReplVec<crypto::Hash>;
pub type FullReplGroup = GroupVec<crypto::Hash>;
pub type Origin = PeerId;

pub async fn subscribe(
    cx: Context,
    topic: Topic,
    user: PathId,
    cid: CallId,
    _: (),
) -> Result<(), ChatError> {
    match topic {
        Topic::Chat(name) if cx.storage.has_chat(name) => {
            cx.chat_subs.entry(name).or_default().insert(user, cid);
            Ok(())
        }
        Topic::Profile(id) if cx.storage.has_profile(id) => {
            cx.profile_subs.insert(id, (user, cid));
            Ok(())
        }
        _ => Err(ChatError::NotFound),
    }
}

pub async fn unsubscribe(cx: Context, topic: Topic, user: PathId, _: ()) -> Result<(), ChatError> {
    match topic {
        Topic::Chat(name) => {
            let mut subs = cx.chat_subs.get_mut(&name).ok_or(ChatError::NotFound)?;
            subs.remove(&user).ok_or(ChatError::NotFound).map(drop)
        }
        // TODO: perform access control with signature
        Topic::Profile(id) => {
            cx.profile_subs.remove(&id).ok_or(ChatError::NotFound)?;
            cx.online.remove(&id).ok_or(ChatError::NotFound).map(drop)
        }
    }
}

handlers::router! { pub client_router(Prefix, State, EncryptedStream):
    rpcs::CREATE_CHAT => chat::create.repld();
    rpcs::ADD_MEMBER => chat::add_member.repld();
    rpcs::KICK_MEMBER => chat::kick_member.repld();
    rpcs::SEND_MESSAGE => chat::send_message.repld();
    rpcs::FETCH_MESSAGES => chat::fetch_messages;
    rpcs::FETCH_MEMBERS => chat::fetch_members;
    rpcs::CREATE_PROFILE => profile::create.repld();
    rpcs::SEND_MAIL => profile::send_mail.repld();
    rpcs::READ_MAIL => profile::read_mail.repld();
    rpcs::INSERT_TO_VAULT => profile::insert_to_vault.repld();
    rpcs::REMOVE_FROM_VAULT => profile::remove_from_vault.repld();
    rpcs::FETCH_PROFILE => profile::fetch_keys.restored();
    rpcs::FETCH_VAULT => profile::fetch_vault.restored();
    rpcs::FETCH_NONCES => profile::fetch_nonces.restored();
    rpcs::FETCH_VAULT_KEY => profile::fetch_vault_key.restored();
    rpcs::SUBSCRIBE => subscribe;
    rpcs::UNSUBSCRIBE => unsubscribe;
}

handlers::router! { pub server_router(Prefix, State, libp2p::Stream):
    rpcs::CREATE_CHAT => chat::create;
    rpcs::ADD_MEMBER => chat::add_member.restored();
    rpcs::KICK_MEMBER => chat::kick_member.restored();
    rpcs::SEND_BLOCK => chat::proposal
        .rated(rate_map! { BlockNotExpected => 10, BlockUnexpectedMessages => 100, Outdated => 5 })
        .restored();
    rpcs::FETCH_CHAT_DATA => chat::fetch_chat_data.rated(rate_map!(40));
    rpcs::SEND_MESSAGE => chat::send_message.restored();
    rpcs::VOTE_BLOCK => chat::vote
        .rated(rate_map! { NoReplicator => 50, NotFound => 5, AlreadyVoted => 30 }).restored();
    rpcs::CREATE_PROFILE => profile::create;
    rpcs::SEND_MAIL => profile::send_mail.restored();
    rpcs::READ_MAIL => profile::read_mail.restored();
    rpcs::INSERT_TO_VAULT => profile::insert_to_vault.restored();
    rpcs::REMOVE_FROM_VAULT => profile::remove_from_vault.restored();
    rpcs::FETCH_PROFILE_FULL => profile::fetch_full.rated(rate_map!(10));
    rpcs::FETCH_VAULT => profile::fetch_vault.rated(rate_map!(100));
}

pub struct State {
    pub location: OnlineLocation,
    pub context: Context,
    pub topic: Topic,
    pub id: CallId,
    pub prefix: Prefix,
}

handlers::quick_impl_from_request! {State => [
    Context => |state| state.context,
    FullReplGroup => |state| state.context.dht
        .read()
        .closest::<{REPLICATION_FACTOR.get() + 1}>(state.topic.as_bytes()),
    ReplGroup => |state| state.context.dht
        .read()
        .closest::<{REPLICATION_FACTOR.get() + 1}>(state.topic.as_bytes())
        .into_iter()
        .filter(|&r| NodeIdentity::from(r) != state.context.local_peer_id)
        .collect(),
    Topic => |state| state.topic,
    Identity => |state| match state.topic {
        Topic::Profile(i) => i,
        _ => return None,
    },
    ChatName => |state| match state.topic {
        Topic::Chat(c) => c,
        _ => return None,
    },
    Prefix => |state| state.prefix,
    OnlineLocation => |state| state.location,
    Origin => |state| match state.location {
        OnlineLocation::Local(_) => return None,
        OnlineLocation::Remote(p) => p.to_peer_id(),
    },
    PathId => |state| match state.location {
        OnlineLocation::Local(p) => p,
        _ => return None,
    },
    CallId => |state| state.id,
]}

pub trait Handler: Sized {
    fn repld(self) -> Repl<Self> {
        Repl { handler: self }
    }

    // fn repl_max(self) -> ReplMax<Self> {
    //     ReplMax { handler: self }
    // }

    fn restored(self) -> Restore<Self> {
        Restore { handler: self }
    }

    fn rated<F>(self, rater: F) -> Rated<Self, F> {
        Rated::new(self, rater)
    }
}

impl<H> Handler for H {}

//#[derive(Clone)]
//pub struct ReplMax<H> {
//    handler: H,
//}
//
//impl<C, H, S, A, B, O> handlers::Handler<C, S, ReplArgs<A>, B> for ReplMax<H>
//where
//    H: handlers::Handler<C, S, A, B>,
//    H::Future: Future<Output = Result<O>>,
//    B: Encode + Send,
//    A: Send,
//    O: Ord,
//    <H::Future as Future>::Output: DecodeOwned + Send + Sync + Eq,
//{
//    type Future = impl Future<Output = Result<O, ChatError>> + Send;
//
//    fn call(self, (topic, cx, prefix, args): ReplArgs<A>, req: B) -> Self::Future {
//        async move {
//            let req_bytes = req.to_bytes();
//            let us = self.handler.call(args, req).await;
//
//            if matches!(us, Err(ChatError::SentDirectly)) {
//                return us;
//            }
//
//            let others = cx.repl_rpc_low::<Result<O>>(topic, prefix, &req_bytes).await?;
//            let us = (cx.local_peer_id, us);
//
//            let (peer, resp) = others
//                .into_iter()
//                .chain(std::iter::once(us))
//                // note we go last so if we are one of the best picks we will be chosen
//                .max_by(|(_, a), (_, b)| match (a, b) {
//                    (Ok(a), Ok(b)) => a.cmp(b),
//                    (Err(_), Ok(_)) => std::cmp::Ordering::Less,
//                    (Ok(_), Err(_)) => std::cmp::Ordering::Greater,
//                    (Err(_), Err(_)) => std::cmp::Ordering::Equal,
//                })
//                .unwrap();
//
//            if peer != cx.local_peer_id {
//                match topic {
//                    Topic::Profile(pf) => profile::recover(cx, pf).await?,
//                    Topic::Chat(chat) => chat::recover(cx, chat).await?,
//                }
//            }
//
//            resp
//        }
//    }
//}

#[derive(Clone)]
pub struct Repl<H> {
    handler: H,
}

pub type ReplArgs<A> = (Topic, Context, Prefix, A);

impl<C, H, S, A, B, O> handlers::Handler<C, S, ReplArgs<A>, B> for Repl<H>
where
    H: handlers::Handler<C, S, A, B>,
    H::Future: Future<Output = Result<O>>,
    B: Encode + Send,
    A: Send,
    <H::Future as Future>::Output: DecodeOwned + Send + Sync + Eq,
{
    type Future = impl Future<Output = Result<O, ChatError>> + Send;

    fn call(self, (topic, cx, prefix, args): ReplArgs<A>, req: B) -> Self::Future {
        async move {
            let req_bytes = req.to_bytes();
            let us = self.handler.call(args, req).await;

            if matches!(us, Err(ChatError::SentDirectly)) {
                return us;
            }

            let others = cx.repl_rpc_low::<Result<O>>(topic, prefix, &req_bytes).await?;
            let us = (cx.local_peer_id, us);

            let mut resps = others.into_iter().chain(std::iter::once(us)).collect::<GroupVec<_>>();

            let mut recovering = false;
            let mut pick = None;
            while let Some((peer, base)) = resps.pop() {
                let mut others = GroupVec::from_iter([peer]);
                resps.retain(|(p, other)| (&base == other).then(|| others.push(*p)).is_none());

                if others.len() > REPLICATION_FACTOR.get() / 2 {
                    pick = Some(base);
                    for peer in others {
                        crate::reputation::Rep::get().rate(peer, -1);
                    }
                } else {
                    if others.contains(&cx.local_peer_id) {
                        recovering = cx.start_recovery(topic);
                    }

                    for peer in others {
                        crate::reputation::Rep::get().rate(peer, 10);
                    }
                }
            }

            if recovering && pick.is_none() {
                return Err(ChatError::OngoingRecovery);
            }

            pick.ok_or(ChatError::NoMajority)?
        }
    }
}

#[derive(Clone)]
pub struct Restore<H> {
    pub handler: H,
}

pub type RestoreArgsArgs<A> = (Context, Topic, A);

impl<C, H, S, A, B, O> handlers::Handler<C, S, RestoreArgsArgs<A>, B> for Restore<H>
where
    H: handlers::Handler<C, S, A, B>,
    H::Future: Future<Output = Result<O>>,
    S: handlers::Stream,
    RestoreArgsArgs<A>: handlers::FromContext<C>,
    A: handlers::FromContext<C>,
    B: handlers::FromStream<S>,
{
    type Future = impl Future<Output = <H::Future as Future>::Output> + Send;

    fn call(self, (cx, topic, args): RestoreArgsArgs<A>, req: B) -> Self::Future {
        async move {
            if cx.should_recover(topic) {
                cx.start_recovery(topic);
                return Err(ChatError::OngoingRecovery);
            }

            self.handler.call(args, req).await
        }
    }
}
