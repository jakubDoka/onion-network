use {
    crate::OnlineLocation,
    chat_spec::{advance_nonce, rpcs, ChatError, FetchProfileResp, Identity, Mail, Profile, Vault},
    codec::{Encode, Reminder, ReminderOwned},
    crypto::proof::Proof,
    dashmap::mapref::entry::Entry,
    handlers::{Dec, DecFixed},
    std::collections::BTreeMap,
};

const MAIL_BOX_CAP: usize = 1024 * 1024;

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub async fn create(
    cx: super::Context,
    identity: Identity,
    Dec((proof, values, enc)): Dec<(
        Proof<crypto::Hash>,
        BTreeMap<crypto::Hash, Vec<u8>>,
        crypto::enc::PublicKey,
    )>,
) -> Result<()> {
    let mut vault = Vault {
        version: proof.nonce,
        sig: proof.signature,
        values,
        merkle_tree: Default::default(),
    };
    vault.recompute();
    handlers::ensure!(vault.is_valid(proof.pk), ChatError::InvalidProof);

    match cx.profiles.entry(identity) {
        Entry::Vacant(entry) => {
            entry.insert(Profile {
                sign: proof.pk,
                enc,
                vault,
                mail_action: proof.nonce,
                mail: Vec::new(),
            });
            Ok(())
        }
        Entry::Occupied(mut entry) if entry.get().vault.version < proof.nonce => {
            entry.get_mut().vault = vault;
            Ok(())
        }
        _ => Err(ChatError::AlreadyExists),
    }
}

// TODO: add size checks

pub async fn insert_to_vault(
    cx: crate::Context,
    identity: Identity,
    Dec((proof, changes)): Dec<(Proof<crypto::Hash>, Vec<(crypto::Hash, Vec<u8>)>)>,
) -> Result<()> {
    cx.profiles
        .get_mut(&identity)
        .ok_or(ChatError::NotFound)?
        .vault
        .try_insert_bulk(changes, proof)
        .then_some(())
        .ok_or(ChatError::InvalidProof)
}

pub async fn remove_from_vault(
    cx: crate::Context,
    identity: Identity,
    Dec((proof, key)): Dec<(Proof<crypto::Hash>, crypto::Hash)>,
) -> Result<()> {
    cx.profiles
        .get_mut(&identity)
        .ok_or(ChatError::NotFound)?
        .vault
        .try_remove(key, proof)
        .then_some(())
        .ok_or(ChatError::InvalidProof)
}

pub async fn fetch_vault_key(
    cx: crate::Context,
    identity: Identity,
    DecFixed(key): DecFixed<crypto::Hash>,
) -> Result<ReminderOwned> {
    let profile = cx.profiles.get(&identity).ok_or(ChatError::NotFound)?;
    profile.vault.values.get(&key).ok_or(ChatError::NotFound).cloned().map(ReminderOwned)
}

pub async fn read_mail(
    cx: super::Context,
    location: OnlineLocation,
    Dec(proof): Dec<Proof<Mail>>,
) -> Result<ReminderOwned> {
    handlers::ensure!(proof.verify(), ChatError::InvalidProof);

    let identity = crypto::hash::new(proof.pk);
    let profile = cx.profiles.get_mut(&identity);

    handlers::ensure!(let Some(mut profile) = profile, ChatError::NotFound);
    handlers::ensure!(
        advance_nonce(&mut profile.mail_action, proof.nonce),
        ChatError::InvalidAction
    );

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
            (p.value().vault.version, p.value().mail_action, &p.value().vault.values).to_bytes()
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
    ReminderOwned(mail): ReminderOwned,
) -> Result<()> {
    let push_mail = || {
        let mut profile = cx.profiles.get_mut(&for_who).ok_or(ChatError::NotFound)?;
        handlers::ensure!(profile.mail.len() + mail.len() < MAIL_BOX_CAP, ChatError::MailboxFull);
        profile.push_mail(&mail);
        Ok(())
    };

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
    let profile = cx
        .repl_rpc::<Result<Profile>>(identity, rpcs::FETCH_PROFILE_FULL, identity)
        .await?
        .into_iter()
        .filter_map(|(_, r)| r.ok())
        .filter(|r| r.sign.identity() == identity)
        .filter(|r| r.is_valid())
        .max_by_key(|r| r.vault.version)
        .ok_or(ChatError::NotFound)?;
    cx.profiles.insert(identity, profile);
    Ok(())
}
