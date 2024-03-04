#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
#[allow(non_local_definitions)]
#[allow(clippy::module_name_repetitions)]
mod node_staker {
    use ink::prelude::vec::Vec;

    pub type Ed = [u8; 32];
    pub type CryptoHash = [u8; 32];

    pub const STAKE_AMOUNT: Balance = 1_000_000;
    pub const INIT_VOTE_POOL: u32 = 3;
    pub const STAKE_DURATION_MILIS: Timestamp = 1000 * 60 * 60 * 24 * 30;
    pub const BASE_SLASH: Balance = 2;
    pub const SLASH_FACTOR: u32 = 1;

    #[derive(scale::Decode, scale::Encode, Clone, Copy)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    pub enum NodeAddress {
        Ip4([u8; 4 + 2]),
        Ip6([u8; 16 + 2]),
    }

    #[ink(event)]
    pub struct Joined {
        pub identity: Ed,
        pub addr: NodeAddress,
    }

    #[ink(event)]
    pub struct AddrChanged {
        pub identity: Ed,
        pub addr: NodeAddress,
    }

    #[ink(event)]
    pub struct Reclaimed {
        pub identity: Ed,
    }

    #[derive(scale::Decode, scale::Encode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    struct Votes {
        pool: u32,
        rating: u32,
    }

    impl Default for Votes {
        fn default() -> Self {
            Self { pool: INIT_VOTE_POOL, rating: 0 }
        }
    }

    #[derive(scale::Decode, scale::Encode, Debug, PartialEq, Eq, Hash, Clone, Copy)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    pub struct NodeIdentity {
        pub sign: CryptoHash,
        pub enc: CryptoHash,
    }

    #[derive(scale::Decode, scale::Encode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    pub struct NodeData {
        pub sign: CryptoHash,
        pub enc: CryptoHash,
        pub id: Ed,
    }

    #[derive(scale::Decode, scale::Encode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    struct Stake {
        owner: AccountId,
        amount: Balance,
        created_at: Timestamp,
        votes: Votes,
        id: Ed,
        addr: NodeAddress,
    }

    impl Stake {
        fn apply_slashes(&self) -> u128 {
            if self.votes.rating > 0 {
                self.amount
                    .saturating_sub(BASE_SLASH << u128::from(self.votes.rating * SLASH_FACTOR))
            } else {
                self.amount
            }
        }
    }

    /// Defines the storage of your contract.
    /// Add new fields to the below struct in order
    /// to add new static storage fields to your contract.
    #[ink(storage)]
    pub struct NodeStaker {
        stakes: ink::storage::Mapping<NodeIdentity, Stake>,
        stake_list: Vec<NodeIdentity>,
    }

    impl Default for NodeStaker {
        fn default() -> Self {
            Self::new()
        }
    }

    impl NodeStaker {
        /// Constructor that initializes the `bool` value to the given `init_value`.
        #[ink(constructor)]
        pub fn new() -> Self {
            Self { stakes: ink::storage::Mapping::new(), stake_list: Vec::new() }
        }

        #[ink(message, payable)]
        #[allow(clippy::needless_pass_by_value)]
        pub fn join(&mut self, data: NodeData, addr: NodeAddress) {
            let amount = self.env().transferred_value();
            assert!(amount == STAKE_AMOUNT, "wrong amount");
            let stake = Stake {
                amount,
                owner: Self::env().caller(),
                created_at: Self::env().block_timestamp(),
                votes: Votes::default(),
                id: data.id,
                addr,
            };
            let id = NodeIdentity { sign: data.sign, enc: data.enc };
            assert!(self.stakes.insert(id, &stake).is_none(), "already joined");
            self.stake_list.push(id);

            self.env().emit_event(Joined { identity: data.id, addr });
        }

        #[ink(message)]
        pub fn vote(&mut self, identity: NodeIdentity, target: NodeIdentity, rating: i32) {
            let mut stake = self.stakes.get(identity).expect("no stake to wote with");
            assert!(stake.owner == self.env().caller(), "not owner");
            let mut target_stake = self.stakes.get(target).expect("target does not exist");
            assert!(target_stake.owner != Self::env().caller(), "cannot vote for self");
            stake.votes.pool = stake
                .votes
                .pool
                .checked_sub(rating.unsigned_abs())
                .expect("not enough votes in pool");
            target_stake.votes.rating = target_stake
                .votes
                .rating
                .checked_add_signed(-rating)
                .expect("too many votes casted");
            self.stakes.insert(identity, &stake);
            self.stakes.insert(target, &target_stake);
        }

        #[ink(message)]
        pub fn list(&self) -> Vec<(NodeData, NodeAddress)> {
            self.stake_list
                .iter()
                .map(|id| {
                    let stake = self.stakes.get(id).unwrap();
                    (NodeData { sign: id.sign, enc: id.enc, id: stake.id }, stake.addr)
                })
                .collect()
        }

        pub fn change_addr(&mut self, identity: NodeIdentity, addr: NodeAddress) {
            let mut stake = self.stakes.get(identity).expect("not joined");
            assert!(stake.owner == self.env().caller(), "not owner");
            stake.addr = addr;
            self.stakes.insert(identity, &stake);
            self.env().emit_event(AddrChanged { identity: stake.id, addr });
        }

        #[ink(message)]
        pub fn reclaim(&mut self, identity: NodeIdentity) {
            let stake = self.stakes.get(identity).expect("not joined");
            assert!(stake.owner == self.env().caller(), "not owner");
            assert!(
                stake.created_at + STAKE_DURATION_MILIS <= self.env().block_timestamp(),
                "still locked"
            );
            self.stakes.remove(identity);
            ink::env::debug_println!("current balance: {}", self.env().balance());
            self.env()
                .transfer(Self::env().caller(), stake.apply_slashes())
                .expect("transfer failed");

            self.stake_list
                .iter()
                .position(|&x| x == identity)
                .map(|i| self.stake_list.swap_remove(i));

            self.env().emit_event(Reclaimed { identity: stake.id });
        }
    }

    #[cfg(test)]
    mod tests {
        use {
            super::*,
            ink::{
                env::{test as ink_env, DefaultEnvironment as Env},
                primitives::AccountId,
            },
        };

        fn init_contract() -> NodeStaker {
            ink_env::set_callee::<Env>(ink_env::default_accounts::<Env>().charlie);
            NodeStaker::new()
        }

        fn accounts() -> [AccountId; 2] {
            let accounts = ink_env::default_accounts::<Env>();
            [accounts.alice, accounts.bob]
        }

        const fn identities() -> [NodeIdentity; 2] {
            [NodeIdentity { sign: [0x01; 32], enc: [0x01; 32] }, NodeIdentity {
                sign: [0x02; 32],
                enc: [0x02; 32],
            }]
        }

        fn join(staker: &mut NodeStaker, amount: Balance, identity: NodeIdentity, to: AccountId) {
            ink_env::set_caller::<Env>(to);
            ink_env::set_value_transferred::<Env>(amount);
            ink_env::set_block_timestamp::<Env>(0);
            staker.join(
                NodeData { sign: identity.sign, enc: identity.enc, id: Ed::default() },
                NodeAddress::Ip4([0; 6]),
            );
            ink_env::set_account_balance::<Env>(
                ink_env::callee::<Env>(),
                ink_env::get_account_balance::<Env>(ink_env::callee::<Env>()).unwrap() + amount,
            );
        }

        fn vote(
            staker: &mut NodeStaker,
            identity: NodeIdentity,
            target: NodeIdentity,
            rating: i32,
            to: AccountId,
        ) {
            ink_env::set_caller::<Env>(to);
            staker.vote(identity, target, rating);
        }

        fn reclaim(
            staker: &mut NodeStaker,
            identity: NodeIdentity,
            block_timestamp: Timestamp,
            to: AccountId,
        ) {
            ink_env::set_caller::<Env>(to);
            ink_env::set_block_timestamp::<Env>(block_timestamp);
            staker.reclaim(identity);
        }

        #[ink::test]
        fn tjoin() {
            let mut node_staker = init_contract();
            let [identity, ..] = identities();
            let [alice, ..] = accounts();
            join(&mut node_staker, STAKE_AMOUNT, identity, alice);
            assert_eq!(node_staker.stakes.get(identity).unwrap().owner, alice);
        }

        #[ink::test]
        #[should_panic(expected = "already joined")]
        fn double_join() {
            let mut node_staker = init_contract();
            let [identity, ..] = identities();
            let [alice, ..] = accounts();
            join(&mut node_staker, STAKE_AMOUNT, identity, alice);
            join(&mut node_staker, STAKE_AMOUNT, identity, alice);
        }

        #[ink::test]
        #[should_panic(expected = "wrong amount")]
        fn join_wrong_amount() {
            let mut node_staker = init_contract();
            let [identity, ..] = identities();
            let [alice, ..] = accounts();
            join(&mut node_staker, STAKE_AMOUNT + 1, identity, alice);
        }

        #[ink::test]
        fn tvote() {
            let mut node_staker = init_contract();
            let [identity, target] = identities();
            let [alice, bob] = accounts();
            join(&mut node_staker, STAKE_AMOUNT, identity, alice);
            join(&mut node_staker, STAKE_AMOUNT, target, bob);
            vote(&mut node_staker, identity, target, -1, alice);
            assert_eq!(node_staker.stakes.get(identity).unwrap().votes.pool, INIT_VOTE_POOL - 1);
            assert_eq!(node_staker.stakes.get(target).unwrap().votes.rating, 1);
        }

        #[ink::test]
        #[should_panic(expected = "not enough votes in pool")]
        fn vote_not_enough_votes() {
            let mut node_staker = init_contract();
            let [identity, target] = identities();
            let [alice, bob] = accounts();
            join(&mut node_staker, STAKE_AMOUNT, identity, alice);
            join(&mut node_staker, STAKE_AMOUNT, target, bob);
            vote(
                &mut node_staker,
                identity,
                target,
                -(i32::try_from(INIT_VOTE_POOL).unwrap() + 1),
                alice,
            );
        }

        #[ink::test]
        #[should_panic(expected = "too many votes casted")]
        fn vote_too_many_votes() {
            let mut node_staker = init_contract();
            let [identity, target] = identities();
            let [alice, bob] = accounts();
            join(&mut node_staker, STAKE_AMOUNT, identity, alice);
            join(&mut node_staker, STAKE_AMOUNT, target, bob);
            vote(&mut node_staker, identity, target, 1, alice);
        }

        #[ink::test]
        fn treclaim() {
            let mut node_staker = init_contract();
            let [identity, ..] = identities();
            let [alice, ..] = accounts();
            join(&mut node_staker, STAKE_AMOUNT, identity, alice);
            reclaim(&mut node_staker, identity, STAKE_DURATION_MILIS, alice);
            assert!(node_staker.stakes.get(identity).is_none());
        }

        #[ink::test]
        #[should_panic(expected = "not joined")]
        fn reclaim_not_joined() {
            let mut node_staker = init_contract();
            let [identity, ..] = identities();
            let [alice, ..] = accounts();
            reclaim(&mut node_staker, identity, STAKE_DURATION_MILIS, alice);
        }

        #[ink::test]
        #[should_panic(expected = "not owner")]
        fn reclaim_not_owner() {
            let mut node_staker = init_contract();
            let [identity, ..] = identities();
            let [alice, bob] = accounts();
            join(&mut node_staker, STAKE_AMOUNT, identity, alice);
            reclaim(&mut node_staker, identity, STAKE_DURATION_MILIS, bob);
        }

        #[ink::test]
        #[should_panic(expected = "still locked")]
        fn reclaim_transfer_failed() {
            let mut node_staker = init_contract();
            let [identity, ..] = identities();
            let [alice, ..] = accounts();
            join(&mut node_staker, STAKE_AMOUNT, identity, alice);
            reclaim(&mut node_staker, identity, STAKE_DURATION_MILIS - 1, alice);
        }
    }

    #[cfg(all(test, feature = "e2e-tests"))]
    mod e2e_tests {
        use {super::*, ink_e2e::build_message};

        type E2EResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

        #[ink_e2e::test]
        async fn call_runtime_works(mut client: ink_e2e::Client<C, E>) -> E2EResult<()> {
            // given
            let constructor = NodeStakerRef::new();
            let contract_acc_id = client
                .instantiate("node_staker", &ink_e2e::alice(), constructor, 0, None)
                .await
                .expect("instantiate failed")
                .account_id;

            let get_balance = build_message::<NodeStakerRef>(contract_acc_id.clone())
                .call(|contract| contract.join([0x01; 32]));
            let res =
                client.call_dry_run(&ink_e2e::alice(), &get_balance, STAKE_AMOUNT, None).await;
            res.return_value();

            Ok(())
        }
    }
}
