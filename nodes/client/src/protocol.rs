use {
    chat_spec::*,
    component_utils::{Codec, Reminder},
    libp2p::futures::StreamExt,
    onion::EncryptedStream,
    std::convert::identity,
};

type Result<T, E = ChatError> = std::result::Result<T, E>;

pub struct RequestDispatch {
    buffer: Vec<u8>,
    sink: libp2p::futures::channel::mpsc::Sender<RequestInit>,
}

impl Clone for RequestDispatch {
    fn clone(&self) -> Self {
        Self { buffer: Vec::new(), sink: self.sink.clone() }
    }
}

pub fn proof_topic<T>(p: &Proof<T>) -> Topic {
    crypto::hash::from_raw(&p.pk).into()
}

impl RequestDispatch {
    pub fn new() -> (Self, RequestStream) {
        let (sink, stream) = libp2p::futures::channel::mpsc::channel(5);
        (Self { buffer: Vec::new(), sink }, stream)
    }

    async fn dispatch<'a, R: Codec<'a>>(
        &'a mut self,
        prefix: u8,
        topic: impl Into<Option<Topic>>,
        request: impl Codec<'_>,
    ) -> Result<R> {
        let id = CallId::new();
        let (tx, rx) = libp2p::futures::channel::oneshot::channel();
        use libp2p::futures::SinkExt;
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
        self.buffer = rx.await.map_err(|_| ChatError::ChannelClosed)?;
        Result::<R>::decode(&mut &self.buffer[..])
            .ok_or(ChatError::InvalidResponse)
            .and_then(identity)
    }

    pub async fn add_member(
        &mut self,
        state: crate::State,
        name: ChatName,
        member: Identity,
        config: Member,
    ) -> Result<()> {
        let msg = (state.next_chat_proof(name, None).unwrap(), member, config);
        let nonce = match self.dispatch(rpcs::ADD_MEMBER, Topic::Chat(name), msg).await {
            Err(ChatError::InvalidChatAction(nonce)) => nonce,
            e => return e,
        };

        let msg = (state.next_chat_proof(name, Some(nonce)).unwrap(), member, config);
        self.dispatch(rpcs::ADD_MEMBER, Topic::Chat(name), msg).await
    }

    pub async fn send_message<'a>(
        &'a mut self,
        state: crate::State,
        name: ChatName,
        content: &[u8],
    ) -> Result<()> {
        let proof = state.next_chat_message_proof(name, content, None).unwrap();
        let nonce = match self.dispatch(rpcs::SEND_MESSAGE, Topic::Chat(name), proof).await {
            Err(ChatError::InvalidChatAction(nonce)) => nonce,
            e => return e,
        };

        let proof = state.next_chat_message_proof(name, content, Some(nonce)).unwrap();
        self.dispatch(rpcs::SEND_MESSAGE, Topic::Chat(name), proof).await
    }

    pub async fn fetch_messages(
        &mut self,
        name: ChatName,
        cursor: Cursor,
    ) -> Result<(Cursor, Reminder)> {
        self.dispatch(rpcs::FETCH_MESSAGES, Topic::Chat(name), cursor).await
    }

    pub async fn create_chat(&mut self, name: ChatName, me: Identity) -> Result<()> {
        self.dispatch(rpcs::CREATE_CHAT, Topic::Chat(name), me).await
    }

    pub async fn set_vault<'a>(&'a mut self, proof: Proof<Reminder<'_>>) -> Result<()> {
        self.dispatch(rpcs::SET_VAULT, proof_topic(&proof), proof).await
    }

    pub async fn fetch_keys(&mut self, identity: Identity) -> Result<FetchProfileResp> {
        self.dispatch(rpcs::FETCH_PROFILE, Topic::Profile(identity), ()).await
    }

    pub async fn send_mail<'a>(&'a mut self, to: Identity, mail: impl Codec<'_>) -> Result<()> {
        self.dispatch(rpcs::SEND_MAIL, Topic::Profile(to), mail).await.or_else(ChatError::recover)
    }

    pub async fn read_mail(&mut self, proof: Proof<Mail>) -> Result<Reminder> {
        self.dispatch(rpcs::READ_MAIL, proof_topic(&proof), proof).await
    }

    pub async fn dispatch_direct<'a, R: Codec<'a>>(
        &'a mut self,
        stream: &mut EncryptedStream,
        prefix: u8,
        topic: impl Into<Option<Topic>>,
        request: impl Codec<'_>,
    ) -> Result<R> {
        stream
            .write(chat_spec::Request {
                prefix,
                id: CallId::new(),
                topic: topic.into(),
                body: Reminder(&request.to_bytes()),
            })
            .ok_or(ChatError::MessageOverload)?;

        self.buffer = stream
            .next()
            .await
            .ok_or(ChatError::ChannelClosed)?
            .map_err(|_| ChatError::ChannelClosed)?;

        log::info!("Received response: {:?}", self.buffer);

        <(CallId, Result<R>)>::decode(&mut &self.buffer[..])
            .ok_or(ChatError::InvalidResponse)
            .and_then(|(_, r)| r)
    }

    pub async fn subscribe(&mut self, topic: impl Into<Topic>) -> Result<Subscription> {
        let (tx, mut rx) = libp2p::futures::channel::mpsc::channel(0);
        let id = CallId::new();
        let topic: Topic = topic.into();
        self.sink
            .try_send(RequestInit::Subscription(SubscriptionInit { id, topic, channel: tx }))
            .map_err(|_| ChatError::ChannelClosed)?;

        log::info!("Subscribing to {:?}", topic);
        let init = rx.next().await.unwrap();
        Result::<()>::decode(&mut &init[..]).unwrap()?;
        log::info!("Subscribed to {:?}", topic);

        Ok(Subscription { events: rx })
    }
}

pub type RequestStream = libp2p::futures::channel::mpsc::Receiver<RequestInit>;

pub enum RequestInit {
    Request(RawRequest),
    Subscription(SubscriptionInit),
}

impl RequestInit {
    pub fn topic(&self) -> Topic {
        match self {
            Self::Request(r) => r.topic.unwrap(),
            Self::Subscription(s) => s.topic,
        }
    }
}

pub struct Subscription {
    events: libp2p::futures::channel::mpsc::Receiver<SubscriptionMessage>,
}

impl Subscription {
    pub async fn next(&mut self) -> Option<Vec<u8>> {
        self.events.next().await
    }
}

pub struct SubscriptionInit {
    pub id: CallId,
    pub topic: Topic,
    pub channel: libp2p::futures::channel::mpsc::Sender<SubscriptionMessage>,
}

pub type SubscriptionMessage = Vec<u8>;

pub struct RawRequest {
    pub id: CallId,
    pub topic: Option<Topic>,
    pub prefix: u8,
    pub payload: Vec<u8>,
    pub channel: libp2p::futures::channel::oneshot::Sender<RawResponse>,
}

pub type RawResponse = Vec<u8>;
