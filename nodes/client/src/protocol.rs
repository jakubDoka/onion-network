use {
    anyhow::Context, chat_spec::*, component_utils::Codec, libp2p::futures::StreamExt,
    onion::EncryptedStream, std::convert::Infallible,
};

pub struct RequestDispatch {
    buffer: Vec<u8>,
    sink: libp2p::futures::channel::mpsc::Sender<RequestInit>,
}

impl Clone for RequestDispatch {
    fn clone(&self) -> Self {
        Self { buffer: Vec::new(), sink: self.sink.clone() }
    }
}

impl RequestDispatch {
    pub fn new() -> (Self, RequestStream) {
        let (sink, stream) = libp2p::futures::channel::mpsc::channel(5);
        (Self { buffer: Vec::new(), sink }, stream)
    }

    async fn dispatch_low<P: Protocol>(
        &mut self,
        topic: Option<PossibleTopic>,
        request: P::Request<'_>,
    ) -> Result<P::Response<'_>, RequestError<P>> {
        let id = CallId::new();
        let (tx, rx) = libp2p::futures::channel::oneshot::channel();
        use libp2p::futures::SinkExt;
        self.sink
            .send(RequestInit::Request(RawRequest {
                id,
                topic,
                payload: (P::PREFIX, id, request).to_bytes(),
                channel: tx,
            }))
            .await
            .map_err(|_| RequestError::ChannelClosed)?;
        self.buffer = rx.await.map_err(|_| RequestError::ChannelClosed)?;
        Self::parse_response::<P>(&self.buffer)
    }

    pub async fn dispatch<P: Protocol>(
        &mut self,
        request: <Repl<P> as Protocol>::Request<'_>,
    ) -> Result<<Repl<P> as Protocol>::Response<'_>, RequestError<Repl<P>>>
    where
        for<'a> P::Request<'a>: ToPossibleTopic,
    {
        let topic = request.to_possible_topic();
        self.dispatch_low(Some(topic), request).await
    }

    pub async fn dispatch_chat_action(
        &mut self,
        state: crate::State,
        chat: ChatName,
        action: impl Into<ChatAction<'_>>,
    ) -> anyhow::Result<()> {
        let action = action.into();

        let proof = state.next_chat_proof(chat, None).context("extracting chat proof")?;
        let Err(RequestError::Handler(ReplError::Inner(ChatActionError::InvalidAction(
            correct_nonce,
        )))) = self.dispatch::<PerformChatAction>((proof, action)).await
        else {
            return Ok(());
        };

        let proof =
            state.next_chat_proof(chat, Some(correct_nonce)).context("extracting chat proof")?;
        self.dispatch::<PerformChatAction>((proof, action)).await.map_err(Into::into)
    }

    pub async fn dispatch_mail(
        &mut self,
        request: <Repl<SendMail> as Protocol>::Request<'_>,
    ) -> Result<<Repl<SendMail> as Protocol>::Response<'_>, RequestError<Repl<SendMail>>> {
        self.dispatch::<SendMail>(request).await.or_else(|e| match e {
            RequestError::Handler(ReplError::Inner(SendMailError::SentDirectly)) => Ok(()),
            e => Err(e),
        })
    }

    pub async fn dispatch_direct<P: Protocol>(
        &mut self,
        stream: &mut EncryptedStream,
        request: &P::Request<'_>,
    ) -> Result<P::Response<'_>, RequestError<P>> {
        stream
            .write((P::PREFIX, CallId::whatever(), request))
            .ok_or(RequestError::ServerIsOwervhelmed)?;

        self.buffer = stream
            .next()
            .await
            .ok_or(RequestError::ChannelClosed)?
            .map_err(|_| RequestError::ChannelClosed)?;

        Self::parse_response::<P>(&self.buffer)
    }

    pub fn parse_response<P: Protocol>(
        response: &[u8],
    ) -> Result<P::Response<'_>, RequestError<P>> {
        <(CallId, ProtocolResult<'_, P>)>::decode(&mut &response[..])
            .ok_or(RequestError::InvalidResponse)
            .and_then(|(_, resp)| resp.map_err(RequestError::Handler))
    }

    pub fn subscribe<P: Topic>(
        &mut self,
        topic: P,
    ) -> Result<(Subscription<P>, SubsOwner<P>), RequestError<Infallible>> {
        let (tx, rx) = libp2p::futures::channel::mpsc::channel(0);
        let id = CallId::new();
        let topic: PossibleTopic = topic.into();
        self.sink
            .try_send(RequestInit::Subscription(SubscriptionInit {
                id,
                payload: (<Subscribe as Protocol>::PREFIX, id, &topic).to_bytes(),
                topic,
                channel: tx,
            }))
            .map_err(|_| RequestError::ChannelClosed)?;

        Ok((
            Subscription { buffer: Vec::new(), events: rx, phantom: std::marker::PhantomData },
            SubsOwner { id, send_back: self.sink.clone(), phantom: std::marker::PhantomData },
        ))
    }
}

pub struct SubsOwner<H: Topic> {
    id: CallId,
    send_back: libp2p::futures::channel::mpsc::Sender<RequestInit>,
    phantom: std::marker::PhantomData<H>,
}

impl<H: Topic> Clone for SubsOwner<H> {
    fn clone(&self) -> Self {
        Self { id: self.id, send_back: self.send_back.clone(), phantom: std::marker::PhantomData }
    }
}

impl<H: Topic> Drop for SubsOwner<H> {
    fn drop(&mut self) {
        let _ = self.send_back.try_send(RequestInit::CloseSubscription(self.id));
    }
}

pub type RequestStream = libp2p::futures::channel::mpsc::Receiver<RequestInit>;

pub enum RequestInit {
    Request(RawRequest),
    Subscription(SubscriptionInit),
    CloseSubscription(CallId),
}

impl RequestInit {
    pub fn topic(&self) -> PossibleTopic {
        match self {
            Self::Request(r) => r.topic.unwrap(),
            Self::Subscription(s) => s.topic,
            Self::CloseSubscription(_) => unreachable!(),
        }
    }
}

pub struct Subscription<H> {
    buffer: Vec<u8>,
    events: libp2p::futures::channel::mpsc::Receiver<SubscriptionMessage>,
    phantom: std::marker::PhantomData<H>,
}

impl<H: Topic> Subscription<H> {
    pub async fn next(&mut self) -> Option<H::Event<'_>> {
        loop {
            self.buffer = self.events.next().await?;
            let mut slc = &self.buffer[..];
            if Result::<(), Infallible>::decode(&mut slc).is_some() && slc.is_empty() {
                continue;
            }
            return <H::Event<'_> as Codec<'_>>::decode(&mut &self.buffer[..]);
        }
    }
}

pub struct SubscriptionInit {
    pub id: CallId,
    pub topic: PossibleTopic,
    pub payload: Vec<u8>,
    pub channel: libp2p::futures::channel::mpsc::Sender<SubscriptionMessage>,
}

pub type SubscriptionMessage = Vec<u8>;

pub struct RawRequest {
    pub id: CallId,
    pub topic: Option<PossibleTopic>,
    pub payload: Vec<u8>,
    pub channel: libp2p::futures::channel::oneshot::Sender<RawResponse>,
}

pub type RawResponse = Vec<u8>;

pub enum RequestError<H: Protocol> {
    InvalidResponse,
    ChannelClosed,
    ServerIsOwervhelmed,
    Handler(H::Error),
}

impl<H: Protocol> std::fmt::Debug for RequestError<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidResponse => write!(f, "invalid response"),
            Self::ChannelClosed => write!(f, "channel closed"),
            Self::ServerIsOwervhelmed => write!(f, "server is owervhelmed"),
            Self::Handler(e) => write!(f, "handler error: {}", e),
        }
    }
}

impl<H: Protocol> std::fmt::Display for RequestError<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

impl<H: Protocol> std::error::Error for RequestError<H> {}
