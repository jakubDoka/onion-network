use {
    crate::{rate_map, reputation::Rated, Context, OnlineLocation},
    chain_api::NodeIdentity,
    chat_spec::{
        rpcs, ChatError, ChatName, GroupVec, Identity, ReplVec, Topic, REPLICATION_FACTOR,
    },
    codec::{DecodeOwned, Encode},
    dht::U256,
    handlers::CallId,
    libp2p::PeerId,
    onion::PathId,
    opfusk::ToPeerId,
    std::future::Future,
};

pub mod chat;
pub mod profile;

type Result<T, E = ChatError> = std::result::Result<T, E>;
pub type ReplGroup = ReplVec<U256>;
pub type FullReplGroup = GroupVec<U256>;
pub type Prefix = u8;
pub type Origin = PeerId;

pub async fn subscribe(
    cx: Context,
    topic: Topic,
    user: PathId,
    loc: OnlineLocation,
    cid: CallId,
    _: (),
) -> Result<(), ChatError> {
    match topic {
        Topic::Chat(name) if cx.chats.contains_key(&name) => {
            cx.chat_subs.entry(name).or_default().insert(user, cid);
            Ok(())
        }
        Topic::Profile(id) => {
            cx.profile_subs.insert(id, (user, cid));
            cx.online.insert(id, loc);
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

handlers::router! { pub client_router(State):
    rpcs::CREATE_CHAT => chat::create.repl();
    rpcs::ADD_MEMBER => chat::add_member.repl();
    rpcs::KICK_MEMBER => chat::kick_member.repl();
    rpcs::SEND_MESSAGE => chat::send_message.repl();
    rpcs::FETCH_MESSAGES => chat::fetch_messages;
    rpcs::FETCH_MEMBERS => chat::fetch_members;
    rpcs::CREATE_PROFILE => profile::create.repl();
    rpcs::SEND_MAIL => profile::send_mail.repl();
    rpcs::READ_MAIL => profile::read_mail.repl();
    rpcs::INSERT_TO_VAULT => profile::insert_to_vault.repl();
    rpcs::REMOVE_FROM_VAULT => profile::remove_from_vault.repl();
    rpcs::FETCH_PROFILE => profile::fetch_keys;
    rpcs::FETCH_VAULT => profile::fetch_vault;
    rpcs::FETCH_NONCES => profile::fetch_nonces;
    rpcs::FETCH_VAULT_KEY => profile::fetch_vault_key;
    rpcs::SUBSCRIBE => subscribe;
    rpcs::UNSUBSCRIBE => unsubscribe;
}

handlers::router! { pub server_router(State):
    rpcs::CREATE_CHAT => chat::create;
    rpcs::ADD_MEMBER => chat::add_member;
    rpcs::KICK_MEMBER => chat::kick_member;
    rpcs::SEND_BLOCK => chat::handle_message_block
        .rated(rate_map! { BlockNotExpected => 10, BlockUnexpectedMessages => 100, Outdated => 5 });
    rpcs::FETCH_CHAT_DATA => chat::fetch_chat_data.rated(rate_map!(40));
    rpcs::SEND_MESSAGE => chat::send_message;
    rpcs::VOTE_BLOCK => chat::vote
        .rated(rate_map! { NoReplicator => 50, NotFound => 5, AlreadyVoted => 30 });
    rpcs::CREATE_PROFILE => profile::create;
    rpcs::SEND_MAIL => profile::send_mail;
    rpcs::READ_MAIL => profile::read_mail;
    rpcs::INSERT_TO_VAULT => profile::insert_to_vault;
    rpcs::REMOVE_FROM_VAULT => profile::remove_from_vault;
    rpcs::FETCH_PROFILE_FULL => profile::fetch_full.rated(rate_map!(40));
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
    fn repl(self) -> Repl<Self> {
        Repl { handler: self }
    }

    // fn repl_max(self) -> ReplMax<Self> {
    //     ReplMax { handler: self }
    // }

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

            let mut pick = None;
            while let Some((peer, base)) = resps.pop() {
                let prev_len = resps.len() + 1;
                let mut others = GroupVec::from_iter([peer]);
                resps.retain(|(_, other)| (&base == other).then(|| others.push(peer)).is_none());

                let count = prev_len - resps.len();
                if count > REPLICATION_FACTOR.get() / 2 {
                    pick = Some(base);
                    for peer in others {
                        crate::reputation::Rep::get().rate(peer, -1);
                    }
                } else {
                    if others.contains(&cx.local_peer_id) {
                        match topic {
                            Topic::Profile(pf) => profile::recover(cx, pf).await?,
                            Topic::Chat(chat) => chat::recover(cx, chat).await?,
                        }
                    }

                    for peer in others {
                        crate::reputation::Rep::get().rate(peer, 10);
                    }
                }
            }

            pick.ok_or(ChatError::NoMajority)?
        }
    }
}
