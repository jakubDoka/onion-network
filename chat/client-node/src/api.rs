#![allow(dead_code)]
#![allow(non_snake_case)]

use {
    crate::{requests::MailVariants, ChainClientExt, RawChatMessage, RequestContext},
    anyhow::Context as _,
    chat_spec::{ChatName, Identity, UserName},
    codec::ReminderOwned,
    crypto::proof::Nonce,
    libp2p::futures::{channel::mpsc, future::TryJoinAll, StreamExt, TryFutureExt},
    std::{
        cell::{Cell, RefCell},
        ops::Deref,
        rc::Rc,
        str::FromStr,
    },
    wasm_bindgen_futures::{
        spawn_local,
        wasm_bindgen::{self, prelude::wasm_bindgen},
    },
    web_sys::wasm_bindgen::JsValue,
};

pub struct Inner {
    vault: RefCell<crate::Vault>,
    vault_nonce: Cell<Nonce>,
    mail_action: Cell<Nonce>,
    keys: crate::UserKeys,
    node_handle: crate::NodeHandle,
}

#[wasm_bindgen]
pub struct Context {
    inner: Rc<Inner>,
}

impl Deref for Context {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl RequestContext for Context {
    fn try_with_vault<R>(
        &self,
        action: impl FnOnce(&mut crate::Vault) -> anyhow::Result<R>,
    ) -> anyhow::Result<R> {
        action(&mut self.vault.borrow_mut())
    }

    fn with_vault<R>(&self, action: impl FnOnce(&mut crate::Vault) -> R) -> anyhow::Result<R> {
        Ok(action(&mut self.vault.borrow_mut()))
    }

    fn with_keys<R>(&self, action: impl FnOnce(&crate::UserKeys) -> R) -> anyhow::Result<R> {
        Ok(action(&self.keys))
    }

    fn with_vault_version<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> anyhow::Result<R> {
        let mut vault_nonce = self.vault_nonce.get();
        let res = action(&mut vault_nonce);
        self.vault_nonce.set(vault_nonce);
        Ok(res)
    }

    fn with_mail_action<R>(&self, action: impl FnOnce(&mut Nonce) -> R) -> anyhow::Result<R> {
        let mut mail_action = self.mail_action.get();
        let res = action(&mut mail_action);
        self.mail_action.set(mail_action);
        Ok(res)
    }

    fn subscription_for<T: Into<chat_spec::Topic> + Clone>(
        &self,
        topic: T,
    ) -> impl std::future::Future<Output = anyhow::Result<crate::Sub<T>>> {
        self.node_handle.subscription_for(topic).map_err(Into::into)
    }
}

#[wasm_bindgen]
impl Context {
    /// @throw
    #[wasm_bindgen]
    pub async fn new(user_keys: UserKeys) -> Result<Context, Error> {
        let (inner, vault, reqs, vault_nonce, mail_action) =
            crate::Node::new(user_keys.inner.clone(), |v| _ = v).await?;

        spawn_local(async move { _ = inner.await });

        Ok(Self {
            inner: Rc::new(Inner {
                vault: RefCell::new(vault),
                vault_nonce: Cell::new(vault_nonce),
                mail_action: Cell::new(mail_action),
                keys: user_keys.inner,
                node_handle: reqs,
            }),
        })
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn create_chat(&mut self, chat_name: &str) -> Result<(), Error> {
        let chat = parse_chat_name(chat_name)?;
        Ok(self.create_and_save_chat(chat).await?)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn send_friend_request_to(
        &mut self,
        user_name_to_send_request_to: &str,
    ) -> Result<(), Error> {
        let to = parse_username(user_name_to_send_request_to)?;
        Ok(self.send_friend_request(to).await?)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn send_friend_message_to(
        &mut self,
        friend_username: &str,
        message: String,
    ) -> Result<(), Error> {
        let name = parse_username(friend_username)?;
        Ok(self.send_frined_message(name, message).await?)
    }
}

#[wasm_bindgen]
struct ChatSubscription {
    target: ChatName,
    member: chat_spec::Member,
    stream: mpsc::Receiver<chat_spec::ChatEvent>,
    cx: Context,
}

#[wasm_bindgen]
impl ChatSubscription {
    /// @throw
    #[wasm_bindgen]
    pub async fn new(api: Context, chat_name: &str) -> Result<ChatSubscription, Error> {
        let chat = parse_chat_name(chat_name)?;
        let mut sub = api.subscription_for(chat).await?;
        let my_member = sub.fetch_my_member(api.with_keys(crate::UserKeys::identity)?).await?;
        api.with_chat_and_keys(chat, |c, _| c.action_no = my_member.action)?;
        let stream = sub.subscribe().await.context("failed to subscribe")?;
        Ok(ChatSubscription { target: chat, member: my_member, stream, cx: api })
    }

    #[wasm_bindgen]
    pub fn member(&self) -> Member {
        Member { name: self.cx.keys.name, identity: self.cx.keys.identity(), inner: self.member }
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn invite_member(&self, member: &str, config: Member) -> Result<(), Error> {
        let member = parse_username(member)?;
        Ok(self.cx.invite_member(self.target, member, config.inner).await?)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn update_member(&self, member_identity: &str, config: Member) -> Result<(), Error> {
        let member = parse_identity(member_identity)?;
        Ok(self.cx.update_member(self.target, member, config.inner).await?)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn kick_member(&self, member_identity: &str) -> Result<(), Error> {
        let member = parse_identity(member_identity)?;
        Ok(self.cx.kick_member(self.target, member).await?)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn fetch_members(
        &mut self,
        starting_from_identity: &str,
        limit: u32,
    ) -> Result<Members, Error> {
        let from = parse_identity(starting_from_identity)?;
        let members =
            self.cx.subscription_for(self.target).await?.fetch_members(from, limit).await?;

        let scx = &self.cx;
        Ok(Members {
            list: members
                .into_iter()
                .map(|(id, m)| async move {
                    Ok::<_, anyhow::Error>(Member {
                        name: scx.keys.chain_client().await?.fetch_username(from).await?,
                        identity: id,
                        inner: m,
                    })
                })
                .collect::<TryJoinAll<_>>()
                .await?,
        })
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn send_message(&mut self, message: String) -> Result<(), Error> {
        Ok(self.cx.send_message(self.target, message).await?)
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn fetch_messages(&mut self, name: &str, cursor: Cursor) -> Result<Messages, Error> {
        let chat = parse_chat_name(name)?;
        let mut c = cursor.inner.get();
        let messages = self.cx.fetch_messages(chat, &mut c).await?;
        cursor.inner.set(c);
        Ok(Messages { list: messages.into_iter().map(Into::into).collect() })
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn next(&mut self) -> Result<ChatEvent, Error> {
        while let Some(event) = self.stream.next().await {
            let event = match event {
                chat_spec::ChatEvent::Message(id, ReminderOwned(message)) => {
                    let msg = "chat was deleted while subscription is active";
                    let secret =
                        self.cx.vault.borrow().chats.get(&self.target).context(msg)?.secret;
                    let message = chain_api::decrypt(message.to_owned(), secret)?;
                    let content = String::from_utf8(message).context("validating message utf8")?;
                    let name =
                        self.cx.keys.chain_client().await?.fetch_username(id).await?.to_string();
                    ChatEvent { message: Some(Message { name, id, content }), ..Default::default() }
                }
                chat_spec::ChatEvent::Member(id, config) => ChatEvent {
                    member: Some(Member {
                        name: self.cx.keys.chain_client().await?.fetch_username(id).await?,
                        identity: id,
                        inner: config,
                    }),
                    ..Default::default()
                },
                chat_spec::ChatEvent::MemberRemoved(id) => {
                    ChatEvent { member_removed: Some(hex::encode(id)), ..Default::default() }
                }
            };

            return Ok(event);
        }

        Err(Error(anyhow::anyhow!("chat subscription closed")))
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
    stream: mpsc::Receiver<MailVariants>,
    cx: Context,
}

#[wasm_bindgen]
impl ProfileSubscription {
    /// @throw
    #[wasm_bindgen]
    pub async fn new(cx: Context) -> Result<ProfileSubscription, Error> {
        let stream =
            cx.profile_subscription().await?.subscribe().await.context("failed to subscribe")?;
        Ok(ProfileSubscription { stream, cx })
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn next(&mut self) -> Result<FriendMessage, Error> {
        let mut messages = Vec::new();
        let mut changes = Vec::new();
        while let Some(mail) = self.stream.next().await {
            mail.handle(&self.cx, &mut changes, &mut messages).await?;

            self.cx.save_vault_components(changes.drain(..)).await?;

            if let Some((username, crate::FriendMessage::DirectMessage { content })) =
                messages.pop()
            {
                return Ok(FriendMessage { sender: username.to_string(), content });
            }
        }

        Err(Error(anyhow::anyhow!("profile subscription closed, reomen it I guess")))
    }
}

#[wasm_bindgen(getter_with_clone)]
struct FriendMessage {
    pub sender: String,
    pub content: String,
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
    name: UserName,
    identity: Identity,
    inner: chat_spec::Member,
}

#[wasm_bindgen]
impl Member {
    #[wasm_bindgen]
    pub fn best(name: &str, identity: &str) -> Result<Member, Error> {
        let name = parse_username(name)?;
        let identity = parse_identity(identity)?;
        Ok(Self { name, identity, inner: chat_spec::Member::best() })
    }

    #[wasm_bindgen]
    pub fn worst(name: &str, identity: &str) -> Result<Member, Error> {
        let name = parse_username(name)?;
        let identity = parse_identity(identity)?;
        Ok(Self { name, identity, inner: chat_spec::Member::worst() })
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
        contains(self.inner.permissions, chat_spec::Permissions::SEND)
    }

    #[wasm_bindgen(setter)]
    pub fn set_can_send(&mut self, can: bool) {
        self.inner.permissions = updated(self.inner.permissions, chat_spec::Permissions::SEND, can);
    }

    #[wasm_bindgen(getter)]
    pub fn can_kick(&self) -> bool {
        contains(self.inner.permissions, chat_spec::Permissions::KICK)
    }

    #[wasm_bindgen(setter)]
    pub fn set_can_kick(&mut self, can: bool) {
        self.inner.permissions = updated(self.inner.permissions, chat_spec::Permissions::KICK, can);
    }

    #[wasm_bindgen(getter)]
    pub fn can_invite(&self) -> bool {
        contains(self.inner.permissions, chat_spec::Permissions::INVITE)
    }

    #[wasm_bindgen(setter)]
    pub fn set_can_invite(&mut self, can: bool) {
        self.inner.permissions =
            updated(self.inner.permissions, chat_spec::Permissions::INVITE, can);
    }
}

fn contains(perms: chat_spec::Permissions, perm: chat_spec::Permissions) -> bool {
    perms & perm != chat_spec::Permissions::empty()
}

fn updated(
    perms: chat_spec::Permissions,
    perm: chat_spec::Permissions,
    can: bool,
) -> chat_spec::Permissions {
    if can {
        perms | perm
    } else {
        perms & !perm
    }
}

#[wasm_bindgen]
pub struct UserKeys {
    inner: crate::UserKeys,
}

#[wasm_bindgen]
impl UserKeys {
    /// @throw
    #[wasm_bindgen]
    pub fn new(username: &str, password: &str) -> Result<UserKeys, Error> {
        let name = parse_username(username)?;
        Ok(UserKeys { inner: crate::UserKeys::new(name, password) })
    }
}

pub struct Error(anyhow::Error);

impl<T: Into<anyhow::Error>> From<T> for Error {
    fn from(e: T) -> Self {
        Self(e.into())
    }
}

impl Into<JsValue> for Error {
    fn into(self) -> JsValue {
        JsValue::from_str(&self.0.to_string())
    }
}

pub fn parse_identity(s: &str) -> Result<Identity, Error> {
    let mut id = Identity::default();
    hex::decode_to_slice(s, &mut id).context("expected hex")?;
    Ok(id)
}

pub fn parse_chat_name(s: &str) -> Result<ChatName, Error> {
    ChatName::from_str(s).context("invalid chat name").map_err(Into::into)
}

pub fn parse_username(s: &str) -> Result<UserName, Error> {
    UserName::from_str(s).context("invalid username").map_err(Into::into)
}
