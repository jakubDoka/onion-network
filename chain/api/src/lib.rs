#![feature(lazy_cell)]

use {
    chain_types::{
        node_staker,
        polkadot::{
            self,
            contracts::calls::types::Call,
            runtime_types::{sp_runtime::DispatchError, sp_weights::weight_v2::Weight},
        },
        runtime_types::pallet_contracts::primitives::{ContractResult, ExecReturnValue},
        user_manager, Hash, InkMessage,
    },
    futures::{StreamExt, TryFutureExt, TryStreamExt},
    parity_scale_codec::{Decode, Encode as _},
    std::str::FromStr,
    subxt::{
        backend::{legacy::LegacyRpcMethods, rpc::RpcClient},
        tx::{Payload, TxProgress},
        OnlineClient, PolkadotConfig,
    },
    subxt_signer::bip39::Mnemonic,
};
pub use {serde_json::json, subxt::tx::TxPayload};

pub const USER_NAME_CAP: usize = 32;

pub type Config = PolkadotConfig;
pub type Balance = u128;
pub type ContractId = <Config as subxt::Config>::AccountId;
pub type AccountId = <Config as subxt::Config>::AccountId;
pub type Error = subxt::Error;
pub type Keypair = subxt_signer::sr25519::Keypair;
pub type Signature = subxt_signer::sr25519::Signature;
pub type CallPayload = Payload<Call>;
pub type NodeAddress = node_staker::NodeAddress;
pub type StakeEvent = node_staker::Event;
pub type Result<T, E = Error> = std::result::Result<T, E>;
pub type Nonce = u64;
pub type RawUserName = [u8; USER_NAME_CAP];
pub type UserIdentity = user_manager::Profile;
pub type NodeIdentity = node_staker::NodeIdentity;
pub type NodeData = node_staker::NodeData;

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

        wait_for_in_block(progress).await.map(drop)
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
    mut progress: TxProgress<PolkadotConfig, OnlineClient<PolkadotConfig>>,
) -> Result<Hash> {
    while let Some(event) = progress.next().await {
        match event? {
            subxt::tx::TxStatus::InBestBlock(b) => {
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
        contract: ContractId,
    ) -> Result<impl futures::Stream<Item = Result<StakeEvent>>, Error> {
        Ok(self
            .inner
            .client
            .blocks()
            .subscribe_finalized()
            .await?
            .then(move |block| {
                let contract = contract.clone();
                async move {
                    Ok::<_, Error>(
                        block?
                            .events()
                            .await?
                            .find::<polkadot::contracts::events::ContractEmitted>()
                            .filter_map(Result::ok)
                            .filter(move |event| event.contract == contract)
                            .map(|event| {
                                StakeEvent::decode(&mut event.data.as_slice()).map_err(|e| {
                                    Error::Decode(subxt::error::DecodeError::custom(e))
                                })
                            })
                            .collect::<Vec<_>>(),
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

    pub async fn join(
        &self,
        dest: ContractId,
        data: NodeData,
        addr: NodeAddress,
        nonce: Nonce,
    ) -> Result<()> {
        self.call_auto_weight(1_000_000, dest, node_staker::messages::join(data, addr), nonce).await
    }

    pub async fn list(&self, addr: ContractId) -> Result<Vec<(NodeData, NodeAddress)>> {
        self.call_dry(0, addr, node_staker::messages::list()).await
    }

    pub async fn vote(
        &self,
        dest: ContractId,
        me: NodeIdentity,
        target: NodeIdentity,
        weight: i32,
        nonce: Nonce,
    ) -> Result<()> {
        let call = node_staker::messages::vote(me, target, weight);
        self.call_auto_weight(0, dest, call, nonce).await
    }

    pub async fn reclaim(&self, dest: ContractId, me: NodeIdentity, nonce: Nonce) -> Result<()> {
        self.call_auto_weight(0, dest, node_staker::messages::reclaim(me), nonce).await
    }

    pub async fn register(
        &self,
        dest: ContractId,
        name: RawUserName,
        data: UserIdentity,
        nonce: Nonce,
    ) -> Result<()> {
        self.call_auto_weight(
            0,
            dest,
            user_manager::messages::register_with_name(name, data),
            nonce,
        )
        .await
    }

    pub async fn get_profile_by_name(
        &self,
        dest: ContractId,
        name: RawUserName,
    ) -> Result<Option<UserIdentity>> {
        let call = user_manager::messages::get_profile_by_name(name);
        self.call_dry(0, dest, call).await
    }

    pub async fn user_exists(&self, dest: ContractId, name: RawUserName) -> Result<bool> {
        self.get_profile_by_name(dest, name).await.map(|p| p.is_some())
    }

    pub async fn get_username(&self, dest: ContractId, id: crypto::Hash) -> Result<RawUserName> {
        let call = user_manager::messages::get_username(id);
        self.call_dry(0, dest, call).await
    }

    async fn call_auto_weight<T: parity_scale_codec::Decode>(
        &self,
        value: Balance,
        dest: ContractId,
        call_data: impl InkMessage,
        nonce: Nonce,
    ) -> Result<T> {
        let (res, mut weight) =
            self.call_dry_low(value, dest.clone(), call_data.to_bytes()).await?;

        weight.ref_time *= 10;
        weight.proof_size *= 10;

        self.signer
            .handle(
                &self.inner,
                polkadot::tx().contracts().call(
                    dest.into(),
                    value,
                    weight,
                    None,
                    call_data.to_bytes(),
                ),
                nonce,
            )
            .await?;

        Ok(res)
    }

    pub async fn get_nonce(&self) -> Result<u64> {
        let id = self.signer.account_id_async().await?;
        self.inner.get_nonce(&id).await
    }

    async fn call_dry<T: parity_scale_codec::Decode>(
        &self,
        value: Balance,
        dest: ContractId,
        call_data: impl InkMessage,
    ) -> Result<T> {
        self.call_dry_low(value, dest, call_data.to_bytes()).await.map(|(t, ..)| t)
    }

    async fn call_dry_low<T: parity_scale_codec::Decode>(
        &self,
        value: Balance,
        dest: ContractId,
        call_data: Vec<u8>,
    ) -> Result<(T, Weight)> {
        let (e, w) = self
            .make_dry_call::<Result<T, ()>>(CallRequest {
                origin: self.signer.account_id_async().await?,
                dest: dest.clone(),
                value,
                gas_limit: None,
                storage_deposit_limit: None,
                input_data: call_data,
            })
            .await?;
        let e = e.map_err(|()| Error::Other("contract returned `Err`".into()))?;
        Ok((e, w))
    }

    async fn make_dry_call<T: parity_scale_codec::Decode>(
        &self,
        call: CallRequest,
    ) -> Result<(T, Weight)> {
        let bytes = call.encode();
        let r = self.inner.legacy.state_call("ContractsApi_call", Some(&bytes), None).await?;

        let r = <ContractResult<Result<ExecReturnValue, DispatchError>, Balance, ()>>::decode(
            &mut r.as_slice(),
        )
        .map_err(|e| Error::Decode(subxt::error::DecodeError::custom(e)))?;

        let res = r.result.map_err(|e| match e {
            DispatchError::Module(me) => {
                let meta = self.inner.client.metadata();
                let pallet = meta.pallet_by_index(me.index);
                let error = pallet.as_ref().and_then(|p| p.error_variant_by_index(me.error[0]));
                Error::Other(format!(
                    "dispatch error: {}.{}",
                    pallet.map_or("unknown", |p| p.name()),
                    error.map_or("unknown", |e| &e.name)
                ))
            }
            e => Error::Other(format!("dispatch error: {e:?}")),
        })?;
        let res = T::decode(&mut res.data.as_slice())
            .map_err(|e| Error::Decode(subxt::error::DecodeError::custom(e)))?;
        Ok((res, r.gas_consumed))
    }
}

/// A struct that encodes RPC parameters required for a call to a smart contract.
///
/// Copied from `pallet-contracts-rpc-runtime-api`.
#[derive(parity_scale_codec::Encode)]
struct CallRequest {
    origin: AccountId,
    dest: AccountId,
    value: Balance,
    gas_limit: Option<Weight>,
    storage_deposit_limit: Option<Balance>,
    input_data: Vec<u8>,
}
