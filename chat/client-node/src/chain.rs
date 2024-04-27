pub use chain_api::Client;
use {
    chain_api::Profile,
    chat_spec::{username_to_raw, UserName},
};

pub(crate) trait ChainClientExt {
    async fn fetch_profile(&self, name: UserName) -> Result<Profile, anyhow::Error>;
}

impl ChainClientExt for Client {
    async fn fetch_profile(&self, name: UserName) -> Result<Profile, anyhow::Error> {
        match self.get_profile_by_name(username_to_raw(name)).await {
            Ok(Some(u)) => Ok(u),
            Ok(None) => anyhow::bail!("user {name} does not exist"),
            Err(e) => anyhow::bail!("failed to fetch user: {e}"),
        }
    }
}

pub fn min_nodes() -> usize {
    component_utils::build_env!(MIN_NODES);
    MIN_NODES.parse().unwrap()
}
