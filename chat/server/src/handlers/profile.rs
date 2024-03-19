use {
    crate::OnlineLocation,
    chat_spec::{advance_nonce, rpcs, ChatError, FetchProfileResp, Identity, Mail, Profile, Proof},
    codec::{Codec, Reminder, ReminderOwned},
    dashmap::mapref::entry::Entry,
    libp2p::futures::StreamExt,
    std::collections::BTreeMap,
};

const MAIL_BOX_CAP: usize = 1024 * 1024;

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub async fn create(
    cx: super::Context,
    identity: Identity,
    (proof, vault, enc): (Proof<crypto::Hash>, Vec<&[u8]>, crypto::enc::PublicKey),
) -> Result<()> {
    crate::ensure!(proof.verify(), ChatError::InvalidProof);

    let vault =
        vault.into_iter().map(|b| (crypto::hash::new(b), b.to_vec())).collect::<BTreeMap<_, _>>();
    let vault_hash = vault.keys().copied().reduce(crypto::hash::combine).unwrap_or_default();
    crate::ensure!(vault_hash == proof.context, ChatError::InvalidProof);

    match cx.profiles.entry(identity) {
        Entry::Vacant(entry) => {
            entry.insert(Profile {
                sign: proof.pk,
                enc,
                vault_sig: proof.signature,
                vault_version: proof.nonce,
                mail_action: proof.nonce,
                vault,
                mail: Vec::new(),
            });
            Ok(())
        }
        Entry::Occupied(mut entry) if entry.get().vault_version < proof.nonce => {
            let account = entry.get_mut();
            account.vault_version = proof.nonce;
            account.vault_sig = proof.signature;
            account.vault.clear();
            account.vault = vault;
            Ok(())
        }
        _ => Err(ChatError::AlreadyExists),
    }
}

pub async fn update_vault(
    cx: crate::Context,
    identity: Identity,
    (proof, old_hash, value): (Proof<crypto::Hash>, crypto::Hash, Option<Reminder<'_>>),
) -> Result<()> {
    let mut profile = cx.profiles.get_mut(&identity).ok_or(ChatError::NotFound)?;

    crate::ensure!(proof.verify(), ChatError::InvalidProof);

    if old_hash != crypto::Hash::default() {
        crate::ensure!(profile.vault.contains_key(&old_hash), ChatError::NotFound);
    }

    if let Some(value) = value {
        profile.vault.insert(crypto::hash::new(value.0), Vec::new());
    }

    let hash = profile
        .vault
        .keys()
        .copied()
        .filter(|&h| h != old_hash)
        .reduce(crypto::hash::combine)
        .unwrap_or_default();
    crate::ensure!(hash == proof.context, ChatError::InvalidProof);
    crate::ensure!(
        advance_nonce(&mut profile.vault_version, proof.nonce),
        ChatError::InvalidAction
    );

    if old_hash != crypto::Hash::default() {
        profile.vault.remove(&old_hash);
    }
    if let Some(value) = value {
        profile.vault.insert(crypto::hash::new(value.0), value.0.to_vec());
    }
    profile.vault_sig = proof.signature;

    Ok(())
}

pub async fn read_mail(
    cx: super::Context,
    location: OnlineLocation,
    proof: Proof<Mail>,
) -> Result<ReminderOwned> {
    crate::ensure!(proof.verify(), ChatError::InvalidProof);

    let identity = crypto::hash::new(proof.pk);
    let profile = cx.profiles.get_mut(&identity);

    crate::ensure!(let Some(mut profile) = profile, ChatError::NotFound);
    crate::ensure!(advance_nonce(&mut profile.mail_action, proof.nonce), ChatError::InvalidAction);

    cx.online.insert(identity, location);

    Ok(ReminderOwned(profile.read_mail().to_vec()))
}

pub async fn fetch_keys(cx: super::Context, identity: Identity, _: ()) -> Result<FetchProfileResp> {
    cx.profiles.get(&identity).ok_or(ChatError::NotFound).map(|p| FetchProfileResp::from(p.value()))
}

pub async fn fetch_vault(cx: super::Context, identity: Identity, _: ()) -> Result<ReminderOwned> {
    cx.profiles
        .get(&identity)
        .ok_or(ChatError::NotFound)
        .map(|p| {
            (
                p.value().vault_version,
                p.value().mail_action,
                p.value().vault.values().collect::<Vec<_>>(),
            )
                .to_bytes()
        })
        .map(ReminderOwned)
}

pub async fn fetch_full(cx: super::Context, identity: Identity, _: ()) -> Result<ReminderOwned> {
    cx.profiles
        .get(&identity)
        .ok_or(ChatError::NotFound)
        .map(|p| p.value().to_bytes())
        .map(ReminderOwned)
}

pub async fn send_mail(
    cx: super::Context,
    origin: OnlineLocation,
    for_who: Identity,
    Reminder(mail): Reminder<'_>,
) -> Result<()> {
    let push_mail = || {
        let mut profile = cx.profiles.get_mut(&for_who).ok_or(ChatError::NotFound)?;
        crate::ensure!(profile.mail.len() + mail.len() < MAIL_BOX_CAP, ChatError::MailboxFull);
        profile.push_mail(mail);
        Ok(())
    };

    let Some(online_in) = cx.online.get(&for_who).map(|v| *v.value()) else {
        return push_mail();
    };

    match online_in {
        OnlineLocation::Local(p) => {
            crate::ensure!(OnlineLocation::Local(p) != origin, ChatError::SendingToSelf);
            crate::ensure!(!cx.push_profile_event(for_who, mail).await, ChatError::SentDirectly);
        }
        OnlineLocation::Remote(peer) => 'b: {
            if matches!(origin, OnlineLocation::Remote(_)) {
                break 'b;
            }

            let Ok(resp) = cx.send_rpc(for_who, peer, rpcs::SEND_MAIL, Reminder(mail)).await else {
                break 'b;
            };

            let Some(Err(ChatError::SentDirectly)) = <Result<()>>::decode(&mut resp.as_slice())
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
    let mut profiles = cx.repl_rpc(identity, rpcs::FETCH_PROFILE_FULL, identity).await;
    let mut latest_profile = None::<Profile>;
    while let Some((peer, Ok(resp))) = profiles.next().await {
        let Some(profile_res) = Result::<Profile>::decode(&mut resp.as_slice()) else {
            log::warn!("invalid profile encoding from {:?}", peer);
            continue;
        };

        let profile = match profile_res {
            Ok(profile) => profile,
            Err(e) => {
                log::warn!("invalid profile from {:?}: {:?}", peer, e);
                continue;
            }
        };

        if crypto::hash::new(profile.sign) != identity {
            log::warn!("invalid profile identity form {:?}", peer);
            continue;
        }

        if !profile.is_valid() {
            log::warn!("invalid profile signature from {:?}", peer);
            continue;
        }

        if let Some(best) = latest_profile.as_ref() {
            if best.vault_version < profile.vault_version {
                latest_profile = Some(profile);
            }
        } else {
            latest_profile = Some(profile);
        }
    }

    let Some(profile) = latest_profile else {
        log::warn!("no valid profile found for {:?}", identity);
        // we keep convention of not found errors being the first variant
        return Err(ChatError::NotFound);
    };

    cx.profiles.insert(identity, profile);

    Ok(())
}
