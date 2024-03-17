//#![allow(dead_code)]
//
//use {
//    crate::{ChatMeta, HardenedChatInvitePayload, HardenedChatMeta, MailVariants},
//    chat_spec::{unpack_messages_ref, ChatName, Identity, Nonce, Proof, UserName},
//    codec::{Codec, Reminder},
//    libp2p::futures,
//    rand::{rngs::OsRng, Rng},
//    std::{
//        cell::{Cell, RefCell},
//        rc::Rc,
//        str::FromStr,
//    },
//    wasm_bindgen_futures::wasm_bindgen::{self, prelude::wasm_bindgen},
//    web_sys::{js_sys::Array, wasm_bindgen::JsValue},
//};
//
//#[wasm_bindgen]
//struct Node {
//    inner: crate::Node,
//}
//
//#[wasm_bindgen]
//impl Node {
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn run(self) -> Result<(), JsValue> {
//        _ = self.inner.await;
//        Err("Node has been stopped.".into())
//    }
//}
//
//#[wasm_bindgen]
//struct Api {
//    inner: Cell<Option<crate::Node>>,
//    vault: Rc<RefCell<crate::Vault>>,
//    reqs: crate::Requests,
//    vault_nonce: Rc<RefCell<Nonce>>,
//    mail_action: Rc<RefCell<Nonce>>,
//    keys: crate::UserKeys,
//}
//
//impl Clone for Api {
//    fn clone(&self) -> Self {
//        Self {
//            inner: Cell::default(),
//            vault: self.vault.clone(),
//            reqs: self.reqs.clone(),
//            vault_nonce: self.vault_nonce.clone(),
//            mail_action: self.mail_action.clone(),
//            keys: self.keys.clone(),
//        }
//    }
//}
//
//#[wasm_bindgen]
//impl Api {
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn new(user_keys: UserKeys) -> Result<Api, JsValue> {
//        let (inner, vault, reqs, vault_nonce, mail_action) =
//            crate::Node::new(user_keys.inner.clone(), |v| _ = v).await.map_err(err_to_js)?;
//        Ok(Self {
//            inner: Cell::new(Some(inner)),
//            vault: Rc::new(RefCell::new(vault)),
//            reqs,
//            vault_nonce: Rc::new(RefCell::new(vault_nonce)),
//            mail_action: Rc::new(RefCell::new(mail_action)),
//            keys: user_keys.inner,
//        })
//    }
//
//    #[wasm_bindgen]
//    pub async fn take_node(&mut self) -> Option<Node> {
//        self.inner.take().map(|inner| Node { inner })
//    }
//
//    /*
//     * api.add_member(chat_name, member_identity, config);
//     * api.add_member(chat_name, member_identity, config);
//     */
//
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn add_member(
//        &self,
//        chat_name: &str,
//        member_identity: &str,
//        config: Member,
//    ) -> Result<(), JsValue> {
//        let chat = parse_chat_name(chat_name)?;
//        let member = parse_identity(member_identity)?;
//        let mut reqs = self.reqs.clone();
//        let keys = reqs.fetch_keys(member).await.map_err(err_to_js)?;
//
//        let invite = {
//            let mut vault = self.vault.borrow_mut();
//            let chat_meta = vault.chats.get_mut(&chat).ok_or("chat not found in vault")?;
//            let cp = self.keys.enc.encapsulate_choosen(&keys.enc, chat_meta.secret, OsRng);
//            MailVariants::ChatInvite { chat, cp }
//        };
//
//        reqs.send_mail(member, invite).await.map_err(err_to_js)?;
//        self.update_member(chat_name, member_identity, config).await
//    }
//
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn update_member(
//        &self,
//        chat_name: &str,
//        member_identity: &str,
//        config: Member,
//    ) -> Result<(), JsValue> {
//        let chat = parse_chat_name(chat_name)?;
//        let member = parse_identity(member_identity)?;
//        let proof = self.next_chat_proof(chat)?;
//        self.reqs.clone().add_member(proof, member, config.inner).await.map_err(err_to_js)
//    }
//
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn kick_member(&self, chat_name: &str, member_identity: &str) -> Result<(), JsValue> {
//        let chat = parse_chat_name(chat_name)?;
//        let member = parse_identity(member_identity)?;
//        let proof = self.next_chat_proof(chat)?;
//        self.reqs.clone().kick_member(proof, member).await.map_err(err_to_js)
//    }
//
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn send_message(&mut self, chat_name: &str, message: &str) -> Result<(), JsValue> {
//        let chat = parse_chat_name(chat_name)?;
//
//        let message = {
//            let vault = self.vault.borrow();
//            let chat_meta = vault.chats.get(&chat).ok_or("chat not found in vault")?;
//            let mut data = message.as_bytes().to_vec();
//            let tag = crypto::encrypt(&mut data, chat_meta.secret, OsRng);
//            data.extend(tag);
//            data
//        };
//        let proof = self.next_chat_message_proof(chat, &message)?;
//
//        self.reqs.send_message(proof, chat).await.map_err(err_to_js)
//    }
//
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn fetch_messages(&mut self, name: &str, cursor: Cursor) -> Result<Array, JsValue> {
//        let chat = parse_chat_name(name)?;
//        let chat_meta = *self.vault.borrow().chats.get(&chat).ok_or("chat not found in vault")?;
//
//        let (new_cusor, Reminder(mesages)) =
//            self.reqs.fetch_messages(chat, cursor.inner.get()).await.map_err(err_to_js)?;
//        cursor.inner.set(new_cusor);
//
//        let array = Array::new_with_length(mesages.len() as u32);
//        for (i, message) in unpack_messages_ref(mesages).enumerate() {
//            let Some(message) = chat_spec::Message::decode(&mut &message[..]) else {
//                continue;
//            };
//
//            let mut content = message.content.0.to_owned();
//            let Some(content) = crypto::decrypt(&mut content, chat_meta.secret) else {
//                continue;
//            };
//
//            let Ok(content) = std::str::from_utf8(content) else {
//                continue;
//            };
//
//            let message = Message { id: message.sender, content: content.to_owned() };
//            array.set(i as u32, JsValue::from(message));
//        }
//
//        Ok(array)
//    }
//
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn create_chat(&mut self, chat_name: &str) -> Result<(), JsValue> {
//        // TODO: do proof of work
//        let chat = parse_chat_name(chat_name)?;
//        let my_id = self.keys.identity_hash();
//
//        self.vault.borrow_mut().chats.insert(chat, ChatMeta::new());
//        let res = self.save_vault().await;
//        if res.is_err() {
//            self.vault.borrow_mut().chats.remove(&chat);
//        }
//        res?;
//
//        self.reqs.create_and_save_chat(chat, my_id).await.map_err(err_to_js)?;
//
//        Ok(())
//    }
//
//    /// @throw
//    #[wasm_bindgen]
//    pub fn create_hardened_chat(&mut self, chat_name: &str) -> Result<(), JsValue> {
//        let chat = parse_chat_name(chat_name)?;
//        self.vault.borrow_mut().hardened_chats.insert(chat, HardenedChatMeta::default());
//        Ok(())
//    }
//
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn send_hardened_chat_invite(
//        &mut self,
//        user_to_invite: &str,
//        hardened_chat_name: &str,
//    ) -> Result<(), JsValue> {
//        let to = parse_identity(user_to_invite)?;
//        let chat = parse_chat_name(hardened_chat_name)?;
//        let chat_meta = self
//            .vault
//            .borrow()
//            .hardened_chats
//            .get(&chat)
//            .ok_or("hardened chat not found in vault")?
//            .clone();
//
//        let keys = self.reqs.fetch_keys(to).await.map_err(err_to_js)?;
//        let (cp, ss) = self.keys.enc.encapsulate(&keys.enc, OsRng);
//
//        let mut payload = HardenedChatInvitePayload {
//            chat,
//            inviter: self.keys.name,
//            inviter_id: self.keys.identity_hash(),
//            members: chat_meta
//                .members
//                .iter()
//                .map(|(&name, m)| (name, m.identity))
//                .collect::<Vec<_>>(),
//        }
//        .to_bytes();
//        let tag = crypto::encrypt(&mut payload, ss, OsRng);
//        payload.extend(tag);
//
//        let mail = MailVariants::HardenedChatInvite { cp, payload };
//        self.reqs.send_mail(to, mail).await.map_err(err_to_js)
//    }
//
//    /// @throw
//    #[wasm_bindgen]
//    pub async fn send_hardened_message(
//        &mut self,
//        hardened_chat_name: &str,
//        message: &str,
//    ) -> Result<(), JsValue> {
//        let chat = parse_chat_name(hardened_chat_name)?;
//        let chat_meta = self
//            .vault
//            .borrow()
//            .hardened_chats
//            .get(&chat)
//            .ok_or("hardend chat not found in vault")?
//            .clone();
//
//        let reqs = std::iter::repeat(self.reqs.clone());
//        let requests = chat_meta.members.iter().zip(reqs).map(|((_, meta), mut reqs)| async move {
//            let mut enc_buffer = message.to_bytes();
//            let tag = crypto::encrypt(&mut enc_buffer, meta.secret, OsRng);
//            enc_buffer.extend(tag);
//
//            let nonce = OsRng.gen::<Nonce>();
//            let mail = MailVariants::HardenedChatMessage {
//                nonce,
//                chat: crypto::hash::with_nonce(chat.as_bytes(), nonce),
//                content: enc_buffer,
//            };
//            reqs.send_mail(meta.identity, mail).await.map_err(err_to_js)
//        });
//
//        futures::future::try_join_all(requests).await.map(drop)
//    }
//
//    // #[wasm_bindgen]
//    // pub async fn fetch_members(
//    //     &mut self,
//    //     name: &str,
//    //     from: &str,
//    //     limit: u32,
//    // ) -> Result<Vec<(Identity, Member)>> {
//    // }
//
//    //  #[wasm_bindgen]
//    //  pub async fn send_chat_invite(&mut self, to: &str, chat_name: &str) -> Result<()> {
//    //     // match mail {
//    //     //     MailVariants::HardenedJoinRequest { cp, payload } => todo!(),
//    //     // }
//
//    //      }
//    //      self.dispatch(rpcs::SEND_MAIL, Topic::Profile(to), mail).await.or_else(ChatError::recover)
//    //  }
//
//    // #[wasm_bindgen]
//    // pub async fn read_mail(&mut self, proof: Proof<Mail>) -> Result<Reminder> {
//    //     self.dispatch(rpcs::READ_MAIL, proof_topic(&proof), proof).await
//    // }
//
//    // #[wasm_bindgen]
//    // pub async fn fetch_my_member(&mut self, name: ChatName, me: Identity) -> Result<Member> {
//    //     let mebers = self.fetch_members(name, me, 1).await?;
//    //     mebers
//    //         .into_iter()
//    //         .next()
//    //         .and_then(|(id, m)| (id == me).then_some(m))
//    //         .ok_or(ChatError::NotMember)
//    // }
//
//    // #[wasm_bindgen]
//    // pub async fn subscribe(
//    //     &mut self,
//    //     topic: impl Into<Topic>,
//    // ) -> Result<mpsc::Receiver<SubscriptionMessage>> {
//    //     let (tx, mut rx) = mpsc::channel(0);
//    //     let id = CallId::new();
//    //     let topic: Topic = topic.into();
//    //     self.sink
//    //         .try_send(RequestInit::Subscription(SubscriptionInit { id, topic, channel: tx }))
//    //         .map_err(|_| ChatError::ChannelClosed)?;
//
//    //     let init =
//    //         crate::timeout(rx.next(), REQUEST_TIMEOUT).await?.ok_or(ChatError::ChannelClosed)?;
//    //     Result::<()>::decode(&mut &init[..]).ok_or(ChatError::InvalidResponse)??;
//
//    //     Ok(rx)
//    // }
//
//    // #[wasm_bindgen]
//    // pub fn unsubscribe(&mut self, topic: impl Into<Topic>) {
//    //     let topic: Topic = topic.into();
//    //     self.sink.try_send(RequestInit::EndSubscription(topic)).unwrap();
//    // }
//
//    fn next_chat_proof(&self, name: ChatName) -> Result<Proof<ChatName>, JsValue> {
//        let mut vault = self.vault.borrow_mut();
//        let chat = vault.chats.get_mut(&name).ok_or("chat not found in vault")?;
//        Ok(Proof::new(&self.keys.sign, &mut chat.action_no, name, OsRng))
//    }
//
//    fn next_chat_message_proof<'a>(
//        &mut self,
//        name: ChatName,
//        message: &'a [u8],
//    ) -> Result<Proof<Reminder<'a>>, JsValue> {
//        let mut vault = self.vault.borrow_mut();
//        let chat = vault.chats.get_mut(&name).ok_or("chat not found in vault")?;
//        Ok(Proof::new(&self.keys.sign, &mut chat.action_no, Reminder(message), OsRng))
//    }
//
//    async fn save_vault(&mut self) -> Result<(), JsValue> {
//        let mut vault = self.vault.borrow().to_bytes();
//        let tag = crypto::encrypt(&mut vault, self.keys.vault, OsRng);
//        vault.extend(tag);
//        let proof = Proof::new(
//            &self.keys.sign,
//            &mut self.vault_nonce.borrow_mut(),
//            Reminder(&vault),
//            OsRng,
//        );
//        self.reqs.set_vault(proof).await.map_err(err_to_js)
//    }
//}
//
//#[wasm_bindgen]
//struct Message {
//    id: Identity,
//    content: String,
//}
//
//#[wasm_bindgen]
//impl Message {
//    #[wasm_bindgen(getter)]
//    pub fn id(&self) -> String {
//        hex::encode(self.id)
//    }
//
//    #[wasm_bindgen(getter)]
//    pub fn content(&self) -> String {
//        self.content.clone()
//    }
//}
//
//#[wasm_bindgen]
//struct Cursor {
//    inner: Rc<Cell<chat_spec::Cursor>>,
//}
//
//#[wasm_bindgen]
//struct Member {
//    inner: chat_spec::Member,
//}
//
//#[wasm_bindgen]
//impl Member {
//    #[wasm_bindgen]
//    pub fn best() -> Member {
//        Member { inner: chat_spec::Member::best() }
//    }
//
//    #[wasm_bindgen]
//    pub fn worst() -> Member {
//        Member { inner: chat_spec::Member::worst() }
//    }
//
//    #[wasm_bindgen(getter)]
//    pub fn rank(&self) -> u32 {
//        self.inner.rank
//    }
//
//    #[wasm_bindgen(setter)]
//    pub fn set_rank(&mut self, rank: u32) {
//        self.inner.rank = rank;
//    }
//
//    #[wasm_bindgen(getter)]
//    pub fn action_cooldown_ms(&self) -> u32 {
//        self.inner.action_cooldown_ms
//    }
//
//    #[wasm_bindgen(setter)]
//    pub fn set_action_cooldown_ms(&mut self, ms: u32) {
//        self.inner.action_cooldown_ms = ms;
//    }
//
//    #[wasm_bindgen(getter)]
//    pub fn can_send(&self) -> bool {
//        self.inner.permissions.contains(chat_spec::Permissions::SEND)
//    }
//
//    #[wasm_bindgen(setter)]
//    pub fn set_can_send(&mut self, can: bool) {
//        self.inner.permissions.set(chat_spec::Permissions::SEND, can);
//    }
//
//    #[wasm_bindgen(getter)]
//    pub fn can_kick(&self) -> bool {
//        self.inner.permissions.contains(chat_spec::Permissions::KICK)
//    }
//
//    #[wasm_bindgen(setter)]
//    pub fn set_can_kick(&mut self, can: bool) {
//        self.inner.permissions.set(chat_spec::Permissions::KICK, can);
//    }
//
//    #[wasm_bindgen(getter)]
//    pub fn can_invite(&self) -> bool {
//        self.inner.permissions.contains(chat_spec::Permissions::INVITE)
//    }
//
//    #[wasm_bindgen(setter)]
//    pub fn set_can_invite(&mut self, can: bool) {
//        self.inner.permissions.set(chat_spec::Permissions::INVITE, can);
//    }
//
//    #[wasm_bindgen(getter)]
//    pub fn can_rate_limit(&self) -> bool {
//        self.inner.permissions.contains(chat_spec::Permissions::RATE_LIMIT)
//    }
//
//    #[wasm_bindgen(setter)]
//    pub fn set_can_rate_limit(&mut self, can: bool) {
//        self.inner.permissions.set(chat_spec::Permissions::RATE_LIMIT, can);
//    }
//}
//
//#[wasm_bindgen]
//struct UserKeys {
//    inner: crate::UserKeys,
//}
//
//#[wasm_bindgen]
//impl UserKeys {
//    /// @throw
//    #[wasm_bindgen]
//    pub fn new(username: &str, password: &str) -> Result<UserKeys, JsValue> {
//        let name = UserName::from_str(username).map_err(err_to_js)?;
//        Ok(UserKeys { inner: crate::UserKeys::new(name, password) })
//    }
//}
//
//pub fn err_to_js(e: impl std::fmt::Display) -> JsValue {
//    JsValue::from_str(&e.to_string())
//}
//
//pub fn parse_identity(s: &str) -> Result<Identity, JsValue> {
//    let mut id = Identity::default();
//    hex::decode_to_slice(s, &mut id).map_err(err_to_js)?;
//    Ok(id)
//}
//
//pub fn parse_chat_name(s: &str) -> Result<ChatName, JsValue> {
//    ChatName::from_str(s).map_err(err_to_js)
//}
