#![cfg_attr(not(feature = "std"), no_std, no_main)]

#[ink::contract]
#[allow(non_local_definitions)]
mod user_manager {
    pub const USER_NAME_CAP: usize = 32;
    pub type RawUserName = [u8; USER_NAME_CAP];
    pub type CryptoHash = [u8; 32];

    #[derive(scale::Decode, scale::Encode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout))]
    pub struct Profile {
        pub sign: CryptoHash,
        pub enc: CryptoHash,
    }

    #[ink(storage)]
    pub struct UserManager {
        username_to_owner: ink::storage::Mapping<RawUserName, AccountId>,
        owner_to_username: ink::storage::Mapping<AccountId, RawUserName>,
        identity_to_username: ink::storage::Mapping<CryptoHash, RawUserName>,
        identities: ink::storage::Mapping<AccountId, Profile>,
    }

    impl Default for UserManager {
        fn default() -> Self {
            Self::new()
        }
    }

    impl UserManager {
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {
                username_to_owner: ink::storage::Mapping::new(),
                identities: ink::storage::Mapping::new(),
                identity_to_username: ink::storage::Mapping::new(),
                owner_to_username: ink::storage::Mapping::new(),
            }
        }

        #[ink(message)]
        pub fn register_with_name(&mut self, name: RawUserName, data: Profile) {
            self.register(data);
            self.pick_name(name);
        }

        #[ink(message)]
        pub fn register(&mut self, data: Profile) {
            self.identities.insert(Self::env().caller(), &data);
        }

        #[ink(message)]
        pub fn pick_name(&mut self, name: RawUserName) {
            assert!(self.owner_to_username.insert(Self::env().caller(), &name).is_none());
            assert!(self.username_to_owner.insert(name, &Self::env().caller()).is_none());
            assert!(self
                .identity_to_username
                .insert(
                    self.identities
                        .get(Self::env().caller())
                        .expect("caller to have identity")
                        .sign,
                    &name
                )
                .is_none());
        }

        #[ink(message)]
        pub fn give_up_name(&mut self, name: RawUserName) {
            assert_eq!(self.username_to_owner.take(name), Some(Self::env().caller()));
            assert_eq!(self.owner_to_username.take(Self::env().caller()), Some(name));
            assert_eq!(
                self.identity_to_username.take(
                    self.identities
                        .get(Self::env().caller())
                        .expect("caller to have identity")
                        .sign
                ),
                Some(name)
            );
        }

        #[ink(message)]
        pub fn transfere_name(&mut self, name: RawUserName, target: AccountId) {
            self.give_up_name(name);
            assert!(self.owner_to_username.insert(target, &name).is_none());
            assert!(self.username_to_owner.insert(name, &target).is_none());
            let identity = self.identities.get(target).expect("target to have identity");
            assert!(self.identity_to_username.insert(identity.sign, &name).is_none());
        }

        #[ink(message)]
        pub fn get_profile(&self, account: AccountId) -> Option<Profile> {
            self.identities.get(account)
        }

        #[ink(message)]
        pub fn get_profile_by_name(&self, name: RawUserName) -> Option<Profile> {
            self.username_to_owner.get(name).and_then(|account| self.identities.get(account))
        }

        #[ink(message)]
        pub fn get_username(&self, identity: CryptoHash) -> Option<RawUserName> {
            self.identity_to_username.get(identity)
        }
    }
}
