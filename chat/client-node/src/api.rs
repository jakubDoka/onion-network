#![cfg(feature = "api")]
#![allow(dead_code)]
#![allow(non_snake_case)]

use {
    crate::{requests::MailVariant, ChainClientExt, RawChatMessage, RequestContext},
    anyhow::Context as _,
    chat_spec::{ChatName, Identity, UserName},
    codec::ReminderOwned,
    libp2p::futures::{channel::mpsc, future::TryJoinAll, StreamExt},
    std::{cell::Cell, ops::Deref, rc::Rc, str::FromStr},
    wasm_bindgen_futures::wasm_bindgen::{self, prelude::wasm_bindgen},
    web_sys::wasm_bindgen::JsValue,
};

#[wasm_bindgen]
pub struct Context {
    inner: Rc<crate::TrivialContext>,
}

impl Deref for Context {
    type Target = crate::TrivialContext;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[wasm_bindgen]
impl Context {
    /// @throw
    #[wasm_bindgen]
    pub async fn new(user_keys: UserKeys) -> Result<Context, Error> {
        crate::Storage::set_backend(crate::LocalStorageCache);
        Ok(Self { inner: Rc::new(crate::TrivialContext::new(user_keys.inner).await?) })
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
    stream: mpsc::Receiver<MailVariant>,
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
    /// call this first before calling `next`
    #[wasm_bindgen]
    pub async fn read_mail(&self) -> Result<FriendMessages, Error> {
        let mail = self.cx.read_mail().await?;
        let map = |(username, crate::FriendMessage::DirectMessage { content })| FriendMessage {
            sender: ToString::to_string(&username),
            content,
        };
        Ok(FriendMessages { list: mail.into_iter().map(map).collect() })
    }

    /// @throw
    #[wasm_bindgen]
    pub async fn next(&mut self) -> Result<FriendMessage, Error> {
        while let Some(mail) = self.stream.next().await {
            if let Some((username, crate::FriendMessage::DirectMessage { content })) =
                mail.handle(&*self.cx).await?
            {
                return Ok(FriendMessage { sender: username.to_string(), content });
            }
        }

        Err(Error(anyhow::anyhow!("profile subscription closed, reomen it I guess")))
    }
}

#[wasm_bindgen]
struct FriendMessages {
    #[wasm_bindgen(getter_with_clone)]
    pub list: Vec<FriendMessage>,
}

#[wasm_bindgen(getter_with_clone)]
#[derive(Clone)]
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
    pub fn new(username: &str, password: &str, chain_node: &str) -> Result<UserKeys, Error> {
        let name = parse_username(username)?;
        Ok(UserKeys { inner: crate::UserKeys::new(name, password, chain_node) })
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
