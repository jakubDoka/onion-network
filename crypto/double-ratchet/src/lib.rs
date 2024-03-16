//use {
//    crypto::{enc, sign::PublicKey, SharedSecret, TransmutationCircle},
//    hkdf::Hkdf,
//    rand::{rngs::OsRng, CryptoRng, RngCore},
//    sha2::Sha256,
//};
//
//pub const RACHET_SEED_SIZE: usize = 64 + 32;
//pub const MAX_KEPT_MISSING_MESSAGES: usize = 10;
//pub const SUB_CONSTANT: SharedSecret = [0u8; 32];
//
//pub type RatchetSeed = [u8; RACHET_SEED_SIZE];
//
//pub struct LimmitedEntropy<'a>(&'a [u8]);
//
//impl RngCore for LimmitedEntropy<'_> {
//    fn next_u32(&mut self) -> u32 {
//        unimplemented!()
//    }
//
//    fn next_u64(&mut self) -> u64 {
//        unimplemented!()
//    }
//
//    fn fill_bytes(&mut self, dest: &mut [u8]) {
//        dest.copy_from_slice(self.0.take(..dest.len()).unwrap());
//    }
//
//    fn try_fill_bytes(&mut self, _: &mut [u8]) -> Result<(), rand::Error> {
//        unimplemented!()
//    }
//}
//impl CryptoRng for LimmitedEntropy<'_> {}
//
//#[derive(Debug, Clone, Copy, Codec, PartialEq, Eq)]
//pub struct MessageHeader {
//    pub root_index: u32,
//    pub sub_index: u32,
//}
//
//#[derive(Debug, Clone, Codec)]
//pub struct NewKeyPayload {
//    pub new_key: enc::PublicKey,
//    pub cp: enc::Ciphertext,
//    pub prev_sub_count: u32,
//}
//
//#[derive(Debug, Clone, Codec)]
//pub struct Message<'a> {
//    pub header: MessageHeader,
//    pub new_key: Option<NewKeyPayload>,
//    pub content: Reminder<'a>,
//}
//
//pub struct DoubleRatchet {
//    index: u32,
//    root_chain: SharedSecret,
//    sender_chain_index: u32,
//    sender_chain: SharedSecret,
//    receiver_chain_index: u32,
//    receiver_chain: SharedSecret,
//    rachet_key_seed: RatchetSeed,
//    prev_sub_count: u32,
//    next_message_cp: Option<enc::Ciphertext>,
//    missing_messages: ArrayVec<(MessageHeader, SharedSecret), MAX_KEPT_MISSING_MESSAGES>,
//}
//
//impl DoubleRatchet {
//    pub fn new_initator() -> Self {
//        let mut rachet_key_seed = [0u8; RACHET_SEED_SIZE];
//        OsRng.fill_bytes(&mut rachet_key_seed);
//
//        Self {
//            index: 0,
//            root_chain: Default::default(),
//            sender_chain_index: 0,
//            sender_chain: Default::default(),
//            receiver_chain_index: 0,
//            receiver_chain: Default::default(),
//            rachet_key_seed,
//            next_message_cp: None,
//            prev_sub_count: 0,
//            missing_messages: ArrayVec::new(),
//        }
//    }
//
//    pub fn recv_message(&mut self, message: Message) -> Option<SharedSecret> {
//        let Some(nk_payload) = message.new_key else {
//            if self.root_chain == SharedSecret::default() {
//                return None;
//            }
//
//            assert_ne!(self.sender_chain, SharedSecret::default());
//            assert_ne!(self.receiver_chain, SharedSecret::default());
//
//            if message.header.root_index != self.index
//                || message.header.sub_index < self.receiver_chain_index
//            {
//                return self
//                    .missing_messages
//                    .iter()
//                    .position(|&(header, _)| header == message.header)
//                    .map(|i| self.missing_messages.swap_remove(i).1);
//            }
//
//            for i in self.receiver_chain_index..message.header.sub_index {
//                Self::add_missing_message(
//                    &mut self.missing_messages,
//                    self.index,
//                    i,
//                    kdf_rc(&mut self.receiver_chain),
//                );
//            }
//
//            self.receiver_chain_index = message.header.sub_index + 1;
//            return Some(kdf_rc(&mut self.receiver_chain));
//        };
//
//        for i in self.receiver_chain_index..nk_payload.prev_sub_count {
//            Self::add_missing_message(
//                &mut self.missing_messages,
//                self.index,
//                i,
//                kdf_rc(&mut self.receiver_chain),
//            );
//        }
//
//        let local_kp = enc::Keypair::new(&mut LimmitedEntropy(&self.rachet_key_seed));
//        let sender_rachet_input =
//            local_kp.decapsulate(enc::Ciphertext::from_ref(&nk_payload.cp)).ok()?;
//        let (new_cp, receiver_rachet_input) =
//            local_kp.encapsulate(enc::PublicKey::from_ref(&nk_payload.new_key), OsRng);
//
//        self.next_message_cp = Some(new_cp);
//        self.prev_sub_count = std::mem::take(&mut self.sender_chain_index) + 1;
//        self.sender_chain = kdf_rk(&mut self.root_chain, sender_rachet_input);
//        self.receiver_chain_index = 0;
//        self.receiver_chain = kdf_rk(&mut self.root_chain, receiver_rachet_input);
//        self.receiver_chain_index = 1;
//        self.index += 1;
//
//        Some(kdf_rc(&mut self.receiver_chain))
//    }
//
//    pub fn add_missing_message(
//        missing_messages: &mut ArrayVec<(MessageHeader, SharedSecret), MAX_KEPT_MISSING_MESSAGES>,
//        root_index: u32,
//        sub_index: u32,
//        key: SharedSecret,
//    ) {
//        if missing_messages.len() >= MAX_KEPT_MISSING_MESSAGES {
//            missing_messages.remove(0);
//        }
//        missing_messages.push((MessageHeader { root_index, sub_index }, key));
//    }
//}
//
//pub fn kdf_rc(root: &mut SharedSecret) -> SharedSecret {
//    let (next_root, message_key) = kdf(*root, SUB_CONSTANT, "reciever chain");
//    *root = next_root;
//    message_key
//}
//
//pub fn kdf_sc(root: &mut SharedSecret) -> SharedSecret {
//    let (next_root, message_key) = kdf(*root, SUB_CONSTANT, "sender chain");
//    *root = next_root;
//    message_key
//}
//
//pub fn kdf_rk(root: &mut SharedSecret, rachet_input: SharedSecret) -> SharedSecret {
//    let (next_root, message_key) = kdf(*root, rachet_input, "root chain");
//    *root = next_root;
//    message_key
//}
//
//pub fn kdf(
//    root: SharedSecret,
//    input: SharedSecret,
//    info: &'static str,
//) -> (SharedSecret, SharedSecret) {
//    let gen = Hkdf::<Sha256>::new(Some(&root), &input);
//    let mut output = [0u8; 64];
//    gen.expand(info.as_bytes(), &mut output).unwrap();
//    unsafe { std::mem::transmute(output) }
//}
