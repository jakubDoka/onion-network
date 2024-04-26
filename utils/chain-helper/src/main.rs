use {chain_api::TransactionHandler, clap::Parser, std::net::SocketAddr};

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
        chain_node: url::Url,
        /// Wallet to transfere from (secret uri)
        signer: chain_api::ClapSecretUri,
        /// accounts to transfere to
        destinations: Vec<chain_api::AccountId>,
    },
    AccountIdOf {
        /// secret uri
        secret: chain_api::ClapSecretUri,
    },
    RegisterNode {
        /// secret uri
        signer: chain_api::ClapSecretUri,
        /// node to register
        node_dientity: chain_api::ClapNodeIdentity,
        /// node enc hash
        node_enc_hash: chain_api::ClapNodeIdentity,
        /// public ip of the node
        addr: SocketAddr,
        /// rpc node to use
        chain_node: url::Url,
        /// custom nonce offset
        #[clap(long, short, env, default_value = "0")]
        nonce: u64,
    },
    ExportIdentity {
        /// mnemonic to derive form
        mnemonic: chain_api::Mnemonic,
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
                    chain_node.as_str(),
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
            Cmd::RegisterNode { signer, chain_node, addr, node_dientity, node_enc_hash, nonce } => {
                let client = chain_api::Client::with_signer(
                    chain_node.as_str(),
                    chain_api::Keypair::from_uri(&signer.0)?,
                )
                .await?;

                if client.joined(node_dientity.0).await? {
                    eprintln!("Node already registered");
                    return Ok(());
                }

                let nonce = client.get_nonce().await? + nonce;
                client.join(node_dientity.0, node_enc_hash.0, addr.into(), nonce).await?;
            }
            Cmd::ExportIdentity { mnemonic } => {
                let keys = chain_api::NodeKeys::from_mnemonic(&mnemonic);
                let identty = chain_api::ClapNodeIdentity(keys.identity());
                let enc_hash = chain_api::ClapNodeIdentity(keys.enc_hash());
                println!("{{ \"sign\": \"{}\", \"enc\": \"{}\" }}", identty, enc_hash);
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Cmd::parse().run().await
}
