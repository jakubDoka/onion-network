use crate::params::{
    CIPHERTEXTBYTES, INDCPA_SECRETKEYBYTES, PUBLICKEYBYTES, SECRETKEYBYTES, SYMBYTES,
};

pub fn keypair_derand(coins: &[u8; SYMBYTES * 2]) -> [u8; SECRETKEYBYTES] {
    let (pk, sk) = crate::indcpa::keypair_derand(coins[..SYMBYTES].try_into().unwrap());
    let pk_hash = crate::symetric::hash_h(&pk);
    let z: [_; SYMBYTES] = coins[SYMBYTES..].try_into().unwrap();
    unsafe { core::mem::transmute((sk, pk, pk_hash, z)) }
}

pub fn enc_derand(
    pk: [u8; PUBLICKEYBYTES],
    coins: &[u8; SYMBYTES],
) -> ([u8; CIPHERTEXTBYTES], [u8; SYMBYTES]) {
    let h = crate::symetric::hash_h(&pk);
    let buf: [_; SYMBYTES * 2] = unsafe { core::mem::transmute((*coins, h)) };
    let kr = crate::symetric::hash_g(&buf);
    let ct = crate::indcpa::enc(coins, pk, kr[SYMBYTES..].try_into().unwrap());
    let ss = kr[..SYMBYTES].try_into().unwrap();
    (ct, ss)
}

pub fn dec(ct: [u8; CIPHERTEXTBYTES], sk: [u8; SECRETKEYBYTES]) -> Option<[u8; SYMBYTES]> {
    let (sk, pk, pk_hash, _) = unsafe {
        core::mem::transmute::<
            _,
            ([u8; INDCPA_SECRETKEYBYTES], [u8; PUBLICKEYBYTES], [u8; SYMBYTES], [u8; SYMBYTES]),
        >(sk)
    };

    let msg = crate::indcpa::dec(ct, sk);
    let buf: [_; SYMBYTES * 2] = unsafe { core::mem::transmute((msg, pk_hash)) };
    let kr = crate::symetric::hash_g(&buf);

    let cyphertext = crate::indcpa::enc(&msg, pk, kr[SYMBYTES..].try_into().unwrap());
    let ss = kr[..SYMBYTES].try_into().unwrap();

    if crate::verify::verify(&ct, &cyphertext) {
        Some(ss)
    } else {
        None
    }
}
