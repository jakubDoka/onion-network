use {
    crate::OnlineLocation,
    chat_spec::{
        advance_nonce, rpcs, CreateProfileError, FetchProfileError, FetchProfileResp, Identity,
        Profile, Proof, ReadMailError, SendMailError, SetVaultError,
    },
    component_utils::{Codec, Reminder, ReminderOwned},
    dashmap::mapref::entry::Entry,
};

const MAIL_BOX_CAP: usize = 1024 * 1024;

pub async fn create(
    cx: super::Context,
    (proof, enc): (Proof<&[u8]>, crypto::Serialized<crypto::enc::PublicKey>),
) -> Result<(), CreateProfileError> {
    crate::ensure!(proof.verify(), CreateProfileError::InvalidProof);

    let user_id = crypto::hash::from_raw(&proof.pk);
    let entry = cx.profiles.entry(user_id);

    match entry {
        Entry::Vacant(entry) => {
            entry.insert(Profile {
                sign: proof.pk,
                enc,
                vault_sig: proof.signature,
                vault_version: proof.nonce,
                mail_action: proof.nonce,
                vault: proof.context.to_vec(),
                mail: Vec::new(),
            });
            Ok(())
        }
        Entry::Occupied(mut entry) if entry.get().vault_version < proof.nonce => {
            let account = entry.get_mut();
            account.vault_version = proof.nonce;
            account.vault_sig = proof.signature;
            account.vault.clear();
            account.vault.extend(proof.context);
            Ok(())
        }
        _ => Err(CreateProfileError::AlreadyExists),
    }
}

pub async fn set_vault(
    cx: super::Context,
    proof: Proof<Reminder<'_>>,
) -> Result<(), SetVaultError> {
    crate::ensure!(proof.verify(), SetVaultError::InvalidProof);

    let identity = crypto::hash::from_raw(&proof.pk);
    let profile = cx.profiles.get_mut(&identity);

    crate::ensure!(let Some(mut profile) = profile, SetVaultError::NotFound);

    crate::ensure!(
        advance_nonce(&mut profile.vault_version, proof.nonce),
        SetVaultError::InvalidAction
    );
    profile.vault_sig = proof.signature;

    profile.vault.clear();
    profile.vault.extend_from_slice(proof.context.0);

    Ok(())
}

pub async fn read_mail(
    cx: super::Context,
    proof: Proof<Identity>,
) -> Result<ReminderOwned, ReadMailError> {
    crate::ensure!(proof.verify(), ReadMailError::InvalidProof);

    let identity = crypto::hash::from_raw(&proof.pk);
    let profile = cx.profiles.get_mut(&identity);

    crate::ensure!(let Some(mut profile) = profile, ReadMailError::NotFound);
    crate::ensure!(
        advance_nonce(&mut profile.mail_action, proof.nonce),
        ReadMailError::InvalidAction
    );

    Ok(ReminderOwned(profile.read_mail().to_vec()))
}

pub async fn fetch_keys(
    cx: super::Context,
    identity: Identity,
) -> Result<FetchProfileResp, FetchProfileError> {
    cx.profiles
        .get(&identity)
        .ok_or(FetchProfileError::NotFound)
        .map(|p| FetchProfileResp::from(p.value()))
}

pub async fn fetch_vault(
    cx: super::Context,
    identity: Identity,
) -> Result<ReminderOwned, FetchProfileError> {
    cx.profiles
        .get(&identity)
        .ok_or(FetchProfileError::NotFound)
        .map(|p| p.value().vault.clone())
        .map(ReminderOwned)
}

pub async fn fetch_full(
    cx: super::Context,
    identity: Identity,
) -> Result<ReminderOwned, FetchProfileError> {
    cx.profiles
        .get(&identity)
        .ok_or(FetchProfileError::NotFound)
        .map(|p| p.value().to_bytes())
        .map(ReminderOwned)
}

pub async fn send_mail(
    cx: super::Context,
    origin: OnlineLocation,
    (for_who, Reminder(mail)): (Identity, Reminder<'_>),
) -> Result<(), SendMailError> {
    let push_mail = || {
        let mut profile = cx.profiles.get_mut(&for_who).ok_or(SendMailError::NotFound)?;
        crate::ensure!(profile.mail.len() + mail.len() < MAIL_BOX_CAP, SendMailError::MailboxFull);
        profile.push_mail(mail);
        Ok(())
    };

    let Some(online_in) = cx.online.get(&for_who).map(|v| *v.value()) else {
        return push_mail();
    };

    match online_in {
        OnlineLocation::Local(p) => {
            crate::ensure!(OnlineLocation::Local(p) != origin, SendMailError::SendingToSelf);
            crate::ensure!(
                !cx.push_notification(for_who, Reminder(mail), p).await,
                SendMailError::SentDirectly
            );
        }
        OnlineLocation::Remote(peer) => 'b: {
            if matches!(origin, OnlineLocation::Remote(_)) {
                break 'b;
            }

            let resp = cx.send_rpc(for_who, peer, rpcs::SEND_MAIL, (for_who, Reminder(mail))).await;
            if let Some(resp) = <Result<(), SendMailError>>::decode(&mut resp.as_slice())
                && resp.is_ok()
            {
                return Ok(());
            }
        }
    }

    cx.online.remove(&for_who);
    push_mail()
}
