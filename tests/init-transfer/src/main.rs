#[tokio::main]
async fn main() {
    config::env_config! {
        struct Config {
            test_wallets: config::List<chain_api::AccountId>,
            balance: u128,
            chain_node: String,
        }
    }

    let Config { test_wallets, balance, chain_node } = Config::from_env();

    let client =
        chain_api::Client::with_signer(chain_node.as_str(), chain_api::dev_keypair("//Bob"))
            .await
            .unwrap();

    for (i, wallet) in test_wallets.0.into_iter().enumerate() {
        client.transfere(wallet, balance, i as _).await.unwrap();
    }
}
