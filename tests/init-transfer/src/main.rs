use config::EnvError;

#[tokio::main]
async fn main() -> Result<(), EnvError> {
    config::env_config! {
        struct Config {
            test_wallets: config::List<chain_api::AccountId>,
            balance: u128,
            chain_nodes: String,
        }
    }

    let Config { test_wallets, balance, chain_nodes } = Config::from_env()?;

    let client =
        chain_api::Client::with_signer(chain_nodes.as_str(), chain_api::dev_keypair("//Bob"))
            .await
            .unwrap();

    let nonce = client.get_nonce().await.unwrap();

    for (i, wallet) in test_wallets.0.into_iter().enumerate() {
        eprintln!("Transferring {} to wallet {}", balance, wallet);
        client.transfere(wallet, balance, (i as u64) + nonce).await.unwrap();
    }

    Ok(())
}
