use {chain_api::TransactionHandler, clap::Parser};

#[derive(Parser)]
pub enum Cmd {
    GenMnemonic {
        /// Amount of words in the mnemonic
        length: usize,
    },
    BulkTransfer {
        /// amount to transfere to each account
        balance: u128,
        /// Rpc node to use
        chain_node: String,
        /// Wallet to transfere from (secret uri)
        signer: chain_api::ClapSecretUri,
        /// accounts to transfere to
        destinations: Vec<chain_api::AccountId>,
    },
    AccountIdOf {
        /// secret uri
        secret: chain_api::ClapSecretUri,
    },
}

impl Cmd {
    async fn run(self) -> anyhow::Result<()> {
        match self {
            Cmd::GenMnemonic { length } => {
                println!("{}", chain_api::Mnemonic::generate(length)?)
            }
            Cmd::BulkTransfer { balance, chain_node, signer, destinations } => {
                let client = chain_api::Client::with_signer(
                    &chain_node,
                    chain_api::Keypair::from_uri(&signer.0)?,
                )
                .await?;

                let nonce = client.get_nonce().await?;

                for (i, wallet) in destinations.into_iter().enumerate() {
                    eprintln!("Transferring {} to wallet {}", balance, wallet);
                    client.transfere(wallet, balance, nonce + (i as u64)).await?;
                }
            }
            Cmd::AccountIdOf { secret } => {
                println!("{}", chain_api::Keypair::from_uri(&secret.0)?.account_id_async().await?)
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Cmd::parse().run().await
}
