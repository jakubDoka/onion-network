use {
    anyhow::Context,
    chain_types::{polkadot, Hash},
    codec::Codec,
    crypto::{
        enc,
        rand_core::{OsRng, SeedableRng},
        sign,
    },
    futures::{SinkExt, StreamExt, TryFutureExt, TryStreamExt},
    rand_chacha::ChaChaRng,
    std::{fs, io, net::IpAddr, str::FromStr},
    subxt::{
        backend::{legacy::LegacyRpcMethods, rpc::RpcClient},
        tx::TxProgress,
        OnlineClient,
    },
};
pub use {
    chain_types::runtime_types::{
        pallet_node_staker::pallet::{
            Event as StakeEvent, NodeAddress, NodeData, NodeIdentity, Stake,
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
pub type StakeEvents = futures::channel::mpsc::Receiver<Result<StakeEvent>>;

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
pub trait TransactionHandler {
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

impl<S: TransactionHandler> Client<S> {
    pub async fn node_contract_event_stream(
        &self,
    ) -> Result<impl futures::Stream<Item = Result<StakeEvent>>, Error> {
        Ok(self
            .inner
            .client
            .blocks()
            .subscribe_finalized()
            .await?
            .then(move |block| {
                async move {
                    Ok::<_, Error>(
                        block?
                            .events()
                            .await?
                            .iter()
                            .filter(|e| e.as_ref().map_or(true, |e| e.pallet_name() == "Staker"))
                            .map(|e| e.and_then(|e| e.as_root_event::<chain_types::Event>()))
                            .filter_map(|e| match e {
                                Ok(chain_types::Event::Staker(e)) => Some(Ok(e)),
                                Ok(_) => None,
                                Err(e) => Some(Err(e)),
                            }),
                    )
                }
                .map_ok(futures::stream::iter)
            })
            .try_flatten())
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
        self.signer.handle(&self.inner, chain_types::tx().staker().join(data, addr), nonce).await
    }

    pub async fn list_nodes(&self) -> Result<Vec<Stake>> {
        self.inner
            .client
            .storage()
            .at_latest()
            .await?
            .iter(chain_types::storage().staker().stakes_iter())
            .await?
            .map_ok(|(_, v)| v)
            .try_collect()
            .await
    }

    pub async fn vote(
        &self,
        me: NodeIdentity,
        target: NodeIdentity,
        rating: i32,
        nonce: Nonce,
    ) -> Result<()> {
        self.signer
            .handle(&self.inner, chain_types::tx().staker().vote(me, target, rating), nonce)
            .await
    }

    pub async fn reclaim(&self, me: NodeIdentity, nonce: Nonce) -> Result<()> {
        self.signer.handle(&self.inner, chain_types::tx().staker().reclaim(me), nonce).await
    }

    pub async fn register(&self, name: RawUserName, data: Profile, nonce: Nonce) -> Result<()> {
        self.signer
            .handle(
                &self.inner,
                chain_types::tx().user_manager().register_with_name(data, name),
                nonce,
            )
            .await
    }

    pub async fn get_profile_by_name(&self, name: RawUserName) -> Result<Option<Profile>> {
        let latest = self.inner.client.storage().at_latest().await?;
        let Some(account_id) =
            latest.fetch(&chain_types::storage().user_manager().username_to_owner(name)).await?
        else {
            return Ok(None);
        };
        latest.fetch(&chain_types::storage().user_manager().identities(account_id)).await
    }

    pub async fn user_exists(&self, name: RawUserName) -> Result<bool> {
        let latest = self.inner.client.storage().at_latest().await?;
        latest
            .fetch(&chain_types::storage().user_manager().username_to_owner(name))
            .await
            .map(|o| o.is_some())
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

impl ChainConfig {
    pub async fn connect(self, keys: &NodeKeys) -> anyhow::Result<(Vec<Stake>, StakeEvents)> {
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
                let Ok(node_list) = client.list_nodes().await else { continue };
                break 'a (node_list, client);
            }
            anyhow::bail!("failed to fetch node list");
        };

        let mut stream = client.node_contract_event_stream().await;
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

                stream = client.node_contract_event_stream().await;
            }
        });

        if node_list.iter().all(|s| s.id != keys.sign.public_key().pre) {
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
