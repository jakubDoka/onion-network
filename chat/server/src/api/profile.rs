use {
    crate::{storage::profile, OnlineLocation},
    chain_api::Nonce,
    chat_spec::{rpcs, ChatError, FetchProfileResp, Identity, Mail},
    codec::{Decode, Reminder, ReminderOwned},
    crypto::proof::Proof,
    handlers::Dec,
    merkle_tree::MerkleTree,
    tokio::task::block_in_place,
};

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub async fn create(
    cx: super::Context,
    identity: Identity,
    Dec((proof, enc)): Dec<(Proof<crypto::Hash>, crypto::enc::PublicKey)>,
) -> Result<()> {
    block_in_place(|| cx.storage.create_profile(proof, enc))?;
    cx.not_found.remove(&identity.into());
    Ok(())
}

// TODO: add size checks

pub async fn insert_to_vault(
    cx: crate::Context,
    identity: Identity,
    Dec((proof, changes)): Dec<(Proof<crypto::Hash>, Vec<(crypto::Hash, Vec<u8>)>)>,
) -> Result<()> {
    block_in_place(|| cx.storage.insert_to_vault(identity, changes, proof))
}

pub async fn remove_from_vault(
    cx: crate::Context,
    identity: Identity,
    Dec((proof, key)): Dec<(Proof<crypto::Hash>, crypto::Hash)>,
) -> Result<()> {
    _ = (cx, identity, proof, key);
    Err(ChatError::Todo)
}

pub async fn fetch_vault_key(
    cx: crate::Context,
    identity: Identity,
    Dec(key): Dec<crypto::Hash>,
) -> Result<ReminderOwned> {
    _ = (cx, identity, key);
    Err(ChatError::Todo)
}

pub async fn read_mail(
    cx: super::Context,
    location: OnlineLocation,
    Dec(proof): Dec<Proof<Mail>>,
) -> Result<ReminderOwned> {
    let mail = block_in_place(|| cx.storage.read_mail(proof))?;
    cx.online.insert(proof.identity(), location);
    Ok(ReminderOwned(mail))
}

pub async fn fetch_keys(cx: super::Context, identity: Identity, _: ()) -> Result<FetchProfileResp> {
    block_in_place(|| cx.storage.get_profile(identity)?.load_keys())
}

/// the return structure is RemiderOwned<(Identity, Vec<u8>)>
pub async fn fetch_vault(cx: super::Context, identity: Identity, _: ()) -> Result<ReminderOwned> {
    block_in_place(|| cx.storage.get_profile(identity)?.fetch_vault()).map(ReminderOwned)
}

pub async fn fetch_nonces(cx: super::Context, identity: Identity, _: ()) -> Result<[Nonce; 2]> {
    block_in_place(|| cx.storage.get_profile(identity)?.nonces())
}

pub async fn fetch_full(cx: super::Context, identity: Identity, _: ()) -> Result<profile::Root> {
    block_in_place(|| cx.storage.get_profile(identity)?.root())
}

pub async fn send_mail(
    cx: super::Context,
    origin: OnlineLocation,
    for_who: Identity,
    ReminderOwned(mail): ReminderOwned,
) -> Result<()> {
    let push_mail = || block_in_place(|| cx.storage.get_profile(for_who)?.append_mail(&mail));

    let Some(online_in) = cx.online.get(&for_who).map(|v| *v.value()) else {
        return push_mail();
    };

    match online_in {
        OnlineLocation::Local(p) => {
            handlers::ensure!(OnlineLocation::Local(p) != origin, ChatError::SendingToSelf);
            handlers::ensure!(
                !cx.push_profile_event(for_who, mail.to_vec()).await,
                ChatError::SentDirectly
            );
        }
        OnlineLocation::Remote(peer) => 'b: {
            if matches!(origin, OnlineLocation::Remote(_)) {
                break 'b;
            }

            let Ok(Err(ChatError::SentDirectly)) = cx
                .send_rpc::<Result<()>>(for_who, peer, rpcs::SEND_MAIL, Reminder(mail.as_slice()))
                .await
            else {
                break 'b;
            };

            return Err(ChatError::SentDirectly);
        }
    }

    cx.online.remove(&for_who);
    push_mail()
}

pub async fn recover(cx: crate::Context, identity: Identity) -> Result<()> {
    let mut resps = cx
        .repl_rpc::<Result<profile::Root>>(identity, rpcs::FETCH_PROFILE_FULL, ())
        .await?
        .into_iter()
        .filter_map(|(p, r)| r.ok().map(|r| (p, r)))
        .filter(|(_, r)| r.sign.identity() == identity)
        .filter(|(_, r)| r.is_valid())
        .collect::<Vec<_>>();

    resps.sort_unstable_by_key(|(_, a)| a.vault_version + a.mail_action);

    let (profile, vault) = 'b: {
        for (peer, profile) in resps {
            let Ok(ReminderOwned(vault)) = cx
                .send_rpc::<Result<ReminderOwned>>(identity, peer, rpcs::FETCH_VAULT, ())
                .await
                .and_then(std::convert::identity)
                .inspect_err(|_| crate::reputation::Rep::get().rate(peer, 40))
            else {
                continue;
            };

            let mut buffer = vault.as_slice();
            let mut hashes = Vec::new();
            while let Some((id, val)) = <(crypto::Hash, &[u8])>::decode(&mut buffer) {
                hashes.push(crypto::hash::kv(&id, val));
            }
            hashes.sort_unstable();
            // FIXME: maybe we just want to compute the proof wihout allocating a tree
            let root = *hashes.into_iter().collect::<MerkleTree<_>>().root();
            if root != profile.vault_root {
                continue;
            }

            break 'b (profile, vault);
        }

        return Err(ChatError::NotFound);
    };

    cx.storage.recover_profile(profile, vault)
}
