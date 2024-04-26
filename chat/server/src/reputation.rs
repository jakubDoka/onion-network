use {
    crate::api::Origin,
    chain_api::NodeIdentity,
    chat_spec::ChatError,
    libp2p::futures::task::AtomicWaker,
    opfusk::PeerIdExt,
    std::{
        future::{poll_fn, Future},
        sync::{LazyLock, Mutex},
        task::Poll,
    },
};

#[macro_export]
macro_rules! rate_map {
    ($($($key:ident)|* => $value:expr),* $(,)?) => {
        |res| match res {
            $($(ChatError::$key)|* => $value,)*
            _ => -1,
        }
    };

    ($flat_cost:expr) => { |_| $flat_cost };
}

#[derive(Clone)]
pub struct Rated<H, F> {
    handler: H,
    rater: F,
}

impl<H, F> Rated<H, F> {
    pub fn new(handler: H, rater: F) -> Self {
        Self { handler, rater }
    }
}

impl<H, F, C, S, A, B, O> handlers::Handler<C, S, (Origin, A), B> for Rated<H, F>
where
    H: handlers::Handler<C, S, A, B>,
    H::Future: Future<Output = Result<O, ChatError>>,
    S: handlers::Stream,
    A: handlers::FromContext<C>,
    (Origin, A): handlers::FromContext<C>,
    B: handlers::FromStream<S>,
    F: Clone + 'static + Send + Sync + FnOnce(ChatError) -> i64,
{
    type Future = impl Future<Output = <H::Future as Future>::Output> + Send;

    fn call(self, (origin, args): (Origin, A), res: B) -> Self::Future {
        async move {
            let res = self.handler.call(args, res).await;
            let severity = match res {
                Ok(_) => -1,
                Err(e) => (self.rater)(e),
            };
            Rep::get().rate(origin.to_hash(), severity);
            res
        }
    }
}

pub struct Rep {
    ratings: dashmap::DashMap<NodeIdentity, u64>,
    to_punish: Mutex<Vec<NodeIdentity>>,
    waker: AtomicWaker,
}

impl Rep {
    const VOTE_DOWN_TRASHOLD: u64 = 1_000;

    pub fn get() -> &'static Self {
        static REP: LazyLock<Rep> = LazyLock::new(|| Rep {
            ratings: dashmap::DashMap::new(),
            to_punish: Mutex::new(Vec::new()),
            waker: AtomicWaker::new(),
        });
        &REP
    }

    pub fn rate(&self, identity: NodeIdentity, severity: i64) {
        let mut rat = self.ratings.entry(identity).or_default();
        *rat = rat.saturating_add_signed(severity);

        if *rat > Self::VOTE_DOWN_TRASHOLD {
            *rat = 0;
            self.to_punish.lock().unwrap().push(identity);
            self.waker.wake();
        }
    }

    pub fn report_at_background(us: NodeIdentity, chain: chain_api::EnvConfig) {
        tokio::spawn(async move {
            if let Err(err) = Self::get().report_loop(us, chain).await {
                log::error!("in report loop: {:?}", err);
            }
        });
    }

    async fn report_loop(
        &'static self,
        us: NodeIdentity,
        chain: chain_api::EnvConfig,
    ) -> anyhow::Result<!> {
        let client = chain.client().await?;
        loop {
            let identity = poll_fn(|cx| {
                let mut to_punish = self.to_punish.lock().unwrap();
                if let Some(identity) = to_punish.pop() {
                    return Poll::Ready(identity);
                }
                self.waker.register(cx.waker());
                Poll::Pending
            })
            .await;

            if let Err(e) = client.vote_if_possible(us, identity).await {
                log::error!("in vote_if_possible: {:?}", e);
            }
        }
    }

    pub fn vote(&self, source: NodeIdentity, target: NodeIdentity) {
        if self.ratings.get(&source).is_some_and(|r| *r.value() > Self::VOTE_DOWN_TRASHOLD / 2) {
            return;
        }

        if self.ratings.get(&target).is_some_and(|r| *r.value() < Self::VOTE_DOWN_TRASHOLD / 2) {
            return;
        }

        self.rate(target, 100);
    }
}
