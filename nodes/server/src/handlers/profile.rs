use {
    super::{
        CallId, Codec, Handler, HandlerResult, ProtocolResult, RequestOrigin, Scope, SyncHandler,
    },
    chat_spec::{
        advance_nonce, CreateAccountError, CreateProfile, FetchFullProfile, FetchProfile,
        FetchProfileError, FetchVault, FetchVaultError, Identity, Profile, Protocol, ReadMail,
        ReadMailError, SendMailError, SetVault, SetVaultError,
    },
    component_utils::Reminder,
    std::collections::hash_map::Entry,
};

const MAIL_BOX_CAP: usize = 1024 * 1024;

impl SyncHandler for FetchProfile {
    fn execute<'a>(cx: Scope<'a>, request: Self::Request<'_>) -> ProtocolResult<'a, Self> {
        cx.storage
            .profiles
            .get(&request)
            .map(std::convert::Into::into)
            .ok_or(FetchProfileError::NotFound)
    }
}

impl SyncHandler for FetchFullProfile {
    fn execute<'a>(sc: Scope<'a>, req: Self::Request<'_>) -> ProtocolResult<'a, Self> {
        sc.cx.storage.profiles.get(&req).map(Into::into).ok_or(FetchProfileError::NotFound)
    }
}

impl SyncHandler for CreateProfile {
    fn execute<'a>(mut cx: Scope<'a>, (proof, enc): Self::Request<'_>) -> ProtocolResult<'a, Self> {
        crate::ensure!(proof.verify(), CreateAccountError::InvalidProof);

        let user_id = crypto::hash::from_raw(&proof.pk);
        let entry = cx.storage.profiles.entry(user_id);

        match entry {
            Entry::Vacant(entry) => {
                entry.insert(Profile {
                    sign: proof.pk,
                    enc,
                    last_sig: proof.signature,
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
                account.last_sig = proof.signature;
                account.vault.clear();
                account.vault.extend(proof.context);
                Ok(())
            }
            _ => Err(CreateAccountError::AlreadyExists),
        }
    }
}

impl SyncHandler for SetVault {
    fn execute<'a>(mut cx: Scope<'a>, proof: Self::Request<'_>) -> ProtocolResult<'a, Self> {
        crate::ensure!(proof.verify(), SetVaultError::InvalidProof);

        let identity = crypto::hash::from_raw(&proof.pk);
        let profile = cx.storage.profiles.get_mut(&identity);

        crate::ensure!(let Some(profile) = profile, SetVaultError::NotFound);

        crate::ensure!(
            advance_nonce(&mut profile.vault_version, proof.nonce),
            SetVaultError::InvalidAction
        );
        profile.last_sig = proof.signature;

        profile.vault.clear();
        profile.vault.extend_from_slice(proof.context.0);

        Ok(())
    }
}

impl SyncHandler for FetchVault {
    fn execute<'a>(sc: Scope<'a>, request: Self::Request<'_>) -> ProtocolResult<'a, Self> {
        let profile = sc.cx.storage.profiles.get(&request);
        crate::ensure!(let Some(profile) = profile, FetchVaultError::NotFound);
        Ok((profile.vault_version, profile.mail_action, Reminder(profile.vault.as_slice())))
    }
}

impl SyncHandler for ReadMail {
    fn execute<'a>(sc: Scope<'a>, request: Self::Request<'_>) -> ProtocolResult<'a, Self> {
        crate::ensure!(request.verify(), ReadMailError::InvalidProof);
        let store = sc.cx.storage;
        let identity = crypto::hash::from_raw(&request.pk);
        let profile = store.profiles.get_mut(&identity);
        crate::ensure!(let Some(profile) = profile, ReadMailError::NotFound);
        crate::ensure!(
            advance_nonce(&mut profile.mail_action, request.nonce),
            ReadMailError::InvalidAction
        );
        store.online.insert(identity, sc.origin);
        Ok(Reminder(profile.read_mail()))
    }
}

pub struct SendMail {
    dm: CallId,
    for_who: Identity,
}

#[allow(clippy::unnecessary_wraps)]
impl SendMail {
    fn clear_presence(self, mut cx: Scope) -> HandlerResult<Self> {
        cx.storage.online.remove(&self.for_who);
        Ok(Ok(()))
    }

    fn pop_pushed_mail(self, mut cx: Scope) -> HandlerResult<Self> {
        if let Some(profile) = cx.storage.profiles.get_mut(&self.for_who) {
            profile.mail.clear();
        };
        Ok(Err(SendMailError::SentDirectly))
    }
}

impl Handler for SendMail {
    type Event = rpc::Event;
    type Protocol = chat_spec::SendMail;

    fn execute<'a>(
        sc: Scope<'a>,
        req @ (for_who, Reminder(mail)): <Self::Protocol as Protocol>::Request<'_>,
    ) -> HandlerResult<'a, Self> {
        crate::ensure!(let Some(profile) = sc.cx.storage.profiles.get_mut(&for_who), Ok(SendMailError::NotFound));
        crate::ensure!(
            profile.mail.len() + mail.len() < MAIL_BOX_CAP,
            Ok(SendMailError::MailboxFull)
        );

        let Entry::Occupied(online_in) = sc.cx.storage.online.entry(for_who) else {
            profile.push_mail(mail);
            return Ok(Ok(()));
        };

        match *online_in.get() {
            RequestOrigin::Client(p) => {
                crate::ensure!(
                    RequestOrigin::Client(p) != sc.origin,
                    Ok(SendMailError::SendingToSelf)
                );
                crate::ensure!(
                    !crate::push_notification(sc.cx.clients, for_who, Reminder(mail), p),
                    Ok(SendMailError::SentDirectly)
                );

                online_in.remove();
                profile.push_mail(mail);
                Ok(Ok(()))
            }
            RequestOrigin::Server(peer) => {
                profile.push_mail(mail);
                if matches!(sc.origin, RequestOrigin::Server(_)) {
                    online_in.remove();
                    return Ok(Ok(()));
                }

                let packet = (sc.prefix, req).to_bytes();
                if let Ok(dm) = sc.cx.swarm.behaviour_mut().rpc.request(peer, packet) {
                    Err(Self { dm, for_who })
                } else {
                    Ok(Ok(()))
                }
            }
        }
    }

    fn resume<'a>(self, sc: Scope<'a>, enent: &'a Self::Event) -> HandlerResult<'a, Self> {
        crate::ensure!(let rpc::Event::Response(_, call, res) = enent, self);
        crate::ensure!(*call == self.dm, self);

        let mut request = match res {
            Ok((request, ..)) => request.as_slice(),
            Err(_) => return self.clear_presence(sc),
        };

        if ProtocolResult::<'a, Self::Protocol>::decode(&mut request)
            == Some(Err(SendMailError::SentDirectly))
        {
            self.pop_pushed_mail(sc)
        } else {
            self.clear_presence(sc)
        }
    }
}
