use {
    anyhow::Context,
    chain_types::{polkadot, Hash},
    codec::Codec,
    crypto::{
        enc,
        rand_core::{OsRng, SeedableRng},
        sign,
    },
    futures::{SinkExt, StreamExt, TryStreamExt},
    rand_chacha::ChaChaRng,
    std::{fs, io, net::IpAddr, str::FromStr},
    subxt::{
        backend::{legacy::LegacyRpcMethods, rpc::RpcClient},
        blocks::Block,
        storage::{address::Yes, StorageAddress},
        tx::TxProgress,
        OnlineClient,
    },
};
pub use {
    chain_types::runtime_types::{
        pallet_node_staker::pallet::{
            Event as ChatStakeEvent, Event2 as SateliteStakeEvent, NodeAddress, NodeData,
            NodeIdentity, Stake as ChatStake, Stake2 as SateliteStake,
        },
        pallet_user_manager::pallet::Profile,
    },
    serde_json::json,
    subxt::{tx::TxPayload, Error, PolkadotConfig as Config},
    subxt_signer::{
        bip39::Mnemonic,
        sr25519::{Keypair, Signature},
    },
};

pub const USER_NAME_CAP: usize = 32;

pub type Balance = u128;
pub type AccountId = <Config as subxt::Config>::AccountId;
pub type Result<T, E = Error> = std::result::Result<T, E>;
pub type Nonce = u64;
pub type RawUserName = [u8; USER_NAME_CAP];
pub type StakeEvents<E> = futures::channel::mpsc::Receiver<Result<E>>;

#[must_use]
pub fn immortal_era() -> String {
    encode_then_hex(&subxt::utils::Era::Immortal)
}

pub fn to_hex(bytes: impl AsRef<[u8]>) -> String {
    format!("0x{}", hex::encode(bytes.as_ref()))
}

pub fn encode_then_hex<E: parity_scale_codec::Encode>(input: &E) -> String {
    format!("0x{}", hex::encode(input.encode()))
}

#[must_use]
pub fn encode_tip(value: u128) -> String {
    encode_then_hex(&parity_scale_codec::Compact(value))
}

#[track_caller]
#[must_use]
pub fn dev_keypair(name: &str) -> Keypair {
    subxt_signer::sr25519::Keypair::from_uri(&subxt_signer::SecretUri::from_str(name).unwrap())
        .unwrap()
}

#[track_caller]
#[must_use]
pub fn mnemonic_keypair(mnemonic: &str) -> Keypair {
    subxt_signer::sr25519::Keypair::from_phrase(&Mnemonic::from_str(mnemonic).unwrap(), None)
        .unwrap()
}

#[must_use]
pub fn new_signature(sig: [u8; 64]) -> Signature {
    subxt_signer::sr25519::Signature(sig)
}

#[allow(async_fn_in_trait)]
pub trait TransactionHandler: Send + Sync + 'static {
    async fn account_id_async(&self) -> Result<AccountId>;
    async fn handle(&self, client: &InnerClient, call: impl TxPayload, nonce: Nonce) -> Result<()>;
}

impl TransactionHandler for Keypair {
    async fn account_id_async(&self) -> Result<AccountId> {
        Ok(self.public_key().into())
    }

    async fn handle(&self, inner: &InnerClient, call: impl TxPayload, nonce: Nonce) -> Result<()> {
        let progress = inner
            .client
            .tx()
            .create_signed_with_nonce(&call, self, nonce, Default::default())?
            .submit_and_watch()
            .await?;

        wait_for_in_block(progress, false).await.map(drop)
    }
}

impl TransactionHandler for () {
    async fn account_id_async(&self) -> Result<AccountId> {
        Ok(AccountId::from([0; 32]))
    }

    async fn handle(
        &self,
        _inner: &InnerClient,
        _call: impl TxPayload,
        _nonce: Nonce,
    ) -> Result<()> {
        Ok(())
    }
}

pub async fn wait_for_in_block(
    mut progress: TxProgress<Config, OnlineClient<Config>>,
    finalized: bool,
) -> Result<Hash> {
    while let Some(event) = progress.next().await {
        match event? {
            subxt::tx::TxStatus::InBestBlock(b) if !finalized => {
                return b.wait_for_success().await.map(|r| r.extrinsic_hash());
            }
            subxt::tx::TxStatus::InFinalizedBlock(b) => {
                return b.wait_for_success().await.map(|r| r.extrinsic_hash());
            }
            subxt::tx::TxStatus::Error { message } => {
                return Err(subxt::Error::Other(format!("tx error (try again): {message}",)));
            }
            subxt::tx::TxStatus::Invalid { message } => {
                return Err(subxt::Error::Other(format!("tx invalid: {message}",)));
            }
            subxt::tx::TxStatus::Dropped { message } => {
                return Err(subxt::Error::Other(
                    format!("tx dropped, maybe try again: {message}",),
                ));
            }
            _ => continue,
        }
    }

    log::error!("tx stream ended without result");
    Err(subxt::Error::Unknown(vec![]))
}

#[derive(Clone)]
pub struct InnerClient {
    pub client: OnlineClient<Config>,
    pub legacy: LegacyRpcMethods<Config>,
}

impl InnerClient {
    pub async fn get_nonce(&self, account: &AccountId) -> Result<u64> {
        let best_block = self
            .legacy
            .chain_get_block_hash(None)
            .await?
            .ok_or(Error::Other("Best block not found".into()))?;
        let account_nonce =
            self.client.blocks().at(best_block).await?.account_nonce(account).await?;
        Ok(account_nonce)
    }
}

#[derive(Clone)]
pub struct Client<S: TransactionHandler> {
    signer: S,
    inner: InnerClient,
}

fn unwrap_chat_staker(e: chain_types::Event) -> Option<ChatStakeEvent> {
    match e {
        chain_types::Event::ChatStaker(e) => Some(e),
        _ => None,
    }
}

fn unwrap_satelite_staker(e: chain_types::Event) -> Option<SateliteStakeEvent> {
    match e {
        chain_types::Event::SateliteStaker(e) => Some(e),
        _ => None,
    }
}

impl<S: TransactionHandler> Client<S> {
    pub async fn chat_event_stream(
        &self,
    ) -> Result<impl futures::Stream<Item = Result<ChatStakeEvent>>> {
        self.node_event_stream("ChatStaker", unwrap_chat_staker).await
    }

    pub async fn satelite_event_stream(
        &self,
    ) -> Result<impl futures::Stream<Item = Result<SateliteStakeEvent>>> {
        self.node_event_stream("SateliteStaker", unwrap_satelite_staker).await
    }

    async fn node_event_stream<E: 'static>(
        &self,
        pallet_name: &'static str,
        unwrap: fn(chain_types::Event) -> Option<E>,
    ) -> Result<impl futures::Stream<Item = Result<E>>, Error> {
        let then = move |block: Result<Block<_, _>>| async move {
            let iter = block?
                .events()
                .await?
                .iter()
                .filter(move |e| e.as_ref().map_or(true, |e| e.pallet_name() == pallet_name))
                .map(|e| e.and_then(|e| e.as_root_event::<chain_types::Event>()))
                .filter_map(move |e| match e {
                    Ok(e) => unwrap(e).map(Ok),
                    Err(e) => Some(Err(e)),
                });
            Ok::<_, Error>(futures::stream::iter(iter))
        };
        let sub = self.inner.client.blocks().subscribe_finalized().await?;
        Ok(sub.then(then).try_flatten())
    }

    pub async fn with_signer(url: &str, account: S) -> Result<Self> {
        let rpc = RpcClient::from_url(url).await?;
        let client = OnlineClient::<Config>::from_rpc_client(rpc.clone()).await?;
        let legacy = LegacyRpcMethods::new(rpc);

        Ok(Self { signer: account, inner: InnerClient { client, legacy } })
    }

    pub async fn transfere(&self, dest: AccountId, amount: Balance, nonce: Nonce) -> Result<()> {
        let transaction = polkadot::tx().balances().transfer_keep_alive(dest.into(), amount);
        self.signer.handle(&self.inner, transaction, nonce).await
    }

    pub async fn join(&self, data: NodeData, addr: NodeAddress, nonce: Nonce) -> Result<()> {
        let tx = chain_types::tx().chat_staker().join(data, addr);
        self.signer.handle(&self.inner, tx, nonce).await
    }

    pub async fn list_chat_nodes(&self) -> Result<Vec<ChatStake>> {
        self.list_nodes(chain_types::storage().chat_staker().stakes_iter()).await
    }

    pub async fn list_satelite_nodes(&self) -> Result<Vec<SateliteStake>> {
        self.list_nodes(chain_types::storage().satelite_staker().stakes_iter()).await
    }

    async fn list_nodes<SA>(&self, tx: SA) -> Result<Vec<SA::Target>>
    where
        SA: StorageAddress<IsIterable = Yes> + 'static,
    {
        let latest = self.inner.client.storage().at_latest().await?;
        latest.iter(tx).await?.map_ok(|(_, v)| v).try_collect().await
    }

    pub async fn vote(
        &self,
        me: NodeIdentity,
        target: NodeIdentity,
        rating: i32,
        nonce: Nonce,
    ) -> Result<()> {
        let tx = chain_types::tx().chat_staker().vote(me, target, rating);
        self.signer.handle(&self.inner, tx, nonce).await
    }

    pub async fn reclaim(&self, me: NodeIdentity, nonce: Nonce) -> Result<()> {
        self.signer.handle(&self.inner, chain_types::tx().chat_staker().reclaim(me), nonce).await
    }

    pub async fn register(&self, name: RawUserName, data: Profile, nonce: Nonce) -> Result<()> {
        let tx = chain_types::tx().user_manager().register_with_name(data, name);
        self.signer.handle(&self.inner, tx, nonce).await
    }

    pub async fn get_profile_by_name(&self, name: RawUserName) -> Result<Option<Profile>> {
        let latest = self.inner.client.storage().at_latest().await?;
        let tx = chain_types::storage().user_manager().username_to_owner(name);
        let Some(account_id) = latest.fetch(&tx).await? else { return Ok(None) };
        latest.fetch(&chain_types::storage().user_manager().identities(account_id)).await
    }

    pub async fn user_exists(&self, name: RawUserName) -> Result<bool> {
        let latest = self.inner.client.storage().at_latest().await?;
        let tx = chain_types::storage().user_manager().username_to_owner(name);
        latest.fetch(&tx).await.map(|o| o.is_some())
    }

    pub async fn get_username(&self, id: crypto::Hash) -> Result<Option<RawUserName>> {
        let latest = self.inner.client.storage().at_latest().await?;
        latest.fetch(&chain_types::storage().user_manager().identity_to_username(id)).await
    }

    pub async fn get_nonce(&self) -> Result<u64> {
        let id = self.signer.account_id_async().await?;
        self.inner.get_nonce(&id).await
    }
}

// TODO: transition to generating keys from mnemonic
#[derive(Clone, Codec)]
pub struct NodeKeys {
    pub enc: enc::Keypair,
    pub sign: sign::Keypair,
}

impl Default for NodeKeys {
    fn default() -> Self {
        Self { enc: enc::Keypair::new(OsRng), sign: sign::Keypair::new(OsRng) }
    }
}

impl NodeKeys {
    pub fn to_stored(&self) -> NodeData {
        NodeData {
            sign: crypto::hash::new(self.sign.public_key()),
            enc: crypto::hash::new(self.enc.public_key()),
            id: self.sign.public_key().pre,
        }
    }

    pub fn from_mnemonic(mnemonic: &Mnemonic) -> Self {
        let seed = crypto::hash::new(mnemonic.to_seed(""));
        let mut rng = ChaChaRng::from_seed(seed);
        Self { enc: enc::Keypair::new(&mut rng), sign: sign::Keypair::new(rng) }
    }

    pub fn load(path: &str) -> io::Result<(Self, bool)> {
        let file = match fs::read(path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let nk = Self::default();
                fs::write(path, nk.to_bytes())?;
                return Ok((nk, true));
            }
            Err(e) => return Err(e),
        };

        let Some(nk) = Self::decode(&mut file.as_slice()) else {
            return Err(io::Error::other("invalid key file"));
        };

        Ok((nk, false))
    }
}

config::env_config! {
    struct ChainConfig {
        exposed_address: IpAddr,
        port: u16,
        nonce: u64,
        chain_nodes: config::List<String>,
        node_account: String,
    }
}

trait Stake {
    fn id(&self) -> sign::Pre;
}

impl Stake for ChatStake {
    fn id(&self) -> sign::Pre {
        self.id
    }
}

impl Stake for SateliteStake {
    fn id(&self) -> sign::Pre {
        self.id
    }
}

impl ChainConfig {
    pub async fn connect_satelite(
        self,
        keys: &NodeKeys,
        register: bool,
    ) -> anyhow::Result<(Vec<SateliteStake>, StakeEvents<SateliteStakeEvent>)> {
        let tx = chain_types::storage().satelite_staker().stakes_iter();
        self.connect(keys, tx, "SateliteStaker", unwrap_satelite_staker, register).await
    }

    pub async fn connect_chat(
        self,
        keys: &NodeKeys,
        register: bool,
    ) -> anyhow::Result<(Vec<ChatStake>, StakeEvents<ChatStakeEvent>)> {
        let tx = chain_types::storage().chat_staker().stakes_iter();
        self.connect(keys, tx, "ChatStaker", unwrap_chat_staker, register).await
    }

    async fn connect<SA, E>(
        self,
        keys: &NodeKeys,
        tx: SA,
        pallet_name: &'static str,
        unwrap: fn(chain_types::Event) -> Option<E>,
        register: bool,
    ) -> anyhow::Result<(Vec<SA::Target>, StakeEvents<E>)>
    where
        SA: StorageAddress<IsIterable = Yes> + 'static + Clone,
        SA::Target: Stake,
        E: 'static + Send,
    {
        let ChainConfig { chain_nodes, node_account, port, exposed_address, nonce } = self;
        let (mut chain_events_tx, stake_events) = futures::channel::mpsc::channel(0);
        let account = if node_account.starts_with("//") {
            dev_keypair(&node_account)
        } else {
            mnemonic_keypair(&node_account)
        };

        let mut others = chain_nodes.0.into_iter();

        let (node_list, client) = 'a: {
            for node in others.by_ref() {
                let Ok(client) = Client::with_signer(&node, account.clone()).await else {
                    continue;
                };
                let Ok(node_list) = client.list_nodes(tx.clone()).await else { continue };
                break 'a (node_list, client);
            }
            anyhow::bail!("failed to fetch node list");
        };

        let mut stream = client.node_event_stream(pallet_name, unwrap).await;
        tokio::spawn(async move {
            loop {
                if let Ok(mut stream) = stream {
                    let mut stream = std::pin::pin!(stream);
                    while let Some(event) = stream.next().await {
                        _ = chain_events_tx.send(event).await;
                    }
                }

                let Some(next) = others.next() else {
                    log::error!("failed to reconnect to chain");
                    std::process::exit(1);
                };

                let Ok(client) = Client::with_signer(&next, account.clone()).await else {
                    stream = Err(Error::Other("failed to reconnect to chain".into()));
                    continue;
                };

                stream = client.node_event_stream(pallet_name, unwrap).await;
            }
        });

        if register && node_list.iter().all(|s| s.id() != keys.sign.public_key().pre) {
            let nonce = client.get_nonce().await.context("fetching nonce")? + nonce;
            client
                .join(keys.to_stored(), (exposed_address, port).into(), nonce)
                .await
                .context("registeing to chain")?;
            log::info!("registered on chain");
        }

        log::info!("entered the network with {} nodes", node_list.len());

        Ok((node_list, stake_events))
    }
}
