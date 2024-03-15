#![allow(dead_code)]

use {
    crate::{ChatMeta, MailVariants},
    chain_api::Nonce,
    chat_spec::{unpack_messages_ref, ChatName, Identity, Proof, UserName},
    component_utils::{Codec, Reminder},
    crypto::TransmutationCircle,
    rand::rngs::OsRng,
    std::str::FromStr,
    wasm_bindgen_futures::wasm_bindgen::{self, prelude::wasm_bindgen},
    web_sys::{js_sys::Array, wasm_bindgen::JsValue},
};

#[wasm_bindgen]
struct Node {
    inner: crate::Node,
}

#[wasm_bindgen]
impl Node {
    /// @throw
    #[wasm_bindgen]
    pub async fn run(self) -> Result<(), JsValue> {
        _ = self.inner.await;
        Err("Node has been stopped.".into())
    }
}

#[wasm_bindgen]
struct Api {
    inner: Option<crate::Node>,
    vault: crate::Vault,
    reqs: crate::RequestDispatch,
    vault_nonce: Nonce,
    mail_action: Nonce,
    keys: crate::UserKeys,
}

#[wasm_bindgen]
impl Api {
    /// @throw
    #[wasm_bindgen]
    pub async fn new(user_keys: UserKeys) -> Result<Api, JsValue> {
        let (inner, vault, reqs, vault_nonce, mail_action) =
            crate::Node::new(user_keys.inner.clone(), |v| _ = v).await.map_err(err_to_js)?;
        Ok(Self {
            inner: Some(inner),
            vault,
            reqs,
            vault_nonce,
            mail_action,
            keys: user_keys.inner,
        })
    }

    #[wasm_bindgen]
    pub async fn take_node(&mut self) -> Option<Node> {
        self.inner.take().map(|inner| Node { inner })
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn add_member(
        &mut self,
        chat_name: &str,
        member_identity: &str,
        config: Member,
    ) -> Result<(), JsValue> {
        let chat = parse_chat_name(chat_name)?;
        let member = parse_identity(member_identity)?;
        let chat_meta = self.vault.chats.get_mut(&chat).ok_or("chat not found in vault")?;

        let keys = self.reqs.fetch_keys(member).await.map_err(err_to_js)?;
        let pk = crypto::enc::PublicKey::from_bytes(keys.enc);
        let cp = self.keys.enc.encapsulate_choosen(&pk, chat_meta.secret, OsRng);
        let invite = MailVariants::ChatInvite { chat, cp: cp.into_bytes() };
        self.reqs.send_mail(member, invite).await.map_err(err_to_js)?;

        self.update_member(chat_name, member_identity, config).await
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn update_member(
        &mut self,
        chat_name: &str,
        member_identity: &str,
        config: Member,
    ) -> Result<(), JsValue> {
        let chat = parse_chat_name(chat_name)?;
        let member = parse_identity(member_identity)?;
        let proof = self.next_chat_proof(chat)?;
        self.reqs.add_member(proof, member, config.inner).await.map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn kick_member(
        &mut self,
        chat_name: &str,
        member_identity: &str,
    ) -> Result<(), JsValue> {
        let chat = parse_chat_name(chat_name)?;
        let member = parse_identity(member_identity)?;
        let proof = self.next_chat_proof(chat)?;
        self.reqs.kick_member(proof, member).await.map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn send_message(&mut self, chat_name: &str, message: &str) -> Result<(), JsValue> {
        let chat = parse_chat_name(chat_name)?;
        let chat_meta = self.vault.chats.get(&chat).ok_or("chat not found in vault")?;
        let mut data = message.as_bytes().to_vec();
        let tag = crypto::encrypt(&mut data, chat_meta.secret, OsRng);
        data.extend(tag);
        let proof = self.next_chat_message_proof(chat, &data)?;
        self.reqs.send_message(proof, chat).await.map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn fetch_messages(
        &mut self,
        name: &str,
        cursor: &mut Cursor,
    ) -> Result<Array, JsValue> {
        let chat = parse_chat_name(name)?;
        let chat_meta = self.vault.chats.get(&chat).ok_or("chat not found in vault")?;

        let (new_cusor, Reminder(mesages)) =
            self.reqs.fetch_messages(chat, cursor.inner).await.map_err(err_to_js)?;
        *cursor = Cursor { inner: new_cusor };

        let array = Array::new_with_length(mesages.len() as u32);
        for (i, message) in unpack_messages_ref(mesages).enumerate() {
            let Some(message) = chat_spec::Message::decode(&mut &message[..]) else {
                continue;
            };

            let mut content = message.content.0.to_owned();
            let Some(content) = crypto::decrypt(&mut content, chat_meta.secret) else {
                continue;
            };

            let Ok(content) = std::str::from_utf8(content) else {
                continue;
            };

            let message = Message { id: message.identity, content: content.to_owned() };
            array.set(i as u32, JsValue::from(message));
        }

        Ok(array)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn create_chat(&mut self, chat_name: &str) -> Result<(), JsValue> {
        // TODO: do proof of work
        let chat = parse_chat_name(chat_name)?;
        let my_id = self.keys.identity_hash();

        self.vault.chats.insert(chat, ChatMeta::new());
        let res = self.save_vault().await;
        if res.is_err() {
            self.vault.chats.remove(&chat);
        }
        res?;

        self.reqs.create_chat(chat, my_id).await.map_err(err_to_js)?;

        Ok(())
    }

    // /// @throw
    // #[wasm_bindgen]
    // pub async fn hardened_chat_invie(
    //     &mut self,
    //     user_to_invite: &str,
    //     hardened_chat_name: &str,
    // ) -> Result<(), JsValue> {
    //     let to = parse_identity(user_to_invite)?;
    //     let chat = parse_chat_name(hardened_chat_name)?;
    //     let chat_meta = self.vault.hardened_chats.get(&chat).ok_or("chat not found in vault")?;
    //     let keys = self.reqs.fetch_keys(to).await.map_err(err_to_js)?;
    //     let pk = crypto::enc::PublicKey::from_bytes(keys.enc);
    //     let cp = self.keys.enc.encapsulate(&pk, OsRng);
    //     let mail = MailVariants::HardenedChatInvite { cp, payload: };
    //     self.reqs.send_mail(to, mail).await.map_err(err_to_js)
    // }

    //  #[wasm_bindgen]
    //  pub async fn send_chat_invite(&mut self, to: &str, chat_name: &str) -> Result<()> {
    //     // match mail {
    //     //     MailVariants::ChatInvite { chat, cp } => todo!(),
    //     //     MailVariants::HardenedJoinRequest { cp, payload } => todo!(),
    //     //     MailVariants::HardenedChatMessage { nonce, chat, content } => todo!(),
    //     //     MailVariants::HardenedChatInvite { cp, payload } => todo!(),
    //     // }

    //      }
    //      self.dispatch(rpcs::SEND_MAIL, Topic::Profile(to), mail).await.or_else(ChatError::recover)
    //  }

    // #[wasm_bindgen]
    // pub async fn read_mail(&mut self, proof: Proof<Mail>) -> Result<Reminder> {
    //     self.dispatch(rpcs::READ_MAIL, proof_topic(&proof), proof).await
    // }

    // #[wasm_bindgen]
    // pub async fn fetch_members(
    //     &mut self,
    //     name: ChatName,
    //     from: Identity,
    //     limit: u32,
    // ) -> Result<Vec<(Identity, Member)>> {
    //     self.dispatch(rpcs::FETCH_MEMBERS, Topic::Chat(name), (from, limit)).await
    // }

    // #[wasm_bindgen]
    // pub async fn fetch_my_member(&mut self, name: ChatName, me: Identity) -> Result<Member> {
    //     let mebers = self.fetch_members(name, me, 1).await?;
    //     mebers
    //         .into_iter()
    //         .next()
    //         .and_then(|(id, m)| (id == me).then_some(m))
    //         .ok_or(ChatError::NotMember)
    // }

    // #[wasm_bindgen]
    // pub async fn subscribe(
    //     &mut self,
    //     topic: impl Into<Topic>,
    // ) -> Result<mpsc::Receiver<SubscriptionMessage>> {
    //     let (tx, mut rx) = mpsc::channel(0);
    //     let id = CallId::new();
    //     let topic: Topic = topic.into();
    //     self.sink
    //         .try_send(RequestInit::Subscription(SubscriptionInit { id, topic, channel: tx }))
    //         .map_err(|_| ChatError::ChannelClosed)?;

    //     let init =
    //         crate::timeout(rx.next(), REQUEST_TIMEOUT).await?.ok_or(ChatError::ChannelClosed)?;
    //     Result::<()>::decode(&mut &init[..]).ok_or(ChatError::InvalidResponse)??;

    //     Ok(rx)
    // }

    // #[wasm_bindgen]
    // pub fn unsubscribe(&mut self, topic: impl Into<Topic>) {
    //     let topic: Topic = topic.into();
    //     self.sink.try_send(RequestInit::EndSubscription(topic)).unwrap();
    // }

    fn next_chat_proof(&mut self, name: ChatName) -> Result<Proof<ChatName>, JsValue> {
        let chat = self.vault.chats.get_mut(&name).ok_or("chat not found in vault")?;
        Ok(Proof::new(&self.keys.sign, &mut chat.action_no, name, OsRng))
    }

    fn next_chat_message_proof<'a>(
        &mut self,
        name: ChatName,
        message: &'a [u8],
    ) -> Result<Proof<Reminder<'a>>, JsValue> {
        let chat = self.vault.chats.get_mut(&name).ok_or("chat not found in vault")?;
        Ok(Proof::new(&self.keys.sign, &mut chat.action_no, Reminder(message), OsRng))
    }

    async fn save_vault(&mut self) -> Result<(), JsValue> {
        let mut vault = self.vault.to_bytes();
        let tag = crypto::encrypt(&mut vault, self.keys.vault, OsRng);
        vault.extend(tag);
        let proof = Proof::new(&self.keys.sign, &mut self.vault_nonce, Reminder(&vault), OsRng);
        self.reqs.set_vault(proof).await.map_err(err_to_js)
    }
}

#[wasm_bindgen]
struct Message {
    id: Identity,
    content: String,
}

#[wasm_bindgen]
impl Message {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        hex::encode(self.id)
    }

    #[wasm_bindgen(getter)]
    pub fn content(&self) -> String {
        self.content.clone()
    }
}

#[wasm_bindgen]
struct Cursor {
    inner: chat_spec::Cursor,
}

#[wasm_bindgen]
struct Member {
    inner: chat_spec::Member,
}

#[wasm_bindgen]
impl Member {
    #[wasm_bindgen]
    pub fn best() -> Member {
        Member { inner: chat_spec::Member::best() }
    }

    #[wasm_bindgen]
    pub fn worst() -> Member {
        Member { inner: chat_spec::Member::worst() }
    }

    #[wasm_bindgen(getter)]
    pub fn rank(&self) -> u32 {
        self.inner.rank
    }

    #[wasm_bindgen(setter)]
    pub fn set_rank(&mut self, rank: u32) {
        self.inner.rank = rank;
    }

    #[wasm_bindgen(getter)]
    pub fn action_cooldown_ms(&self) -> u32 {
        self.inner.action_cooldown_ms
    }

    #[wasm_bindgen(setter)]
    pub fn set_action_cooldown_ms(&mut self, ms: u32) {
        self.inner.action_cooldown_ms = ms;
    }

    #[wasm_bindgen(getter)]
    pub fn can_send(&self) -> bool {
        self.inner.permissions.contains(chat_spec::Permissions::SEND)
    }

    #[wasm_bindgen(setter)]
    pub fn set_can_send(&mut self, can: bool) {
        self.inner.permissions.set(chat_spec::Permissions::SEND, can);
    }

    #[wasm_bindgen(getter)]
    pub fn can_kick(&self) -> bool {
        self.inner.permissions.contains(chat_spec::Permissions::KICK)
    }

    #[wasm_bindgen(setter)]
    pub fn set_can_kick(&mut self, can: bool) {
        self.inner.permissions.set(chat_spec::Permissions::KICK, can);
    }

    #[wasm_bindgen(getter)]
    pub fn can_invite(&self) -> bool {
        self.inner.permissions.contains(chat_spec::Permissions::INVITE)
    }

    #[wasm_bindgen(setter)]
    pub fn set_can_invite(&mut self, can: bool) {
        self.inner.permissions.set(chat_spec::Permissions::INVITE, can);
    }

    #[wasm_bindgen(getter)]
    pub fn can_rate_limit(&self) -> bool {
        self.inner.permissions.contains(chat_spec::Permissions::RATE_LIMIT)
    }

    #[wasm_bindgen(setter)]
    pub fn set_can_rate_limit(&mut self, can: bool) {
        self.inner.permissions.set(chat_spec::Permissions::RATE_LIMIT, can);
    }
}

#[wasm_bindgen]
struct UserKeys {
    inner: crate::UserKeys,
}

#[wasm_bindgen]
impl UserKeys {
    /// @throw
    #[wasm_bindgen]
    pub fn new(username: &str, password: &str) -> Result<UserKeys, JsValue> {
        let name = UserName::from_str(username).map_err(err_to_js)?;
        Ok(UserKeys { inner: crate::UserKeys::new(name, password) })
    }
}

pub fn err_to_js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

pub fn parse_identity(s: &str) -> Result<Identity, JsValue> {
    let mut id = Identity::default();
    hex::decode_to_slice(s, &mut id).map_err(err_to_js)?;
    Ok(id)
}

pub fn parse_chat_name(s: &str) -> Result<ChatName, JsValue> {
    ChatName::from_str(s).map_err(err_to_js)
}
