use {config::EnvError, std::str::FromStr};

#[tokio::main]
async fn main() -> Result<(), EnvError> {
    config::env_config! {
        struct Config {
            /// comma separated list of test wallets to transfere funds to
            test_wallets: config::List<chain_api::AccountId>,
            /// amount to transfere to each wallet
            balance: u128,
            /// comma separated list of chain nodes for redundancy
            chain_nodes: String,
        }
    }

    let Config { test_wallets, balance, chain_nodes } = Config::from_env()?;

    let client = chain_api::Client::with_signer(
        chain_nodes.as_str(),
        chain_api::Keypair::from_uri(&chain_api::SecretUri::from_str("//Bob").unwrap()).unwrap(),
    )
    .await
    .unwrap();

    let nonce = client.get_nonce().await.unwrap();

    for (i, wallet) in test_wallets.0.into_iter().enumerate() {
        eprintln!("Transferring {} to wallet {}", balance, wallet);
        client.transfere(wallet, balance, (i as u64) + nonce).await.unwrap();
    }

    Ok(())
}
