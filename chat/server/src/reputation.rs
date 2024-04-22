use {
    crate::api::Origin,
    chain_api::NodeIdentity,
    chat_spec::ChatError,
    codec::Codec,
    handlers::FromRequestOwned,
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
        |res| match $crate::reputation::extract_error(res) {
            $(Some($(ChatError::$key)|*) => $value,)*
            _ => -1,
        }
    };

    ($flat_cost:expr) => { |_| $flat_cost };
}

pub fn extract_error(res: &handlers::Response) -> Option<ChatError> {
    match res {
        handlers::Response::Failure(body) => ChatError::decode(&mut body.as_slice()),
        _ => None,
    }
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

impl<'a, C, T, R, H, F> handlers::Handler<'a, C, (Origin, T), R> for Rated<H, F>
where
    C: handlers::Context,
    R: codec::Codec<'a> + Send,
    T: FromRequestOwned<C>,
    H: handlers::Handler<'a, C, T, R>,
    F: Fn(&<H::Future as Future>::Output) -> i64 + Send + Sync + 'static + Clone,
    Origin: FromRequestOwned<C> + Send + 'static,
{
    type Future = impl Future<Output = <H::Future as Future>::Output> + Send;

    fn call_computed(self, (origin, args): (Origin, T), res: R) -> Self::Future {
        async move {
            let res = self.handler.call_computed(args, res).await;
            let severity = (self.rater)(&res);
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

    pub fn report_at_background(us: NodeIdentity) {
        tokio::spawn(async move {
            if let Err(err) = Self::get().report_loop(us).await {
                log::error!("in report loop: {:?}", err);
            }
        });
    }

    async fn report_loop(&'static self, us: NodeIdentity) -> anyhow::Result<!> {
        let client = chain_api::EnvConfig::from_env()?.client().await?;
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
