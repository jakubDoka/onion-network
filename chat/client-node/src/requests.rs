use {
    crate::{
        ChainClientExt, ChatMeta, FriendMeta, Node, NodeHandle, RawChatMessage, RecoverMail,
        Storage, Sub, Theme, UserKeys, Vault, VaultKey,
    },
    anyhow::Context,
    chain_api::Encrypted,
    chat_spec::*,
    codec::{Codec, Decode, Encode, ReminderOwned},
    crypto::{
        enc::{ChoosenCiphertext, Ciphertext},
        proof::{Nonce, Proof},
    },
    double_ratchet::{DoubleRatchet, MessageHeader},
    libp2p::futures::{
        future::{JoinAll, TryJoinAll},
        TryFutureExt,
    },
    onion::SharedSecret,
    rand::rngs::OsRng,
    std::{cell::RefCell, future::Future},
};

impl Sub<Identity> {
    pub async fn fetch_vault_key(&mut self, key: crypto::Hash) -> anyhow::Result<ReminderOwned> {
        self.request(rpcs::FETCH_VAULT_KEY, key).await.map_err(Into::into)
    }

    pub async fn insert_to_vault(
        &mut self,
        proof: Proof<crypto::Hash>,
        changes: Vec<(crypto::Hash, Vec<u8>)>,
    ) -> anyhow::Result<()> {
        debug_assert_eq!(proof.topic(), self.topic.into());
        self.request(rpcs::INSERT_TO_VAULT, (proof, changes)).await.map_err(Into::into)
    }

    pub async fn remove_from_vault(
        &mut self,
        proof: Proof<crypto::Hash>,
        key: crypto::Hash,
    ) -> anyhow::Result<()> {
        debug_assert_eq!(proof.topic(), self.topic.into());
        self.request(rpcs::REMOVE_FROM_VAULT, (proof, key)).await.map_err(Into::into)
    }

    pub async fn fetch_keys(&mut self) -> anyhow::Result<FetchProfileResp> {
        Storage::get_or_insert(self.topic, self.user, |k| async move {
            let f = self.sub.request::<FetchProfileResp>(rpcs::FETCH_PROFILE, k, ()).await;
            log::info!("fetching keys: {:?}", f.as_ref().map(|r| &r.enc));
            f
        })
        .await
    }

    pub async fn send_mail(&mut self, mail: impl Encode) -> anyhow::Result<()> {
        self.request(rpcs::SEND_MAIL, mail).await.recover_mail()
    }
}

impl Sub<ChatName> {
    pub async fn add_member(
        &mut self,
        proof: Proof<ChatName>,
        member: Identity,
        config: Member,
    ) -> anyhow::Result<()> {
        debug_assert_eq!(proof.context, self.topic);
        let msg = (&proof, member, config);
        self.request(rpcs::ADD_MEMBER, msg).await.map_err(Into::into)
    }

    pub async fn fetch_messages(
        &mut self,
        cursor: Cursor,
    ) -> anyhow::Result<([u8; chat_spec::MAX_MESSAGE_FETCH_SIZE], Cursor)> {
        self.request(rpcs::FETCH_MESSAGES, cursor).await.map_err(Into::into)
    }

    pub async fn fetch_members(
        &mut self,
        from: Identity,
        limit: u32,
    ) -> anyhow::Result<Vec<(Identity, Member)>> {
        self.request(rpcs::FETCH_MEMBERS, (from, limit)).await
    }

    pub async fn fetch_my_member(&mut self, me: Identity) -> anyhow::Result<Member> {
        let mebers = self.fetch_members(me, 1).await?;
        mebers
            .into_iter()
            .next()
            .and_then(|(id, m)| (id == me).then_some(m))
            .context("member not found")
    }

    async fn create_chat(&mut self, me: Identity) -> anyhow::Result<()> {
        self.request(rpcs::CREATE_CHAT, me).await
    }

    async fn send_message<'a>(&'a mut self, proof: &Proof<ReminderOwned>) -> anyhow::Result<()> {
        self.request(rpcs::SEND_MESSAGE, proof).await
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
    //     self.sub.request_storage_stream(identity, addr, srpcs::STORE_FILE, proof).await
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
    //     self.sub.request_satelite_request(satelite, srpcs::ALLOCATE_FILE, (size, proof)).await
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
    fn with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> R) -> R;
    fn with_keys<R>(&self, action: impl FnOnce(&UserKeys) -> R) -> R;
    fn with_vault_version<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> R;
    fn with_mail_action<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> R;
    fn subscription_for<T: Into<Topic> + Clone>(
        &self,
        topic: T,
    ) -> impl Future<Output = anyhow::Result<Sub<T>>>;

    fn set_chat_nonce(&self, chat: ChatName, nonce: Nonce) -> Option<()> {
        self.with_chat_and_keys_low(chat, |c, _| c.action_no = nonce)
    }

    fn chat_secret(&self, chat: ChatName) -> Option<SharedSecret> {
        self.with_chat_and_keys_low(chat, |c, _| c.secret)
    }

    fn try_with_vault<R>(
        &self,
        action: impl FnOnce(&mut Vault) -> anyhow::Result<R>,
    ) -> anyhow::Result<R> {
        self.with_vault(|v| action(v))
    }

    async fn profile_subscription(&self) -> anyhow::Result<Sub<Identity>> {
        self.subscription_for(self.with_keys(UserKeys::identity)).await
    }

    async fn set_theme(&self, theme: Theme) -> anyhow::Result<()> {
        self.with_vault(|v| v.theme = theme);
        self.save_vault_components([VaultKey::Theme]).await
    }

    async fn send_frined_message(&self, name: UserName, content: String) -> anyhow::Result<()> {
        let mut frined =
            self.try_with_vault(|v| v.friends.get_mut(&name).cloned().context("friend not found"))?;

        let session = frined.dr.sender_hash();
        let (header, ss) = frined.dr.send();
        let message = FriendMessage::DirectMessage { content };
        let content = Encrypted::new(message, ss);
        let mail = MailVariant::FriendMessage { header, session, content };
        self.subscription_for(frined.identity).await?.send_mail(mail).await?;
        self.with_vault(|v| _ = v.friends.insert(name, frined));
        self.save_vault_components([VaultKey::Friend(name)]).await?;

        Ok(())
    }

    async fn read_mail(&self) -> anyhow::Result<Vec<(UserName, FriendMessage)>> {
        let proof = self.with_mail_action(|nonce| {
            self.with_keys(|k| Proof::new(&k.sign, nonce, Mail::new(), OsRng))
        });
        let ReminderOwned(bytes) =
            self.profile_subscription().await?.request(rpcs::READ_MAIL, proof).await?;
        Ok(chat_spec::unpack_mail(&bytes)
            .filter_map(MailVariant::decode_exact)
            .map(|mv| mv.handle(self))
            .collect::<TryJoinAll<_>>()
            .await?
            .into_iter()
            .flatten()
            .collect())
    }

    async fn save_vault_components(
        &self,
        id: impl IntoIterator<Item = VaultKey>,
    ) -> anyhow::Result<()> {
        self.save_vault_components_low(id, false).await
    }

    async fn flush_vault(&self) -> anyhow::Result<()> {
        self.save_vault_components_low([], true).await
    }

    async fn save_vault_components_low(
        &self,
        id: impl IntoIterator<Item = VaultKey>,
        forced: bool,
    ) -> anyhow::Result<()> {
        let chnages = {
            let key = self.with_keys(|k| k.vault);
            self.with_vault(|v| {
                id.into_iter().for_each(|id| _ = v.update(id, key));
                v.changes(forced)
            })
        };
        if chnages.is_empty() {
            return Ok(());
        }
        let total_hash = self.with_vault(|v| v.merkle_hash());
        let proof = self.with_keys(|k| {
            self.with_vault_version(|nonce| Proof::new(&k.sign, nonce, total_hash, OsRng))
        });
        self.profile_subscription().await?.insert_to_vault(proof, chnages).await?;
        self.with_vault(|v| v.clear_changes(proof.nonce + 1));

        Ok(())
    }

    async fn create_and_save_chat(&self, name: ChatName) -> anyhow::Result<()> {
        let my_id = self.with_keys(UserKeys::identity);
        self.subscription_for(name).await?.create_chat(my_id).await?;
        self.with_vault(|v| v.chats.insert(name, ChatMeta::new()));
        self.save_vault_components([VaultKey::Chats]).await.context("saving vault")?;
        Ok(())
    }

    async fn kick_member(&self, from: ChatName, identity: Identity) -> anyhow::Result<()> {
        let proof = self
            .with_chat_and_keys(from, |c, k| Proof::new(&k.sign, &mut c.action_no, from, OsRng))?;
        let mut sub = self.subscription_for(from).await?;
        sub.request(rpcs::KICK_MEMBER, (proof, identity)).await.map_err(Into::into)
    }

    fn with_chat_and_keys_low<R>(
        &self,
        chat: ChatName,
        action: impl FnOnce(&mut ChatMeta, &UserKeys) -> R,
    ) -> Option<R> {
        self.with_vault(|v| {
            let chat_meta = v.chats.get_mut(&chat)?;
            Some(self.with_keys(|keys| action(chat_meta, &keys)))
        })
    }

    fn with_chat_and_keys<R>(
        &self,
        chat: ChatName,
        action: impl FnOnce(&mut ChatMeta, &UserKeys) -> R,
    ) -> anyhow::Result<R> {
        self.with_chat_and_keys_low(chat, action)
            .with_context(|| format!("chat '{chat}' not found"))
    }

    async fn invite_member(
        &self,
        name: ChatName,
        member: UserName,
        config: Member,
    ) -> anyhow::Result<()> {
        let identity = self
            .with_keys(UserKeys::chain_client)
            .await?
            .fetch_profile(member)
            .await
            .context("fetching identity")?
            .sign;

        let mut them = self.subscription_for(identity).await?;
        let keys = them.fetch_keys().await?;

        let (invite, proof) = self.with_chat_and_keys(name, |c, k| {
            let cp = k.enc.encapsulate_choosen(&keys.enc, c.secret, OsRng);
            let proof = Proof::new(&k.sign, &mut c.action_no, name, OsRng);
            (MailVariant::ChatInvite { chat: name, cp }, proof)
        })?;

        them.send_mail(invite).await.context("sending invite")?;
        self.subscription_for(name).await?.add_member(proof, identity, config).await
    }

    async fn send_friend_request(&self, name: UserName) -> anyhow::Result<()> {
        let identity = self
            .with_keys(UserKeys::chain_client)
            .await?
            .fetch_profile(name)
            .await
            .context("fetching identity")?
            .sign;

        let mut them = self.subscription_for(identity).await?;
        let keys = them.fetch_keys().await.context("fetching keys")?;
        let (cp, ss) = self.with_keys(|k| k.enc.encapsulate(&keys.enc, OsRng));
        let (dr, init, id) = DoubleRatchet::sender(ss, OsRng);

        let us = self.with_keys(UserKeys::identity);

        let request = FriendRequest { username: self.with_keys(|k| k.name), identity: us, init };
        let mail = MailVariant::FriendRequest { cp, payload: Encrypted::new(request, ss) };
        them.send_mail(mail).await?;

        let friend = FriendMeta { dr, identity, id };
        self.with_vault(|v| v.friend_index.insert(friend.dr.receiver_hash(), name));
        self.with_vault(|v| v.friends.insert(name, friend));
        self.save_vault_components([VaultKey::Friend(name), VaultKey::FriendNames]).await?;

        Ok(())
    }

    async fn send_message(&self, name: ChatName, message: String) -> anyhow::Result<()> {
        let mut sub = self.subscription_for(name).await?;
        let proof = self.with_chat_and_keys(name, |c, k| {
            let msg = ReminderOwned(chain_api::encrypt(message.into_bytes(), c.secret));
            Proof::new(&k.sign, &mut c.action_no, msg, OsRng)
        })?;
        sub.send_message(&proof).await
    }

    async fn fetch_messages(
        &self,
        name: ChatName,
        cursor: &mut Cursor,
    ) -> anyhow::Result<Vec<RawChatMessage>> {
        let chat_meta = self.with_chat_and_keys(name, |c, _| c.clone())?;
        let mut sub = self.subscription_for(name).await?;
        let (messages, new_cusor) = sub.fetch_messages(*cursor).await?;
        *cursor = new_cusor;

        Ok(unpack_messages_ref(&messages)
            .take_while(|m| !m.is_empty())
            .map(|message| async move {
                log::info!("message: {:?}", message);
                let chat_spec::Message { sender: identity, content, .. } =
                    chat_spec::Message::decode_exact(message).context("invalid message")?;
                let content = chain_api::decrypt(content.0.to_owned(), chat_meta.secret)
                    .context("decrypt content")?;
                let client = self.with_keys(|k| k.chain_client()).await?;
                let sender = client.fetch_username(identity).await?;
                String::from_utf8(content)
                    .map(|content| RawChatMessage { identity, sender, content })
                    .context("invalid message")
            })
            .collect::<JoinAll<_>>()
            .await
            .into_iter()
            .filter_map(|r| r.inspect_err(|e| log::warn!("decripting chat message: {:?}", e)).ok())
            .collect())
    }

    async fn update_member(
        &self,
        name: ChatName,
        member: Identity,
        config: Member,
    ) -> anyhow::Result<()> {
        let proof = self
            .with_chat_and_keys(name, |c, k| Proof::new(&k.sign, &mut c.action_no, name, OsRng))?;
        self.subscription_for(name).await?.update_member(proof, member, config).await
    }
}

impl<RC: RequestContext> RequestContext for &RC {
    fn with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> R) -> R {
        (*self).with_vault(action)
    }

    fn with_keys<R>(&self, action: impl FnOnce(&UserKeys) -> R) -> R {
        (*self).with_keys(action)
    }

    fn with_vault_version<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> R {
        (*self).with_vault_version(action)
    }

    fn with_mail_action<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> R {
        (*self).with_mail_action(action)
    }

    fn subscription_for<T: Into<Topic> + Clone>(
        &self,
        topic: T,
    ) -> impl Future<Output = anyhow::Result<Sub<T>>> {
        (*self).subscription_for(topic)
    }
}

pub struct TrivialContext {
    pub vault: RefCell<Vault>,
    pub keys: UserKeys,
    pub mail_action: RefCell<crypto::proof::Nonce>,
    pub vault_version: RefCell<crypto::proof::Nonce>,
    pub requests: NodeHandle,
}

impl TrivialContext {
    pub async fn new(keys: UserKeys) -> anyhow::Result<Self> {
        let (node, vault, requests, vault_version, mail_action) =
            Node::new(keys.clone(), |_| (), chain_api::unpack_addr).await?;

        crate::spawn(node);

        Ok(Self {
            vault: RefCell::new(vault),
            keys,
            mail_action: RefCell::new(mail_action),
            vault_version: RefCell::new(vault_version),
            requests,
        })
    }
}

impl RequestContext for TrivialContext {
    fn with_vault<R>(&self, action: impl FnOnce(&mut Vault) -> R) -> R {
        action(&mut self.vault.borrow_mut())
    }

    fn with_keys<R>(&self, action: impl FnOnce(&UserKeys) -> R) -> R {
        action(&self.keys)
    }

    fn with_vault_version<R>(&self, action: impl FnOnce(&mut crypto::proof::Nonce) -> R) -> R {
        action(&mut self.vault_version.borrow_mut())
    }

    fn with_mail_action<R>(&self, action: impl FnOnce(&mut crypto::proof::Nonce) -> R) -> R {
        action(&mut self.mail_action.borrow_mut())
    }

    fn subscription_for<T: Into<chat_spec::Topic> + Clone>(
        &self,
        topic: T,
    ) -> impl libp2p::futures::prelude::Future<Output = anyhow::Result<Sub<T>>> {
        self.requests.subscription_for(topic).map_err(Into::into)
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
pub enum MailVariant {
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

impl MailVariant {
    pub fn handle(
        self,
        ctx: &impl RequestContext,
    ) -> impl Future<Output = anyhow::Result<Option<(UserName, FriendMessage)>>> + '_ {
        let mut updates = GroupVec::new();
        let res = self.handle_sync(ctx, &mut updates);
        async {
            ctx.save_vault_components(updates).await?;
            res
        }
    }

    fn handle_sync(
        self,
        ctx: &impl RequestContext,
        updates: &mut GroupVec<VaultKey>,
    ) -> anyhow::Result<Option<(UserName, FriendMessage)>> {
        Ok(match self {
            MailVariant::ChatInvite { chat, cp } => {
                let secret = ctx
                    .with_keys(|k| k.enc.decapsulate_choosen(&cp))
                    .context("failed to decapsulate invite")?;
                ctx.with_vault(|v| v.chats.insert(chat, ChatMeta::from_secret(secret)));
                updates.push(VaultKey::Chats);
                None
            }
            MailVariant::FriendRequest { cp, mut payload } => {
                let secret = ctx
                    .with_keys(|k| k.enc.decapsulate(&cp))
                    .context("fialed to decapsulate friend request")?;

                let FriendRequest { username, identity, init } =
                    payload.decrypt(secret).context("failed to decrypt frined request")?;

                let (dr, id) = DoubleRatchet::recipient(secret, init, OsRng);
                updates.push(VaultKey::Friend(username));
                updates.push(VaultKey::FriendNames);

                let friend = FriendMeta { dr, identity, id };
                ctx.with_vault(|v| v.friend_index.insert(friend.dr.receiver_hash(), username));
                ctx.with_vault(|v| v.friends.insert(username, friend));
                None
            }
            MailVariant::FriendMessage { header, session, mut content } => {
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

                updates.push(VaultKey::Friend(name));

                Some(message)
            }
        })
    }
}
