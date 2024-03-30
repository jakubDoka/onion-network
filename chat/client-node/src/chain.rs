use {
    crate::requests::Result,
    chain_api::{Profile, TransactionHandler},
    chat_spec::*,
    std::str::FromStr,
    web_sys::wasm_bindgen::{self, JsValue},
};

pub async fn fetch_profile(my_name: UserName, name: UserName) -> Result<Profile, anyhow::Error> {
    let client = chain_node(my_name).await?;
    match client.get_profile_by_name(username_to_raw(name)).await {
        Ok(Some(u)) => Ok(u),
        Ok(None) => anyhow::bail!("user {name} does not exist"),
        Err(e) => anyhow::bail!("failed to fetch user: {e}"),
    }
}

pub async fn chain_node(name: UserName) -> Result<chain_api::Client<WebSigner>, chain_api::Error> {
    component_utils::build_env!(CHAIN_NODES);
    chain_api::Client::with_signer(CHAIN_NODES, WebSigner(name)).await
}

pub fn min_nodes() -> usize {
    component_utils::build_env!(MIN_NODES);
    MIN_NODES.parse().unwrap()
}

async fn sign_with_wallet(payload: &str) -> Result<Vec<u8>, JsValue> {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(catch, js_namespace = integration)]
        async fn sign(data: &str) -> Result<JsValue, JsValue>;
    }

    let sig = sign(payload).await?;
    let sig = sig.as_string().ok_or("user did something very wrong")?;
    let sig = sig.trim_start_matches("0x01");
    hex::decode(sig).map_err(|e| e.to_string().into())
}

async fn get_account_id(name: &str) -> Result<String, JsValue> {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(catch, js_namespace = integration)]
        async fn address(name: &str) -> Result<JsValue, JsValue>;
    }

    let id = address(name).await?;
    id.as_string().ok_or("user, pleas stop").map_err(Into::into)
}

pub struct WebSigner(pub UserName);

impl TransactionHandler for WebSigner {
    async fn account_id_async(&self) -> Result<chain_api::AccountId, chain_api::Error> {
        let id = get_account_id(&self.0)
            .await
            .map_err(|e| chain_api::Error::Other(e.as_string().unwrap_or_default()))?;
        chain_api::AccountId::from_str(&id)
            .map_err(|e| chain_api::Error::Other(format!("invalid id received: {e}")))
    }

    async fn handle(
        &self,
        inner: &chain_api::InnerClient,
        call: impl chain_api::TxPayload,
        nonce: chain_api::Nonce,
    ) -> Result<(), chain_api::Error> {
        let account_id = self.account_id_async().await?;
        let genesis_hash = chain_api::encode_then_hex(&inner.client.genesis_hash());
        let spec_version =
            chain_api::to_hex(inner.client.runtime_version().spec_version.to_be_bytes());
        let transaction_version =
            chain_api::to_hex(inner.client.runtime_version().transaction_version.to_be_bytes());
        let nonce_enc = chain_api::to_hex(nonce.to_be_bytes());
        let mortality_checkpoint = chain_api::encode_then_hex(&inner.client.genesis_hash());
        let era = chain_api::immortal_era();
        let method = chain_api::to_hex(call.encode_call_data(&inner.client.metadata())?);
        let signed_extensions: Vec<String> = inner
            .client
            .metadata()
            .extrinsic()
            .signed_extensions()
            .iter()
            .map(|e| e.identifier().to_string())
            .collect();
        let tip = chain_api::encode_tip(0u128);
        let payload = chain_api::json!({
            "specVersion": spec_version,
            "transactionVersion": transaction_version,
            "address": account_id.to_string(),
            "blockHash": mortality_checkpoint,
            "blockNumber": "0x00000000",
            "era": era,
            "genesisHash": genesis_hash,
            "method": method,
            "nonce": nonce_enc,
            "signedExtensions": signed_extensions,
            "tip": tip,
            "version": 4,
        });

        let signature = sign_with_wallet(&payload.to_string())
            .await
            .map_err(|e| chain_api::Error::Other(e.as_string().unwrap_or_default()))?;

        let signature = signature
            .try_into()
            .map_err(|_| chain_api::Error::Other("signature has invalid size".into()))
            .map(chain_api::new_signature)?;

        let tx = inner.client.tx();

        tx.validate(&call)?;
        let unsigned_payload =
            tx.create_partial_signed_with_nonce(&call, nonce, Default::default())?;

        let progress = unsigned_payload
            .sign_with_address_and_signature(&account_id.into(), &signature.into())
            .submit_and_watch()
            .await?;

        chain_api::wait_for_in_block(progress, true).await.map(drop)
    }
}
