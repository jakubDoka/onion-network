#![allow(dead_code)]
#![allow(non_snake_case)]

use {
    crate::{MailVariants, RawChatMessage, RequestContext, SubscriptionMessage},
    anyhow::Context as _,
    chat_spec::{ChatName, Identity, Nonce, UserName},
    codec::{Codec, Reminder},
    libp2p::futures::{channel::mpsc, StreamExt},
    std::{
        cell::{Cell, RefCell},
        rc::Rc,
        str::FromStr,
    },
    wasm_bindgen_futures::{
        spawn_local,
        wasm_bindgen::{self, prelude::wasm_bindgen},
    },
    web_sys::wasm_bindgen::JsValue,
};

pub struct Context {
    vault: RefCell<crate::Vault>,
    vault_nonce: Cell<Nonce>,
    mail_action: Cell<Nonce>,
    keys: crate::UserKeys,
}

impl RequestContext for Context {
    fn with_vault<R>(
        &self,
        action: impl FnOnce(&mut crate::Vault) -> crate::Result<R>,
    ) -> crate::Result<R> {
        let mut vault = self.vault.borrow_mut();
        action(&mut vault)
    }

    fn with_keys<R>(&self, action: impl FnOnce(&crate::UserKeys) -> R) -> crate::Result<R> {
        Ok(action(&self.keys))
    }

    fn with_vault_version<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> crate::Result<R> {
        let mut vault_nonce = self.vault_nonce.get();
        let res = action(&mut vault_nonce);
        self.vault_nonce.set(vault_nonce);
        Ok(res)
    }

    fn with_mail_action<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> crate::Result<R> {
        let mut mail_action = self.mail_action.get();
        let res = action(&mut mail_action);
        self.mail_action.set(mail_action);
        Ok(res)
    }
}

#[wasm_bindgen]
#[derive(Clone)]
struct Api {
    reqs: crate::Requests,
    ctx: Rc<Context>,
}

#[wasm_bindgen]
impl Api {
    /// @throw
    #[wasm_bindgen]
    pub async fn new(user_keys: UserKeys) -> Result<Api, JsValue> {
        let (inner, vault, reqs, vault_nonce, mail_action) =
            crate::Node::new(user_keys.inner.clone(), |v| _ = v).await.map_err(err_to_js)?;

        spawn_local(async move { _ = inner.await });

        Ok(Api {
            reqs,
            ctx: Rc::new(Context {
                vault: RefCell::new(vault),
                vault_nonce: Cell::new(vault_nonce),
                mail_action: Cell::new(mail_action),
                keys: user_keys.inner,
            }),
        })
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn create_chat(&mut self, chat_name: &str) -> Result<(), JsValue> {
        let chat = parse_chat_name(chat_name)?;
        self.reqs.clone().create_and_save_chat(chat, self.ctx.as_ref()).await.map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn create_hardened_chat(&mut self, chat_name: &str) -> Result<(), JsValue> {
        let chat = parse_chat_name(chat_name)?;
        self.reqs.create_hardened_chat(chat, self.ctx.as_ref()).await.map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn send_hardened_chat_invite(
        &mut self,
        user_name_to_invite: &str,
        hardened_chat_name: &str,
    ) -> Result<(), JsValue> {
        let to = parse_chat_name(user_name_to_invite)?;
        let chat = parse_chat_name(hardened_chat_name)?;
        self.reqs
            .clone()
            .send_hardened_chat_invite(to, chat, self.ctx.as_ref())
            .await
            .map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn send_hardened_message(
        &mut self,
        hardened_chat_name: &str,
        message: &str,
    ) -> Result<Usernames, JsValue> {
        let chat = parse_chat_name(hardened_chat_name)?;
        self.reqs
            .clone()
            .send_hardened_message(chat, message.as_bytes(), self.ctx.as_ref())
            .await
            .map_err(err_to_js)
            .map(|list| Usernames { list: list.into_iter().map(|s| s.to_string()).collect() })
    }
}

#[wasm_bindgen]
struct ChatSubscription {
    target: ChatName,
    member: chat_spec::Member,
    stream: mpsc::Receiver<SubscriptionMessage>,
    reqs: crate::Requests,
    ctx: Rc<Context>,
}

#[wasm_bindgen]
impl ChatSubscription {
    /// @throw
    #[wasm_bindgen]
    pub async fn new(api: Api, chat_name: &str) -> Result<ChatSubscription, JsValue> {
        let chat = parse_chat_name(chat_name)?;
        let my_member = api
            .reqs
            .clone()
            .fetch_my_member(chat, api.ctx.keys.identity_hash())
            .await
            .map_err(err_to_js)?;
        api.ctx
            .with_vault(|vault| {
                vault
                    .chats
                    .get_mut(&chat)
                    .map(|c| c.action_no = my_member.action)
                    .with_context(crate::vault_chat_404(chat))
            })
            .map_err(err_to_js)?;
        let stream = api.reqs.clone().subscribe(chat).await.map_err(err_to_js)?;
        Ok(ChatSubscription {
            target: chat,
            member: my_member,
            stream,
            reqs: api.reqs,
            ctx: api.ctx,
        })
    }

    #[wasm_bindgen]
    pub fn member(&self) -> Member {
        Member { identity: self.ctx.keys.identity_hash(), inner: self.member }
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn invite_member(
        &self,
        member_identity: &str,
        config: Member,
    ) -> Result<(), JsValue> {
        let member = parse_identity(member_identity)?;
        self.reqs
            .clone()
            .invite_member(self.target, member, self.ctx.as_ref(), config.inner)
            .await
            .map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn update_member(
        &self,
        member_identity: &str,
        config: Member,
    ) -> Result<(), JsValue> {
        let member = parse_identity(member_identity)?;
        self.reqs
            .clone()
            .update_member(self.target, member, config.inner, self.ctx.as_ref())
            .await
            .map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn kick_member(&self, member_identity: &str) -> Result<(), JsValue> {
        let member = parse_identity(member_identity)?;
        self.reqs
            .clone()
            .kick_member(self.target, member, self.ctx.as_ref())
            .await
            .map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn fetch_members(
        &mut self,
        starting_from_identity: &str,
        limit: u32,
    ) -> Result<Members, JsValue> {
        let from = parse_identity(starting_from_identity)?;
        self.reqs.clone().fetch_members(self.target, from, limit).await.map_err(err_to_js).map(
            |list| Members {
                list: list.into_iter().map(|(id, m)| Member { identity: id, inner: m }).collect(),
            },
        )
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn send_message(&mut self, message: &str) -> Result<(), JsValue> {
        self.reqs
            .clone()
            .send_encrypted_message(self.target, message.to_bytes().to_vec(), self.ctx.as_ref())
            .await
            .map_err(err_to_js)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn fetch_messages(
        &mut self,
        name: &str,
        cursor: Cursor,
    ) -> Result<Messages, JsValue> {
        let chat = parse_chat_name(name)?;
        let mut c = cursor.inner.get();
        let messages = self
            .reqs
            .fetch_and_decrypt_messages(chat, &mut c, self.ctx.as_ref())
            .await
            .map_err(err_to_js)?;
        cursor.inner.set(c);

        Ok(Messages { list: messages.into_iter().map(Into::into).collect() })
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn next(&mut self) -> Result<ChatEvent, JsValue> {
        while let Some(bytes) = self.stream.next().await {
            let Some(event) = chat_spec::ChatEvent::decode(&mut &bytes[..]) else {
                log::warn!("invalid message: {:?}", bytes);
                continue;
            };

            let event = match event {
                chat_spec::ChatEvent::Message(id, Reminder(message)) => {
                    let msg = "chat was deleted while subscription is active";
                    let secret = self.ctx.vault.borrow().chats.get(&self.target).ok_or(msg)?.secret;
                    let mut message = message.to_owned();
                    let Some(message) = crypto::decrypt(&mut message, secret) else {
                        log::warn!("failed to decrypt message: {:?}", message);
                        continue;
                    };

                    let Some(mut message) = RawChatMessage::decode(&mut &message[..]) else {
                        log::warn!("invalid message: {:?}", message);
                        continue;
                    };
                    message.identity = id;

                    ChatEvent { message: Some(message.into()), ..Default::default() }
                }
                chat_spec::ChatEvent::Member(id, config) => ChatEvent {
                    member: Some(Member { identity: id, inner: config }),
                    ..Default::default()
                },
                chat_spec::ChatEvent::MemberRemoved(id) => {
                    ChatEvent { member_removed: Some(hex::encode(id)), ..Default::default() }
                }
            };

            return Ok(event);
        }

        Err(JsValue::from_str("chat subscription closed"))
    }

    #[wasm_bindgen]
    pub fn unsubscribe(&self) {
        self.reqs.clone().unsubscribe(self.target);
    }
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Default)]
struct ChatEvent {
    pub message: Option<Message>,
    pub member: Option<Member>,
    pub member_removed: Option<String>,
}

#[wasm_bindgen]
struct ProfileSubscription {
    stream: mpsc::Receiver<SubscriptionMessage>,
    reqs: crate::Requests,
    context: Rc<Context>,
}

#[wasm_bindgen]
impl ProfileSubscription {
    /// @throw
    #[wasm_bindgen]
    pub async fn new(api: Api) -> Result<ProfileSubscription, JsValue> {
        let stream =
            api.reqs.clone().subscribe(api.ctx.keys.identity_hash()).await.map_err(err_to_js)?;
        Ok(ProfileSubscription { stream, reqs: api.reqs, context: api.ctx })
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn next(&mut self) -> Result<HardenedChatMessage, JsValue> {
        while let Some(bytes) = self.stream.next().await {
            let Some(mail) = MailVariants::decode(&mut &bytes[..]) else {
                log::warn!("invalid message: {:?}", bytes);
                continue;
            };

            let (message, id) =
                mail.handle(self.context.as_ref(), self.reqs.clone()).await.map_err(err_to_js)?;

            if let Some(vid) = id {
                self.reqs
                    .save_vault_component(vid, self.context.as_ref())
                    .await
                    .map_err(err_to_js)?;
            }

            if let Some((username, message, chatname)) = message {
                return Ok(HardenedChatMessage {
                    sender: username.to_string(),
                    content: message.to_string(),
                    owner: self.context.keys.name.to_string(),
                    chat_name: chatname.to_string(),
                });
            }
        }

        Err(JsValue::from_str("profile subscription closed"))
    }

    /// @throw
    #[wasm_bindgen]
    pub fn unsubscribe(&self) {
        self.reqs.clone().unsubscribe(self.context.keys.identity_hash());
    }
}

#[wasm_bindgen(getter_with_clone)]
struct HardenedChatMessage {
    pub sender: String,
    pub content: String,
    pub owner: String,
    pub chat_name: String,
}

#[wasm_bindgen]
struct Usernames {
    #[wasm_bindgen(getter_with_clone)]
    pub list: Vec<String>,
}

#[wasm_bindgen]
struct Messages {
    #[wasm_bindgen(getter_with_clone)]
    pub list: Vec<Message>,
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone)]
struct Message {
    pub name: String,
    id: Identity,
    pub content: String,
}

impl From<RawChatMessage> for Message {
    fn from(m: RawChatMessage) -> Self {
        Self { name: m.sender.to_string(), id: m.identity, content: m.content }
    }
}

#[wasm_bindgen]
impl Message {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        hex::encode(self.id)
    }
}

#[wasm_bindgen]
struct Cursor {
    inner: Rc<Cell<chat_spec::Cursor>>,
}

#[wasm_bindgen]
struct Members {
    #[wasm_bindgen(getter_with_clone)]
    pub list: Vec<Member>,
}

#[wasm_bindgen]
#[derive(Clone)]
struct Member {
    identity: Identity,
    inner: chat_spec::Member,
}

#[wasm_bindgen]
impl Member {
    #[wasm_bindgen]
    pub fn best(identity: &str) -> Result<Member, JsValue> {
        let identity = parse_identity(identity)?;
        Ok(Self { identity, inner: chat_spec::Member::best() })
    }

    #[wasm_bindgen]
    pub fn worst(identity: &str) -> Result<Member, JsValue> {
        let identity = parse_identity(identity)?;
        Ok(Self { identity, inner: chat_spec::Member::worst() })
    }

    pub fn identity(&self) -> String {
        hex::encode(self.identity)
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
