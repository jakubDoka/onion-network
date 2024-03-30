use {
    crate::{
        fetch_profile, vault_chat_404, ChatMeta, Encrypted, FriendMeta, RawChatMessage, RawRequest,
        RequestInit, RequestStream, SubscriptionInit, SubscriptionMessage, UserKeys, Vault,
        VaultComponentId,
    },
    anyhow::Context,
    chat_spec::*,
    codec::{Codec, Reminder},
    crypto::enc::{ChoosenCiphertext, Ciphertext},
    double_ratchet::{DoubleRatchet, MessageHeader},
    libp2p::futures::{
        channel::{mpsc, oneshot},
        SinkExt, StreamExt,
    },
    onion::EncryptedStream,
    rand::rngs::OsRng,
    std::{collections::HashSet, convert::identity},
};

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) type Result<T, E = anyhow::Error> = std::result::Result<T, E>;

#[derive(Clone)]
pub struct Requests {
    buffer: Vec<u8>,
    sink: mpsc::Sender<RequestInit>,
}

impl Requests {
    pub fn new() -> (Self, RequestStream) {
        let (sink, stream) = mpsc::channel(5);
        (Self { buffer: Vec::new(), sink }, stream)
    }

    pub async fn invite_member(
        &mut self,
        name: ChatName,
        member: Identity,
        ctx: impl RequestContext,
        config: Member,
    ) -> Result<()> {
        let keys = self.fetch_keys(member).await?;

        let (invite, proof) = ctx.try_with_vault(|vault| {
            let chat_meta = vault.chats.get_mut(&name).with_context(vault_chat_404(name))?;
            let cp =
                ctx.with_keys(|k| k.enc.encapsulate_choosen(&keys.enc, chat_meta.secret, OsRng))?;
            let proof =
                ctx.with_keys(|k| Proof::new(&k.sign, &mut chat_meta.action_no, name, OsRng))?;
            Ok((MailVariants::ChatInvite { chat: name, cp }, proof))
        })?;

        self.send_mail(member, invite).await.context("sending invite")?;
        self.add_member(proof, member, config).await
    }

    pub async fn send_encrypted_message(
        &mut self,
        name: ChatName,
        mut message: Vec<u8>,
        ctx: impl RequestContext,
    ) -> Result<()> {
        let proof = ctx.try_with_vault(|vault| {
            let chat_meta = vault.chats.get_mut(&name).with_context(vault_chat_404(name))?;
            let tag = crypto::encrypt(&mut message, chat_meta.secret, OsRng);
            message.extend(tag);
            let msg = Reminder(&message);
            let proof =
                ctx.with_keys(|k| Proof::new(&k.sign, &mut chat_meta.action_no, msg, OsRng))?;
            Ok(proof)
        })?;
        self.send_message(proof, name).await
    }

    pub async fn fetch_and_decrypt_messages(
        &mut self,
        name: ChatName,
        cursor: &mut Cursor,
        ctx: impl RequestContext,
    ) -> Result<Vec<RawChatMessage>> {
        let chat_meta = ctx.try_with_vault(|v| {
            v.chats.get(&name).context("we are not part of the chat").copied()
        })?;

        let (new_cusor, Reminder(mesages)) = self.fetch_messages(name, *cursor).await?;
        *cursor = new_cusor;

        Ok(unpack_messages_ref(mesages)
            .filter_map(|message| {
                let message = chat_spec::Message::decode(&mut &message[..])?;
                let mut content = message.content.0.to_owned();
                let content = crypto::decrypt(&mut content, chat_meta.secret)?;
                RawChatMessage::decode(&mut &*content)
                    .map(|m| RawChatMessage { identity: message.sender, ..m })
            })
            .collect())
    }

    pub async fn create_and_save_chat(
        &mut self,
        name: ChatName,
        ctx: impl RequestContext,
    ) -> Result<()> {
        let my_id = ctx.with_keys(UserKeys::identity_hash)?;
        ctx.try_with_vault(|v| Ok(v.chats.insert(name, ChatMeta::new())))?;
        self.save_vault_component(VaultComponentId::Chats, &ctx).await.context("saving vault")?;
        self.create_chat(name, my_id).await
    }

    pub async fn send_friend_request(
        &mut self,
        name: UserName,
        ctx: impl RequestContext,
    ) -> Result<()> {
        let identity = fetch_profile(ctx.with_keys(|k| k.name)?, name)
            .await
            .context("fetching identity")?
            .sign;
        self.send_friend_request_to_identity(name, identity, ctx)
            .await
            .context("sending friend request")
    }

    pub async fn send_friend_request_to_identity(
        &mut self,
        name: UserName,
        identity: Identity,
        ctx: impl RequestContext,
    ) -> Result<()> {
        let keys = self.fetch_keys(identity).await.context("fetching keys")?;
        let (cp, ss) = ctx.with_keys(|k| k.enc.encapsulate(&keys.enc, OsRng))?;
        let (dr, init, id) = DoubleRatchet::sender(ss, OsRng);

        let request = FriendRequest {
            username: ctx.with_keys(|k| k.name)?,
            identity: ctx.with_keys(UserKeys::identity_hash)?,
            init,
        };
        let mail = MailVariants::FriendRequest { cp, payload: Encrypted::new(request, ss) };
        self.send_mail(identity, mail).await.context("sending friend request")?;

        let friend = FriendMeta { dr, identity, id };
        ctx.with_vault(|v| v.friend_index.insert(friend.dr.receiver_hash(), name))?;
        ctx.with_vault(|v| v.friends.insert(name, friend))?;
        self.save_vault_component(VaultComponentId::Friend(name), &ctx).await?;
        self.save_vault_component(VaultComponentId::FriendNames, &ctx).await?;

        Ok(())
    }

    // pub async fn send_friend_chat_invite(
    //     &mut self,
    //     name: UserName,
    //     chat: ChatName,
    //     ctx: impl RequestContext,
    // ) -> Result<()> {
    //     let (members, identity, header, ss) = ctx.try_with_vault(|v| {
    //         let friend = v.friends.get_mut(&name).context("friend on found")?;
    //         let (header, ss) = friend.dr.send();
    //         let chat = v.friend_chats.get(&chat).with_context(vault_chat_404(chat))?;
    //         Ok((chat.members.clone(), friend.identity, header, ss))
    //     })?;

    //     let invite = FriendMessage::Invite { chat, members };
    //     let mail = MailVariants::FriendMessage { header, content: Encrypted::new(invite, ss) };
    //     self.send_mail(identity, mail).await.context("sending invite")?;

    //     Ok(())
    // }

    pub async fn send_frined_message(
        &mut self,
        name: UserName,
        content: String,
        ctx: impl RequestContext,
    ) -> Result<()> {
        let (session, (header, ss), identity) = ctx.try_with_vault(|v| {
            let friend = v.friends.get_mut(&name).context("friend not found")?;
            Ok((friend.dr.sender_hash(), friend.dr.send(), friend.identity))
        })?;

        let message = FriendMessage::DirectMessage { content };
        let mail =
            MailVariants::FriendMessage { header, session, content: Encrypted::new(message, ss) };
        self.send_mail(identity, mail).await.context("sending message")
    }

    pub async fn fetch_vault_key(
        &mut self,
        identity: Identity,
        key: crypto::Hash,
    ) -> Result<Reminder> {
        self.dispatch(rpcs::FETCH_VAULT_KEY, Topic::Profile(identity), key).await
    }

    pub async fn update_member(
        &mut self,
        name: ChatName,
        member: Identity,
        config: Member,
        ctx: impl RequestContext,
    ) -> Result<()> {
        let proof = ctx.try_with_vault(|v| {
            let chat_meta = v.chats.get_mut(&name).with_context(vault_chat_404(name))?;
            let action_no = &mut chat_meta.action_no;
            let proof = ctx.with_keys(|k| Proof::new(&k.sign, action_no, name, OsRng))?;
            Ok(proof)
        })?;
        self.add_member(proof, member, config).await
    }

    pub async fn add_member(
        &mut self,
        proof: Proof<ChatName>,
        member: Identity,
        config: Member,
    ) -> Result<()> {
        let msg = (&proof, member, config);
        self.dispatch(rpcs::ADD_MEMBER, Topic::Chat(proof.context), msg).await
    }

    pub async fn kick_member(
        &mut self,
        from: ChatName,
        identity: Identity,
        ctx: impl RequestContext,
    ) -> Result<()> {
        let proof = ctx.try_with_vault(|v| {
            let chat_meta = v.chats.get_mut(&from).with_context(vault_chat_404(from))?;
            let action_no = &mut chat_meta.action_no;
            let proof = ctx.with_keys(|k| Proof::new(&k.sign, action_no, from, OsRng))?;
            Ok(proof)
        })?;

        self.dispatch(rpcs::KICK_MEMBER, Topic::Chat(proof.context), (proof, identity)).await
    }

    pub async fn fetch_messages(
        &mut self,
        name: ChatName,
        cursor: Cursor,
    ) -> Result<(Cursor, Reminder)> {
        self.dispatch(rpcs::FETCH_MESSAGES, Topic::Chat(name), cursor).await
    }

    pub async fn insert_to_vault(
        &mut self,
        proof: Proof<crypto::Hash>,
        key: crypto::Hash,
        data: Vec<u8>,
    ) -> Result<()> {
        self.dispatch(rpcs::INSERT_TO_VAULT, proof.topic(), (proof, key, data)).await
    }

    pub async fn remove_from_vault(
        &mut self,
        proof: Proof<crypto::Hash>,
        key: crypto::Hash,
    ) -> Result<()> {
        self.dispatch(rpcs::REMOVE_FROM_VAULT, proof.topic(), (proof, key)).await
    }

    pub async fn fetch_keys(&mut self, identity: Identity) -> Result<FetchProfileResp> {
        self.dispatch(rpcs::FETCH_PROFILE, Topic::Profile(identity), ()).await
    }

    pub async fn send_mail<'a>(
        &'a mut self,
        to: Identity,
        mail: impl Codec<'_>,
    ) -> Result<(), ChatError> {
        self.dispatch_low(rpcs::SEND_MAIL, Topic::Profile(to), mail)
            .await
            .or_else(ChatError::recover)
    }

    pub async fn read_mail(&mut self, ctx: impl RequestContext) -> Result<Reminder> {
        let proof = ctx.with_mail_action(|nonce| {
            ctx.with_keys(|k| Proof::new(&k.sign, nonce, Mail, OsRng))
        })??;
        self.dispatch(rpcs::READ_MAIL, proof.topic(), proof).await
    }

    pub async fn fetch_members(
        &mut self,
        name: ChatName,
        from: Identity,
        limit: u32,
    ) -> Result<Vec<(Identity, Member)>, ChatError> {
        self.dispatch_low(rpcs::FETCH_MEMBERS, Topic::Chat(name), (from, limit)).await
    }

    pub async fn fetch_my_member(
        &mut self,
        name: ChatName,
        me: Identity,
    ) -> Result<Member, ChatError> {
        let mebers = self.fetch_members(name, me, 1).await?;
        mebers
            .into_iter()
            .next()
            .and_then(|(id, m)| (id == me).then_some(m))
            .ok_or(ChatError::NotMember)
    }

    pub async fn subscribe(
        &mut self,
        topic: impl Into<Topic>,
    ) -> anyhow::Result<mpsc::Receiver<SubscriptionMessage>> {
        let (tx, mut rx) = mpsc::channel(0);
        let id = CallId::new();
        let topic: Topic = topic.into();
        self.sink
            .try_send(RequestInit::Subscription(SubscriptionInit { id, topic, channel: tx }))
            .map_err(|_| ChatError::ChannelClosed)?;

        let init =
            crate::timeout(rx.next(), REQUEST_TIMEOUT).await?.ok_or(ChatError::ChannelClosed)?;
        Result::<(), ChatError>::decode(&mut &init[..]).ok_or(ChatError::InvalidResponse)??;

        Ok(rx)
    }

    pub fn unsubscribe(&mut self, topic: impl Into<Topic>) {
        let topic: Topic = topic.into();
        _ = self.sink.try_send(RequestInit::EndSubscription(topic));
    }

    async fn create_chat(&mut self, name: ChatName, me: Identity) -> Result<()> {
        self.dispatch(rpcs::CREATE_CHAT, Topic::Chat(name), me).await
    }

    async fn send_message<'a>(
        &'a mut self,
        proof: Proof<Reminder<'_>>,
        name: ChatName,
    ) -> Result<()> {
        self.dispatch(rpcs::SEND_MESSAGE, Topic::Chat(name), proof).await
    }

    pub async fn save_vault_component(
        &mut self,
        id: VaultComponentId,
        ctx: &impl RequestContext,
    ) -> Result<()> {
        let key = ctx.with_keys(|k| k.vault)?;
        let Some((hash, data)) = ctx.try_with_vault(|v| Ok(v.shapshot(id, key)))? else {
            return Ok(());
        };
        let total_hash = ctx.try_with_vault(|v| Ok(v.merkle_hash()))?;
        let proof = ctx.with_keys(|k| {
            ctx.with_vault_version(|nonce| Proof::new(&k.sign, nonce, total_hash, OsRng))
        })??;
        self.insert_to_vault(proof, hash, data).await
    }

    pub(crate) async fn dispatch_direct<'a, R: Codec<'a>>(
        &'a mut self,
        stream: &mut EncryptedStream,
        prefix: u8,
        topic: impl Into<Option<Topic>>,
        request: impl Codec<'_>,
    ) -> Result<R, ChatError> {
        stream
            .write(chat_spec::Request {
                prefix,
                id: CallId::new(),
                topic: topic.into(),
                body: Reminder(&request.to_bytes()),
            })
            .ok_or(ChatError::MessageOverload)?;

        self.buffer = crate::timeout(stream.next(), REQUEST_TIMEOUT)
            .await?
            .ok_or(ChatError::ChannelClosed)?
            .map_err(|_| ChatError::ChannelClosed)?;

        <(CallId, Result<R, ChatError>)>::decode(&mut &self.buffer[..])
            .ok_or(ChatError::InvalidResponse)
            .and_then(|(_, r)| r)
    }

    async fn dispatch_low<'a, R: Codec<'a>>(
        &'a mut self,
        prefix: u8,
        topic: impl Into<Option<Topic>>,
        request: impl Codec<'_>,
    ) -> Result<R, ChatError> {
        let id = CallId::new();
        let (tx, rx) = oneshot::channel();
        self.sink
            .send(RequestInit::Request(RawRequest {
                id,
                topic: topic.into(),
                prefix,
                payload: request.to_bytes(),
                channel: tx,
            }))
            .await
            .map_err(|_| ChatError::ChannelClosed)?;
        self.buffer =
            crate::timeout(rx, REQUEST_TIMEOUT).await?.map_err(|_| ChatError::ChannelClosed)?;
        Result::<R, ChatError>::decode(&mut &self.buffer[..])
            .ok_or(ChatError::InvalidResponse)
            .and_then(identity)
    }

    async fn dispatch<'a, R: Codec<'a>>(
        &'a mut self,
        prefix: u8,
        topic: impl Into<Option<Topic>>,
        request: impl Codec<'_>,
    ) -> Result<R> {
        self.dispatch_low(prefix, topic, request).await.map_err(Into::into)
    }
}

pub trait RequestContext {
    fn try_with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> Result<R>) -> Result<R>;
    fn with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> R) -> Result<R>;
    fn with_keys<R>(&self, action: impl FnOnce(&UserKeys) -> R) -> Result<R>;
    fn with_vault_version<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> Result<R>;
    fn with_mail_action<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> Result<R>;
}

impl<T: RequestContext> RequestContext for &T {
    fn try_with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> Result<R>) -> Result<R> {
        (*self).try_with_vault(action)
    }

    fn with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> R) -> Result<R> {
        (*self).with_vault(action)
    }

    fn with_keys<R>(&self, action: impl FnOnce(&UserKeys) -> R) -> Result<R> {
        (*self).with_keys(action)
    }

    fn with_vault_version<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> Result<R> {
        (*self).with_vault_version(action)
    }

    fn with_mail_action<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> Result<R> {
        (*self).with_mail_action(action)
    }
}

#[derive(Codec)]
pub struct JoinRequestPayload {
    pub name: UserName,
    pub chat: ChatName,
    pub identity: Identity,
    #[codec(with = codec::unsafe_as_raw_bytes)]
    pub init: double_ratchet::InitiatiorKey,
}

#[derive(Codec)]
pub enum FriendMessage {
    DirectMessage { content: String },
    Message { chat: ChatName, content: String },
    Invite { chat: ChatName, members: HashSet<UserName> },
    Add { chat: ChatName, member: UserName },
}

#[derive(Codec)]
pub struct FriendRequest {
    pub username: UserName,
    pub identity: Identity,
    #[codec(with = codec::unsafe_as_raw_bytes)]
    pub init: double_ratchet::InitiatiorKey,
}

#[derive(Codec)]
#[allow(clippy::large_enum_variant)]
pub enum MailVariants {
    ChatInvite {
        chat: ChatName,
        cp: ChoosenCiphertext,
    },
    FriendMessage {
        header: MessageHeader,
        session: crypto::Hash,
        content: Encrypted<FriendMessage>,
    },
    FriendRequest {
        cp: Ciphertext,
        payload: Encrypted<FriendRequest>,
    },
}

impl MailVariants {
    pub async fn handle(
        self,
        ctx: &impl RequestContext,
        _requests: Requests,
        updates: &mut Vec<VaultComponentId>,
        messages: &mut Vec<(UserName, FriendMessage)>,
    ) -> Result<()> {
        match self {
            MailVariants::ChatInvite { chat, cp } => {
                let secret = ctx
                    .with_keys(|k| k.enc.decapsulate_choosen(&cp))?
                    .context("failed to decapsulate invite")?;
                ctx.with_vault(|v| v.chats.insert(chat, ChatMeta::from_secret(secret)))?;
                updates.push(VaultComponentId::Chats);
            }
            MailVariants::FriendRequest { cp, mut payload } => {
                let secret = ctx
                    .with_keys(|k| k.enc.decapsulate(&cp))?
                    .context("fialed to decapsulate friend request")?;

                let FriendRequest { username, identity, init } =
                    payload.decrypt(secret).context("failed to decrypt frined request")?;

                let (dr, id) = DoubleRatchet::recipient(secret, init, OsRng);
                updates.push(VaultComponentId::Friend(username));
                updates.push(VaultComponentId::FriendNames);

                let friend = FriendMeta { dr, identity, id };
                ctx.with_vault(|v| v.friend_index.insert(friend.dr.receiver_hash(), username))?;
                ctx.with_vault(|v| v.friends.insert(username, friend))?;
            }
            MailVariants::FriendMessage { header, session, mut content } => {
                let message = ctx.try_with_vault(|v| {
                    let nm = *v.friend_index.get(&session).context("friend not found")?;
                    let friend = v.friends.get_mut(&nm).context("friend not found, wath?")?;

                    let mut temp_dr = friend.dr.clone();
                    let key =
                        temp_dr.recv(header, OsRng).context("failed to determine message key")?;
                    let message = content.decrypt(key).context("failed to decrypt message")?;

                    friend.dr = temp_dr;
                    v.friend_index.remove(&session);
                    v.friend_index.insert(friend.dr.receiver_hash(), nm);

                    Ok((nm, message))
                })?;

                messages.push(message);
            }
        }

        Ok(())
    }
}
