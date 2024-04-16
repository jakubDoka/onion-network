use {
    crate::{
        chain::fetch_profile, vault_chat_404, ChatMeta, Encrypted, FriendMeta, RawChatMessage,
        RawOnionRequest, RawSateliteRequest, RawStorageRequest, RequestChannel, RequestInit,
        SubscriptionInit, SubscriptionMessage, UserKeys, Vault, VaultComponentId,
    },
    anyhow::Context,
    chat_spec::*,
    codec::{Codec, Reminder},
    component_utils::{PacketReader, PacketWriter},
    crypto::{
        enc::{ChoosenCiphertext, Ciphertext},
        proof::{Nonce, Proof},
    },
    double_ratchet::{DoubleRatchet, MessageHeader},
    libp2p::futures::{
        self,
        channel::{mpsc, oneshot},
        AsyncReadExt, AsyncWriteExt, SinkExt, StreamExt,
    },
    onion::EncryptedStream,
    rand::rngs::OsRng,
    std::{collections::HashSet, convert::identity, io, net::SocketAddr},
    storage_spec::{
        rpcs as srpcs, ClientError, File, NodeIdentity, StoreContext, DATA_PIECES, MAX_PIECES,
        PARITY_PIECES, PIECE_SIZE,
    },
};

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) type Result<T, E = anyhow::Error> = std::result::Result<T, E>;

#[derive(Clone)]
pub struct Requests {
    buffer: Vec<u8>,
    sink: mpsc::Sender<RequestInit>,
}

impl Requests {
    pub fn new() -> (Self, RequestChannel) {
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
        self.save_vault_components([VaultComponentId::Chats], &ctx)
            .await
            .context("saving vault")?;
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
        let changes = [VaultComponentId::Friend(name), VaultComponentId::FriendNames];
        self.save_vault_components(changes, &ctx).await?;

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
        changes: Vec<(crypto::Hash, Vec<u8>)>,
    ) -> Result<()> {
        self.dispatch(rpcs::INSERT_TO_VAULT, proof.topic(), (proof, changes)).await
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

        let init = crate::chat_timeout(rx.next(), REQUEST_TIMEOUT)
            .await?
            .ok_or(ChatError::ChannelClosed)?;
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

    pub async fn save_vault_components(
        &mut self,
        id: impl IntoIterator<Item = VaultComponentId>,
        ctx: &impl RequestContext,
    ) -> Result<()> {
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

        self.buffer = crate::chat_timeout(stream.next(), REQUEST_TIMEOUT)
            .await?
            .ok_or(ChatError::ChannelClosed)?
            .map_err(|_| ChatError::ChannelClosed)?;

        <(CallId, Result<R, ChatError>)>::decode(&mut &self.buffer[..])
            .ok_or(ChatError::InvalidResponse)
            .and_then(|(_, r)| r)
    }

    pub async fn dispatch_storage_stream(
        &mut self,
        identity: NodeIdentity,
        addr: SocketAddr,
        prefix: u8,
        request: impl Codec<'_>,
    ) -> Result<libp2p::Stream, ChatError> {
        let (stream, stream_recv) = oneshot::channel();
        self.sink
            .send(RequestInit::StorageRequest(RawStorageRequest {
                prefix,
                payload: request.to_bytes(),
                response: Ok(stream),
                identity,
                addr,
            }))
            .await
            .map_err(|_| ChatError::ChannelClosed)?;

        let stream = crate::chat_timeout(stream_recv, REQUEST_TIMEOUT)
            .await?
            .map_err(|_| ChatError::ChannelClosed)?;

        Ok(stream)
    }

    pub async fn upload_file(
        &mut self,
        satelite: NodeIdentity,
        size: u64,
        blob: impl futures::Stream<Item = Vec<u8>>,
        context: &impl RequestContext,
    ) -> Result<()> {
        let mut blob = std::pin::pin!(blob);
        let Some(mut chunk) = blob.next().await else {
            anyhow::bail!("empty file");
        };

        let (file, proofs) = self.allocate_file(satelite, size, &context).await?;

        let tasks = file.holders.into_iter().zip(proofs).map(|((id, addr), proof)| {
            let mut s = self.clone();
            async move { s.file_store_stream(id, addr, proof).await }
        });

        let mut streams = futures::future::join_all(tasks)
            .await
            .into_iter()
            .filter_map(Result::ok)
            .collect::<Vec<_>>();

        if streams.len() < DATA_PIECES {
            return Err(ClientError::NotEnoughtNodes.into());
        }

        const BUFFER_SIZE: usize = DATA_PIECES * PIECE_SIZE;

        let mut data_buffer = Vec::with_capacity(BUFFER_SIZE);
        let mut parity_buffer = [0u8; PARITY_PIECES * PIECE_SIZE];
        let encoding = storage_spec::Encoding::default();
        let mut last_iter = false;

        // FIXME: the user can purposefully not send file to some nodes so the nodes need to
        // confirm with the satelite after download is complete
        loop {
            let to_drain = chunk.len().min(data_buffer.capacity() - data_buffer.len());
            data_buffer.extend(chunk.drain(..to_drain));

            if last_iter && !data_buffer.is_empty() {
                data_buffer.resize(data_buffer.capacity(), 0);
            }

            if let Ok(data) = <&[u8; BUFFER_SIZE]>::try_from(data_buffer.as_slice()) {
                encoding.encode(data, &mut parity_buffer);
                let chunks = data_buffer.chunks(PIECE_SIZE).chain(parity_buffer.chunks(PIECE_SIZE));
                let tasks = streams.iter_mut().zip(chunks).map(|(s, chunk)| s.write_all(chunk));
                let results = futures::future::join_all(tasks).await;
                if results.into_iter().filter_map(Result::ok).count() < DATA_PIECES {
                    return Err(ClientError::NotEnoughtNodes.into());
                }
                data_buffer.clear();
            }

            if last_iter {
                break;
            }

            if !chunk.is_empty() {
                continue;
            }

            let Some(next) = blob.next().await else {
                last_iter = true;
                continue;
            };
            chunk = next;
        }

        Ok(())
    }

    async fn file_store_stream(
        &mut self,
        identity: NodeIdentity,
        addr: SocketAddr,
        proof: Proof<StoreContext>,
    ) -> Result<libp2p::Stream, ChatError> {
        self.dispatch_storage_stream(identity, addr, srpcs::STORE_FILE, proof).await
    }

    async fn allocate_file(
        &mut self,
        satelite: NodeIdentity,
        size: u64,
        context: impl RequestContext,
    ) -> Result<(File, [Proof<StoreContext>; MAX_PIECES]), ClientError> {
        // FIXME: get nonce more efficiently and cache it
        let proof = context.with_keys(|k| Proof::new(&k.sign, &mut 0, satelite, OsRng))?;
        let nonce = match self
            .dispatch_satelite_request(satelite, srpcs::ALLOCATE_FILE, (size, proof))
            .await
        {
            Err(ClientError::InvalidNonce(nonce)) => nonce,
            e => return e,
        };

        let proof = context.with_keys(|k| Proof::new(&k.sign, &mut { nonce }, satelite, OsRng))?;
        self.dispatch_satelite_request(satelite, srpcs::ALLOCATE_FILE, (size, proof)).await
    }

    pub async fn dispatch_storage_request<'a, R: Codec<'a>>(
        &'a mut self,
        identity: NodeIdentity,
        addr: SocketAddr,
        prefix: u8,
        request: impl Codec<'_>,
    ) -> Result<R, ChatError> {
        let (tx, rx) = oneshot::channel();
        self.sink
            .send(RequestInit::StorageRequest(RawStorageRequest {
                prefix,
                payload: request.to_bytes(),
                response: Err(tx),
                identity,
                addr,
            }))
            .await
            .map_err(|_| ChatError::ChannelClosed)?;

        self.buffer = crate::chat_timeout(rx, REQUEST_TIMEOUT)
            .await?
            .map_err(|_| ChatError::ChannelClosed)?;
        Result::<R, ChatError>::decode(&mut &self.buffer[..])
            .ok_or(ChatError::InvalidResponse)
            .and_then(std::convert::identity)
    }

    pub async fn dispatch_satelite_request<'a, R: Codec<'a>>(
        &'a mut self,
        identity: NodeIdentity,
        prefix: u8,
        request: impl Codec<'_>,
    ) -> Result<R, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.sink
            .send(RequestInit::SateliteRequest(RawSateliteRequest {
                prefix,
                payload: request.to_bytes(),
                response: tx,
                identity,
            }))
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        self.buffer = crate::timeout(rx, REQUEST_TIMEOUT)
            .await
            .ok_or(ClientError::Timeout)?
            .map_err(|_| ClientError::ChannelClosed)?;
        Result::<R, _>::decode(&mut &self.buffer[..])
            .ok_or(ClientError::InvalidResponse)
            .and_then(std::convert::identity)
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
            .send(RequestInit::OnionRequest(RawOnionRequest {
                id,
                topic: topic.into(),
                prefix,
                payload: request.to_bytes(),
                response: tx,
            }))
            .await
            .map_err(|_| ChatError::ChannelClosed)?;
        self.buffer = crate::chat_timeout(rx, REQUEST_TIMEOUT)
            .await?
            .map_err(|_| ChatError::ChannelClosed)?;
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

pub struct RequestStream {
    stream: libp2p::Stream,
    writer: PacketWriter,
    reader: PacketReader,
    buffer: Vec<u8>,
}

impl RequestStream {
    pub async fn write_arbitrary(&mut self, message: &[u8]) -> io::Result<()> {
        self.stream.write_all(message).await
    }

    pub async fn read_arbitrary(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buffer).await
    }

    pub async fn write(&mut self, message: impl for<'a> Codec<'a>) -> io::Result<()> {
        self.writer.write_packet(message).expect("wrath");
        self.writer.flush(&mut self.stream).await
    }

    pub async fn read<'a, R: Codec<'a>>(&'a mut self) -> io::Result<R> {
        self.buffer = self.reader.next_packet(&mut self.stream).await?;
        R::decode(&mut &self.buffer[..]).ok_or(io::ErrorKind::InvalidData.into())
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

                updates.push(VaultComponentId::Friend(message.0));
                messages.push(message);
            }
        }

        Ok(())
    }
}
