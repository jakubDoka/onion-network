use {
    crate::{
        ChainClientExt, ChatMeta, DowncastNonce, FriendMeta, NodeHandle, RawChatMessage,
        RecoverMail, Subscription, Theme, UserKeys, Vault, VaultComponentId,
    },
    anyhow::Context,
    chain_api::Encrypted,
    chat_spec::*,
    codec::{Codec, Decode, Encode, Reminder, ReminderOwned},
    crypto::{
        enc::{ChoosenCiphertext, Ciphertext},
        proof::{Nonce, Proof},
    },
    double_ratchet::{DoubleRatchet, MessageHeader},
    rand::rngs::OsRng,
    std::{collections::HashSet, future::Future},
};

impl NodeHandle {}

impl Subscription {
    pub async fn send_encrypted_message(
        &mut self,
        name: ChatName,
        mut message: Vec<u8>,
        ctx: impl RequestContext,
    ) -> anyhow::Result<()> {
        let proof = ctx.try_with_vault(|vault| {
            let chat_meta =
                vault.chats.get_mut(&name).with_context(|| format!("could not find '{name}'"))?;
            let tag = crypto::encrypt(&mut message, chat_meta.secret, OsRng);
            message.extend(tag);
            let msg = Reminder(&message);
            let proof =
                ctx.with_keys(|k| Proof::new(&k.sign, &mut chat_meta.action_no, msg, OsRng))?;
            Ok(proof)
        })?;
        let nonce = match self.send_message(proof, name).await.downcast_nonce()? {
            Err(nonce) => nonce,
            Ok(r) => return Ok(r),
        };
        log::info!("foo bar {nonce} {}", proof.nonce);
        let proof = ctx.with_keys(|k| Proof::new(&k.sign, &mut { nonce }, proof.context, OsRng))?;
        ctx.try_with_vault(|v| {
            v.chats.get_mut(&name).with_context(|| format!("could not find '{name}'"))?.action_no =
                nonce + 1;
            Ok(())
        })?;
        self.send_message(proof, name).await.map_err(Into::into)
    }

    pub async fn fetch_and_decrypt_messages(
        &mut self,
        name: ChatName,
        cursor: &mut Cursor,
        ctx: impl RequestContext,
    ) -> anyhow::Result<Vec<RawChatMessage>> {
        let chat_meta = ctx.try_with_vault(|v| {
            v.chats.get(&name).context("we are not part of the chat").copied()
        })?;

        let (new_cusor, ReminderOwned(mesages)) = self.fetch_messages(name, *cursor).await?;
        *cursor = new_cusor;

        Ok(unpack_messages_ref(&mesages)
            .filter_map(|message| {
                let message = chat_spec::Message::decode(&mut &message[..])?;
                let mut content = message.content.0.to_owned();
                let content = crypto::decrypt(&mut content, chat_meta.secret)?;
                RawChatMessage::decode(&mut &*content)
                    .map(|m| RawChatMessage { identity: message.sender, ..m })
            })
            .collect())
    }

    pub async fn fetch_vault_key(
        &mut self,
        identity: Identity,
        key: crypto::Hash,
    ) -> anyhow::Result<ReminderOwned> {
        self.request(rpcs::FETCH_VAULT_KEY, Topic::Profile(identity), key).await.map_err(Into::into)
    }

    pub async fn update_member(
        &mut self,
        name: ChatName,
        member: Identity,
        config: Member,
        ctx: impl RequestContext,
    ) -> anyhow::Result<()> {
        let proof = ctx.try_with_vault(|v| {
            let chat_meta =
                v.chats.get_mut(&name).with_context(|| format!("could not find '{name}'"))?;
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
    ) -> anyhow::Result<()> {
        let msg = (&proof, member, config);
        self.request(rpcs::ADD_MEMBER, Topic::Chat(proof.context), msg).await.map_err(Into::into)
    }

    pub async fn kick_member(
        &mut self,
        from: ChatName,
        identity: Identity,
        ctx: impl RequestContext,
    ) -> anyhow::Result<()> {
        let proof = ctx.try_with_vault(|v| {
            let chat_meta =
                v.chats.get_mut(&from).with_context(|| format!("could not find '{from}'"))?;
            let action_no = &mut chat_meta.action_no;
            let proof = ctx.with_keys(|k| Proof::new(&k.sign, action_no, from, OsRng))?;
            Ok(proof)
        })?;

        self.request(rpcs::KICK_MEMBER, Topic::Chat(proof.context), (proof, identity))
            .await
            .map_err(Into::into)
    }

    pub async fn fetch_messages(
        &mut self,
        name: ChatName,
        cursor: Cursor,
    ) -> anyhow::Result<(Cursor, ReminderOwned)> {
        self.request(rpcs::FETCH_MESSAGES, Topic::Chat(name), cursor).await.map_err(Into::into)
    }

    pub async fn insert_to_vault(
        &mut self,
        proof: Proof<crypto::Hash>,
        changes: Vec<(crypto::Hash, Vec<u8>)>,
    ) -> anyhow::Result<()> {
        self.request(rpcs::INSERT_TO_VAULT, proof.topic(), (proof, changes))
            .await
            .map_err(Into::into)
    }

    pub async fn remove_from_vault(
        &mut self,
        proof: Proof<crypto::Hash>,
        key: crypto::Hash,
    ) -> anyhow::Result<()> {
        self.request(rpcs::REMOVE_FROM_VAULT, proof.topic(), (proof, key)).await.map_err(Into::into)
    }

    pub async fn fetch_keys(&mut self, identity: Identity) -> anyhow::Result<FetchProfileResp> {
        self.request(rpcs::FETCH_PROFILE, Topic::Profile(identity), ()).await.map_err(Into::into)
    }

    pub async fn send_mail(&mut self, to: Identity, mail: impl Encode) -> anyhow::Result<()> {
        log::info!("sending mail to {:?}", mail.to_bytes());
        self.request(rpcs::SEND_MAIL, Topic::Profile(to), mail).await.recover_mail()
    }

    pub async fn read_mail(&mut self, ctx: impl RequestContext) -> anyhow::Result<ReminderOwned> {
        let proof = ctx.with_mail_action(|nonce| {
            ctx.with_keys(|k| Proof::new(&k.sign, nonce, Mail, OsRng))
        })??;
        self.request(rpcs::READ_MAIL, proof.topic(), proof).await
    }

    pub async fn fetch_members(
        &mut self,
        name: ChatName,
        from: Identity,
        limit: u32,
    ) -> anyhow::Result<Vec<(Identity, Member)>> {
        self.request(rpcs::FETCH_MEMBERS, Topic::Chat(name), (from, limit)).await
    }

    pub async fn fetch_my_member(
        &mut self,
        name: ChatName,
        me: Identity,
    ) -> anyhow::Result<Member> {
        let mebers = self.fetch_members(name, me, 1).await?;
        mebers
            .into_iter()
            .next()
            .and_then(|(id, m)| (id == me).then_some(m))
            .context("member not found")
    }

    async fn create_chat(&mut self, name: ChatName, me: Identity) -> anyhow::Result<()> {
        self.request(rpcs::CREATE_CHAT, name, me).await
    }

    async fn send_message<'a>(
        &'a mut self,
        proof: Proof<Reminder<'_>>,
        name: ChatName,
    ) -> anyhow::Result<()> {
        self.request(rpcs::SEND_MESSAGE, name, proof).await
    }

    pub async fn save_vault_components(
        &mut self,
        id: impl IntoIterator<Item = VaultComponentId>,
        ctx: &impl RequestContext,
    ) -> anyhow::Result<()> {
        let key = ctx.with_keys(|k| k.vault)?;
        let chnages = ctx.with_vault(|v| {
            id.into_iter().filter_map(|id| v.shapshot(id, key)).collect::<Vec<_>>()
        })?;
        if chnages.is_empty() {
            return Ok(());
        }
        let total_hash = ctx.try_with_vault(|v| Ok(v.merkle_hash()))?;
        let proof = ctx.with_keys(|k| {
            ctx.with_vault_version(|nonce| Proof::new(&k.sign, nonce, total_hash, OsRng))
        })??;
        self.insert_to_vault(proof, chnages).await
    }

    // pub async fn request_storage_stream(
    //     &mut self,
    //     identity: NodeIdentity,
    //     addr: SocketAddr,
    //     prefix: u8,
    //     request: impl Codec<'_>,
    // ) -> Result<libp2p::Stream, ChatError> {
    //     let (stream, stream_recv) = oneshot::channel();
    //     self.sink
    //         .send(RequestInit::StorageRequest(RawStorageRequest {
    //             prefix,
    //             payload: request.to_bytes(),
    //             response: Ok(stream),
    //             identity,
    //             addr,
    //         }))
    //         .await
    //         .map_err(|_| ChatError::ChannelClosed)?;

    //     let stream = crate::chat_timeout(stream_recv, REQUEST_TIMEOUT)
    //         .await?
    //         .map_err(|_| ChatError::ChannelClosed)?;

    //     Ok(stream)
    // }

    // pub async fn upload_file(
    //     &mut self,
    //     satelite: NodeIdentity,
    //     size: u64,
    //     blob: impl futures::Stream<Item = Vec<u8>>,
    //     context: &impl RequestContext,
    // ) -> anyhow::Result<()> {
    //     let mut blob = std::pin::pin!(blob);
    //     let Some(mut chunk) = blob.next().await else {
    //         anyhow::bail!("empty file");
    //     };

    //     let (file, proofs) = self.allocate_file(satelite, size, &context).await?;

    //     let tasks = file.holders.into_iter().zip(proofs).map(|((id, addr), proof)| {
    //         let mut s = self.clone();
    //         async move { s.file_store_stream(id, addr, proof).await }
    //     });

    //     let mut streams = futures::future::join_all(tasks)
    //         .await
    //         .into_iter()
    //         .filter_map(Result::ok)
    //         .collect::<Vec<_>>();

    //     if streams.len() < DATA_PIECES {
    //         return Err(ClientError::NotEnoughtNodes.into());
    //     }

    //     const BUFFER_SIZE: usize = DATA_PIECES * PIECE_SIZE;

    //     let mut data_buffer = Vec::with_capacity(BUFFER_SIZE);
    //     let mut parity_buffer = [0u8; PARITY_PIECES * PIECE_SIZE];
    //     let encoding = storage_spec::Encoding::default();
    //     let mut last_iter = false;

    //     // FIXME: the user can purposefully not send file to some nodes so the nodes need to
    //     // confirm with the satelite after download is complete
    //     loop {
    //         let to_drain = chunk.len().min(data_buffer.capacity() - data_buffer.len());
    //         data_buffer.extend(chunk.drain(..to_drain));

    //         if last_iter && !data_buffer.is_empty() {
    //             data_buffer.resize(data_buffer.capacity(), 0);
    //         }

    //         if let Ok(data) = <&[u8; BUFFER_SIZE]>::try_from(data_buffer.as_slice()) {
    //             encoding.encode(data, &mut parity_buffer);
    //             let chunks = data_buffer.chunks(PIECE_SIZE).chain(parity_buffer.chunks(PIECE_SIZE));
    //             let tasks = streams.iter_mut().zip(chunks).map(|(s, chunk)| s.write_all(chunk));
    //             let results = futures::future::join_all(tasks).await;
    //             if results.into_iter().filter_map(Result::ok).count() < DATA_PIECES {
    //                 return Err(ClientError::NotEnoughtNodes.into());
    //             }
    //             data_buffer.clear();
    //         }

    //         if last_iter {
    //             break;
    //         }

    //         if !chunk.is_empty() {
    //             continue;
    //         }

    //         let Some(next) = blob.next().await else {
    //             last_iter = true;
    //             continue;
    //         };
    //         chunk = next;
    //     }

    //     Ok(())
    // }

    // async fn file_store_stream(
    //     &mut self,
    //     identity: NodeIdentity,
    //     addr: SocketAddr,
    //     proof: Proof<StoreContext>,
    // ) -> Result<libp2p::Stream, ChatError> {
    //     self.request_storage_stream(identity, addr, srpcs::STORE_FILE, proof).await
    // }

    // async fn allocate_file(
    //     &mut self,
    //     satelite: NodeIdentity,
    //     size: u64,
    //     context: impl RequestContext,
    // ) -> Result<(File, [Proof<StoreContext>; MAX_PIECES]), ClientError> {
    //     // FIXME: get nonce more efficiently and cache it
    //     let proof = context.with_keys(|k| Proof::new(&k.sign, &mut 0, satelite, OsRng))?;
    //     let nonce = match self
    //         .request_satelite_request(satelite, srpcs::ALLOCATE_FILE, (size, proof))
    //         .await
    //     {
    //         Err(ClientError::InvalidNonce(nonce)) => nonce,
    //         e => return e,
    //     };

    //     let proof = context.with_keys(|k| Proof::new(&k.sign, &mut { nonce }, satelite, OsRng))?;
    //     self.request_satelite_request(satelite, srpcs::ALLOCATE_FILE, (size, proof)).await
    // }

    // pub async fn request_storage_request<'a, R: Codec<'a>>(
    //     &'a mut self,
    //     identity: NodeIdentity,
    //     addr: SocketAddr,
    //     prefix: u8,
    //     request: impl Codec<'_>,
    // ) -> Result<R, ChatError> {
    //     let (tx, rx) = oneshot::channel();
    //     self.sink
    //         .send(RequestInit::StorageRequest(RawStorageRequest {
    //             prefix,
    //             payload: request.to_bytes(),
    //             response: Err(tx),
    //             identity,
    //             addr,
    //         }))
    //         .await
    //         .map_err(|_| ChatError::ChannelClosed)?;

    //     self.buffer = crate::chat_timeout(rx, REQUEST_TIMEOUT)
    //         .await?
    //         .map_err(|_| ChatError::ChannelClosed)?;
    //     Result::<R, ChatError>::decode(&mut &self.buffer[..])
    //         .ok_or(ChatError::InvalidResponse)
    //         .and_then(std::convert::identity)
    // }

    // pub async fn request_satelite_request<'a, R: Codec<'a>>(
    //     &'a mut self,
    //     identity: NodeIdentity,
    //     prefix: u8,
    //     request: impl Codec<'_>,
    // ) -> Result<R, ClientError> {
    //     let (tx, rx) = oneshot::channel();
    //     self.sink
    //         .send(RequestInit::SateliteRequest(RawSateliteRequest {
    //             prefix,
    //             payload: request.to_bytes(),
    //             response: tx,
    //             identity,
    //         }))
    //         .await
    //         .map_err(|_| ClientError::ChannelClosed)?;

    //     self.buffer = crate::timeout(rx, REQUEST_TIMEOUT)
    //         .await
    //         .ok_or(ClientError::Timeout)?
    //         .map_err(|_| ClientError::ChannelClosed)?;
    //     Result::<R, _>::decode(&mut &self.buffer[..])
    //         .ok_or(ClientError::InvalidResponse)
    //         .and_then(std::convert::identity)
    // }
}

#[allow(async_fn_in_trait)]
pub trait RequestContext: Sized {
    fn try_with_vault<R>(
        &self,
        action: impl FnOnce(&mut Vault) -> anyhow::Result<R>,
    ) -> anyhow::Result<R>;
    fn with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> R) -> anyhow::Result<R>;
    fn with_keys<R>(&self, action: impl FnOnce(&UserKeys) -> R) -> anyhow::Result<R>;
    fn with_vault_version<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> anyhow::Result<R>;
    fn with_mail_action<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> anyhow::Result<R>;
    fn subscription_for(
        &self,
        topic: impl Into<Topic>,
    ) -> impl Future<Output = anyhow::Result<Subscription>>;

    async fn set_theme(&self, theme: Theme) -> anyhow::Result<()> {
        self.with_vault(|v| v.theme = theme)?;
        self.subscription_for(self.with_keys(UserKeys::identity)?)
            .await?
            .save_vault_components([VaultComponentId::Theme], self)
            .await
    }

    async fn send_frined_message(&self, name: UserName, content: String) -> anyhow::Result<()> {
        let (session, (header, ss), identity) = self.try_with_vault(|v| {
            let friend = v.friends.get_mut(&name).context("friend not found")?;
            Ok((friend.dr.sender_hash(), friend.dr.send(), friend.identity))
        })?;

        self.subscription_for(self.with_keys(UserKeys::identity)?)
            .await?
            .save_vault_components([VaultComponentId::Friend(name)], self)
            .await?;

        let message = FriendMessage::DirectMessage { content };
        let mail =
            MailVariants::FriendMessage { header, session, content: Encrypted::new(message, ss) };
        self.subscription_for(identity)
            .await?
            .send_mail(identity, mail)
            .await
            .context("sending message")
    }

    async fn create_and_save_chat(&self, name: ChatName) -> anyhow::Result<()> {
        let my_id = self.with_keys(UserKeys::identity)?;
        self.try_with_vault(|v| Ok(v.chats.insert(name, ChatMeta::new())))?;
        self.subscription_for(my_id)
            .await?
            .save_vault_components([VaultComponentId::Chats], &self)
            .await
            .context("saving vault")?;
        self.subscription_for(name).await?.create_chat(name, my_id).await
    }

    async fn invite_member(
        &self,
        name: ChatName,
        member: UserName,
        config: Member,
    ) -> anyhow::Result<()> {
        let identity = self
            .with_keys(UserKeys::chain_client)?
            .await?
            .fetch_profile(member)
            .await
            .context("fetching identity")?
            .sign;
        self.invite_member_identity(name, identity, config).await.context("inviting member")
    }

    async fn invite_member_identity(
        &self,
        name: ChatName,
        member: Identity,
        config: Member,
    ) -> anyhow::Result<()> {
        let mut them = self.subscription_for(member).await?;
        let keys = them.fetch_keys(member).await?;

        let (invite, proof) = self.try_with_vault(|vault| {
            let chat_meta =
                vault.chats.get_mut(&name).with_context(|| format!("could not find '{name}'"))?;
            let cp =
                self.with_keys(|k| k.enc.encapsulate_choosen(&keys.enc, chat_meta.secret, OsRng))?;
            let proof =
                self.with_keys(|k| Proof::new(&k.sign, &mut chat_meta.action_no, name, OsRng))?;
            Ok((MailVariants::ChatInvite { chat: name, cp }, proof))
        })?;

        them.send_mail(member, invite).await.context("sending invite")?;
        self.subscription_for(name).await?.add_member(proof, member, config).await
    }

    async fn send_friend_request(&self, name: UserName) -> anyhow::Result<()> {
        let identity = self
            .with_keys(UserKeys::chain_client)?
            .await?
            .fetch_profile(name)
            .await
            .context("fetching identity")?
            .sign;
        self.send_friend_request_to_identity(name, identity).await.context("sending friend request")
    }

    async fn send_friend_request_to_identity(
        &self,
        name: UserName,
        identity: Identity,
    ) -> anyhow::Result<()> {
        let mut them = self.subscription_for(identity).await?;
        let keys = them.fetch_keys(identity).await.context("fetching keys")?;
        let (cp, ss) = self.with_keys(|k| k.enc.encapsulate(&keys.enc, OsRng))?;
        let (dr, init, id) = DoubleRatchet::sender(ss, OsRng);

        let us = self.with_keys(UserKeys::identity)?;

        let request = FriendRequest { username: self.with_keys(|k| k.name)?, identity: us, init };
        let mail = MailVariants::FriendRequest { cp, payload: Encrypted::new(request, ss) };
        them.send_mail(identity, mail).await.context("sending friend request")?;

        let friend = FriendMeta { dr, identity, id };
        self.with_vault(|v| v.friend_index.insert(friend.dr.receiver_hash(), name))?;
        self.with_vault(|v| v.friends.insert(name, friend))?;
        let changes = [VaultComponentId::Friend(name), VaultComponentId::FriendNames];
        self.subscription_for(us).await?.save_vault_components(changes, self).await?;

        Ok(())
    }
}

impl<T: RequestContext> RequestContext for &T {
    fn try_with_vault<R>(
        &self,
        action: impl FnOnce(&mut Vault) -> anyhow::Result<R>,
    ) -> anyhow::Result<R> {
        (*self).try_with_vault(action)
    }

    fn with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> R) -> anyhow::Result<R> {
        (*self).with_vault(action)
    }

    fn with_keys<R>(&self, action: impl FnOnce(&UserKeys) -> R) -> anyhow::Result<R> {
        (*self).with_keys(action)
    }

    fn with_vault_version<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> anyhow::Result<R> {
        (*self).with_vault_version(action)
    }

    fn with_mail_action<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> anyhow::Result<R> {
        (*self).with_mail_action(action)
    }

    fn subscription_for(
        &self,
        topic: impl Into<Topic>,
    ) -> impl Future<Output = anyhow::Result<Subscription>> {
        (*self).subscription_for(topic)
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
        updates: &mut Vec<VaultComponentId>,
        messages: &mut Vec<(UserName, FriendMessage)>,
    ) -> anyhow::Result<()> {
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
                let message @ (name, _) = ctx.try_with_vault(|v| {
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

                updates.push(VaultComponentId::Friend(name));
                messages.push(message);
            }
        }

        Ok(())
    }
}
