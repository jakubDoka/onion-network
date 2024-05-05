use {
    crate::storage::chat::UNFINALIZED_BUFFER_CAP,
    chain_api::NodeIdentity,
    chat_spec::{
        rpcs, BlockNumber, ChatError, ChatEvent, ChatName, Cursor, Identity, Member, ReplVec,
        REPLICATION_FACTOR,
    },
    codec::ReminderOwned,
    crypto::proof::Proof,
    handlers::{self as hds, Dec},
    opfusk::PeerIdExt,
    std::collections::HashMap,
    tokio::task::block_in_place,
};

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub async fn create(
    cx: crate::Context,
    name: ChatName,
    Dec(identity): hds::dec!(Identity),
) -> Result<()> {
    block_in_place(|| cx.storage.create_chat(name, identity))?;
    cx.not_found.remove(&name.into());
    Ok(())
}

pub async fn add_member(
    cx: crate::Context,
    Dec((proof, identity, member)): hds::dec!(Proof<ChatName>, Identity, Member),
) -> Result<()> {
    block_in_place(|| cx.storage.get_chat(proof.context)?.add_member(proof, identity, member))?;
    cx.push_chat_event(proof.context, ChatEvent::Member(identity, member)).await;
    Ok(())
}

pub async fn update_member(
    cx: crate::Context,
    Dec((proof, identity, member)): hds::dec!(Proof<ChatName>, Identity, Member),
) -> Result<()> {
    block_in_place(|| cx.storage.get_chat(proof.context)?.update_member(proof, identity, member))?;
    cx.push_chat_event(proof.context, ChatEvent::Member(identity, member)).await;
    Ok(())
}

pub async fn kick_member(
    cx: crate::Context,
    Dec((proof, identity)): hds::dec!(Proof<ChatName>, Identity),
) -> Result<()> {
    block_in_place(|| cx.storage.get_chat(proof.context)?.kick_member(proof, identity))?;
    cx.push_chat_event(proof.context, ChatEvent::MemberRemoved(identity)).await;
    Ok(())
}

pub async fn fetch_members(
    cx: crate::Context,
    name: ChatName,
    Dec((identity, count)): hds::dec!(Identity, usize),
) -> Result<Vec<(Identity, Member)>> {
    block_in_place(|| cx.storage.get_chat(name)?.fetch_members(identity, count))
}

pub async fn send_message(
    cx: crate::Context,
    group: super::FullReplGroup,
    name: ChatName,
    Dec(proof): hds::dec!(Proof<ReminderOwned>; chat_spec::MAX_MESSAGE_SIZE),
) -> Result<()> {
    let finalization =
        block_in_place(|| cx.storage.get_chat(name)?.send_message(cx, group, &proof))?;
    if let Some(req) = finalization {
        _ = cx.repl_rpc::<()>(name, rpcs::SEND_BLOCK, req).await;
    }
    cx.push_chat_event(name, ChatEvent::Message(proof.identity(), proof.context)).await;
    Ok(())
}

pub async fn vote(
    cx: crate::Context,
    origin: super::Origin,
    group: super::ReplGroup,
    name: ChatName,
    Dec((hash, bn, agrees)): hds::dec!(crypto::Hash, BlockNumber, bool),
) -> Result<()> {
    origin.try_to_hash().filter(|c| group.contains(c)).ok_or(ChatError::NoReplicator)?;
    block_in_place(|| cx.storage.get_chat(name)?.vote(cx.local_peer_id, bn, hash, agrees))
}

pub async fn proposal(
    cx: crate::Context,
    origin: super::Origin,
    group: super::FullReplGroup,
    name: ChatName,
    Dec((number, ReminderOwned(block))): hds::dec!(BlockNumber, ReminderOwned; UNFINALIZED_BUFFER_CAP),
) -> Result<()> {
    // FIXME: make this check when processing request header
    let origin =
        origin.try_to_hash().filter(|c| group.contains(c)).ok_or(ChatError::NoReplicator)?;
    let vote =
        block_in_place(|| cx.storage.get_chat(name)?.proposal(origin, group, number, block))?;

    if let Some((base_hash, vote, error)) = vote {
        _ = cx.repl_rpc::<()>(name, rpcs::VOTE_BLOCK, (number, base_hash, vote)).await;
        if let Some(error) = error {
            return Err(error);
        }
    }

    Ok(())
}

pub async fn fetch_chat_data(
    cx: crate::Context,
    name: ChatName,
    _: (),
) -> Result<Vec<(Identity, Member)>> {
    // FIXME: stream the response
    block_in_place(|| cx.storage.get_chat(name)?.export_data())
}

pub async fn fetch_messages(
    cx: crate::Context,
    name: ChatName,
    Dec(cursor): hds::dec!(Cursor),
) -> Result<([u8; chat_spec::MAX_MESSAGE_FETCH_SIZE], Cursor)> {
    block_in_place(|| cx.storage.get_chat(name)?.fetch_messages(cursor))
}

pub async fn recover(cx: crate::Context, name: ChatName) -> Result<()> {
    let mut repl_chat_data = cx
        .repl_rpc::<Result<HashMap<Identity, Member>>>(name, rpcs::FETCH_CHAT_DATA, ())
        .await?
        .into_iter()
        .filter_map(|(p, s)| s.ok().map(|s| (p, s)))
        .collect::<ReplVec<_>>();

    if repl_chat_data.len() <= REPLICATION_FACTOR.get() / 2 {
        log::warn!("not enough data to recover chat: {:?}", name);
        cx.storage.remove_chat(name)?;
        return Err(ChatError::NotFound);
    }

    let chat_data = reconstruct_chat(&mut repl_chat_data);
    cx.storage.recover_chat(name, chat_data).map_err(Into::into)
}

fn reconstruct_chat(
    block_data: &mut ReplVec<(NodeIdentity, HashMap<Identity, Member>)>,
) -> HashMap<Identity, Member> {
    fn retain_outliers<T>(
        block_data: &mut ReplVec<T>,
        tolerance: impl Fn(usize) -> usize,
        field: impl Fn(&T) -> usize,
    ) {
        if block_data.len() <= REPLICATION_FACTOR.get() {
            return;
        }

        block_data.sort_unstable_by_key(|data| field(data));
        let median = (field(&block_data[block_data.len() / 2 - 1])
            + field(&block_data[block_data.len() / 2]))
            / 2;
        let tolerance = tolerance(median);
        block_data.retain(|data| field(data).abs_diff(median) <= tolerance);
    }

    fn retain_members(
        block_data: &mut ReplVec<(NodeIdentity, HashMap<Identity, Member>)>,
    ) -> HashMap<Identity, Member> {
        let mut member_count_map = HashMap::<Identity, ReplVec<Member>>::new();
        for (id, other) in block_data.iter().flat_map(|(_, data)| data) {
            member_count_map.entry(*id).or_default().push(*other);
        }

        member_count_map.values_mut().for_each(|v| retain_outliers(v, |_| 10, |m| m.action as _));
        member_count_map.retain(|_, v| v.len() >= REPLICATION_FACTOR.get() / 2);

        member_count_map.into_iter().map(|(id, mut v)| (id, Member::combine(&mut v))).collect()
    }

    retain_outliers(block_data, |m| (m / 10).clamp(1, 3), |(_, data)| data.len());

    retain_members(block_data)
}
