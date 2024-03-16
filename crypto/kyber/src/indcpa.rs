use {
    crate::{
        params::{
            INDCPA_BYTES, INDCPA_MSGBYTES, INDCPA_PUBLICKEYBYTES, INDCPA_SECRETKEYBYTES, K, N,
            POLYCOMPRESSEDBYTES, POLYVECBYTES, POLYVECCOMPRESSEDBYTES, Q, SYMBYTES,
        },
        poly::Poly,
        polyvec::Polyvec,
    },
    core::array,
    sha3::digest::{crypto_common::BlockSizeUser, typenum::Unsigned, ExtendableOutput, XofReader},
};

pub const XOF_BLOCKBYTES: usize = <<sha3::Shake128 as BlockSizeUser>::BlockSize as Unsigned>::USIZE;
pub const GEN_MATRIX_NBLOCKS: usize =
    (12 * N / 8 * (1 << 12) / Q + XOF_BLOCKBYTES) / XOF_BLOCKBYTES;

pub fn pack_pk(polyvec: &Polyvec, seed: [u8; SYMBYTES]) -> [u8; INDCPA_PUBLICKEYBYTES] {
    unsafe { core::mem::transmute((crate::polyvec::to_bytes(polyvec), seed)) }
}

pub fn unpack_pk(pk: [u8; INDCPA_PUBLICKEYBYTES]) -> (Polyvec, [u8; SYMBYTES]) {
    let (polyvec, seed) =
        unsafe { core::mem::transmute::<_, ([u8; POLYVECBYTES], [u8; SYMBYTES])>(pk) };
    (crate::polyvec::from_bytes(&polyvec), seed)
}

pub fn pack_sk(sk: Polyvec) -> [u8; INDCPA_SECRETKEYBYTES] {
    crate::polyvec::to_bytes(&sk)
}

pub fn unpack_sk(sk: [u8; INDCPA_SECRETKEYBYTES]) -> Polyvec {
    crate::polyvec::from_bytes(&sk)
}

pub fn pack_ciphertext(b: Polyvec, v: Poly) -> [u8; INDCPA_BYTES] {
    unsafe { core::mem::transmute((crate::polyvec::compress(&b), crate::poly::compress(&v))) }
}

pub fn unpack_ciphertext(c: [u8; INDCPA_BYTES]) -> (Polyvec, Poly) {
    let (b, v) = unsafe {
        core::mem::transmute::<_, ([u8; POLYVECCOMPRESSEDBYTES], [u8; POLYCOMPRESSEDBYTES])>(c)
    };
    (crate::polyvec::decompress(&b), crate::poly::decompress(&v))
}

pub fn rej_uniform(r: &mut [i16], buf: &[u8]) -> usize {
    let mut ctr = r.iter_mut();
    for &[a, b, c] in buf.array_chunks::<3>() {
        let val0 = (u16::from(a) | (u16::from(b) << 8)) & 0xfff;
        let val1 = ((u16::from(b) >> 4) | (u16::from(c) << 4)) & 0xfff;

        if val0 < Q as u16 {
            if let Some(r) = ctr.next() {
                *r = val0 as i16;
            } else {
                break;
            }
        }
        if val1 < Q as u16 {
            if let Some(r) = ctr.next() {
                *r = val1 as i16;
            } else {
                break;
            }
        }
    }

    let remining = ctr.into_slice().len();
    r.len() - remining
}

pub fn gen_matrix(seed: &[u8; SYMBYTES], transposed: bool) -> [Polyvec; K] {
    let mut output = [[[0i16; N]; K]; K];
    for (i, a) in output.iter_mut().enumerate() {
        for (j, a) in a.iter_mut().enumerate() {
            let mut state: sha3::Shake128 = Default::default();
            if transposed {
                crate::symetric::xof_absorb(&mut state, seed, i as u8, j as u8);
            } else {
                crate::symetric::xof_absorb(&mut state, seed, j as u8, i as u8);
            }
            let mut xof_reader = state.finalize_xof();
            let mut buf = [0u8; GEN_MATRIX_NBLOCKS * XOF_BLOCKBYTES + 2];
            let mut buflen = GEN_MATRIX_NBLOCKS * XOF_BLOCKBYTES;
            xof_reader.read(&mut buf[..buflen]);
            let mut ctr = crate::indcpa::rej_uniform(&mut a[..], &buf[..buflen]);
            while ctr < N {
                let off = buflen % 3;
                for k in 0..off {
                    buf[k] = buf[buflen - off + k];
                }
                xof_reader.read(&mut buf[off..=off]);
                buflen = off + XOF_BLOCKBYTES;
                ctr += crate::indcpa::rej_uniform(&mut a[ctr..], &buf[..buflen]);
            }
        }
    }
    output
}

pub fn keypair_derand(
    coins: &[u8; SYMBYTES],
) -> ([u8; INDCPA_PUBLICKEYBYTES], [u8; INDCPA_SECRETKEYBYTES]) {
    let buf = crate::symetric::hash_g(coins);
    let pubicseed = buf[..SYMBYTES].try_into().unwrap();
    let noiseseed = buf[SYMBYTES..].try_into().unwrap();
    let a = gen_matrix(pubicseed, false);

    let mut skpv = array::from_fn(|i| crate::poly::getnoise_eta1(noiseseed, i as u8));
    let mut e = array::from_fn(|i| crate::poly::getnoise_eta1(noiseseed, (i + K) as u8));

    crate::polyvec::ntt(&mut skpv);
    crate::polyvec::ntt(&mut e);

    let mut pkpv = a.map(|a| crate::polyvec::basemul_acc_montgomery(&a, &skpv));
    pkpv.iter_mut().for_each(crate::poly::tomont);

    pkpv = crate::polyvec::add(pkpv, e);
    crate::polyvec::reduce(&mut pkpv);

    (crate::indcpa::pack_pk(&pkpv, *pubicseed), crate::indcpa::pack_sk(skpv))
}

pub fn enc(
    m: &[u8; INDCPA_MSGBYTES],
    pk: [u8; INDCPA_PUBLICKEYBYTES],
    coins: &[u8; SYMBYTES],
) -> [u8; INDCPA_BYTES] {
    let (pkpv, seed) = crate::indcpa::unpack_pk(pk);
    let k = crate::poly::from_msg(m);
    let at = gen_matrix(&seed, true);

    let mut sp: Polyvec = array::from_fn(|i| crate::poly::getnoise_eta1(coins, i as u8));
    let ep: Polyvec = array::from_fn(|i| crate::poly::getnoise_eta2(coins, (i + K) as u8));
    let epp: Poly = crate::poly::getnoise_eta2(coins, (K + K) as u8);

    crate::polyvec::ntt(&mut sp);

    let mut b = at.map(|a| crate::polyvec::basemul_acc_montgomery(&a, &sp));

    let mut v = crate::polyvec::basemul_acc_montgomery(&pkpv, &sp);

    crate::polyvec::invntt_tomont(&mut b);
    crate::poly::invntt_tomont(&mut v);

    b = crate::polyvec::add(b, ep);
    v = crate::poly::add(v, epp);
    v = crate::poly::add(v, k);
    crate::polyvec::reduce(&mut b);
    crate::poly::reduce(&mut v);

    crate::indcpa::pack_ciphertext(b, v)
}

pub fn dec(ct: [u8; INDCPA_BYTES], sk: [u8; INDCPA_SECRETKEYBYTES]) -> [u8; INDCPA_MSGBYTES] {
    let skpv = crate::indcpa::unpack_sk(sk);
    let (mut b, v) = crate::indcpa::unpack_ciphertext(ct);

    crate::polyvec::ntt(&mut b);
    let mut mp = crate::polyvec::basemul_acc_montgomery(&skpv, &b);
    crate::poly::invntt_tomont(&mut mp);

    mp = crate::poly::sub(v, mp);
    crate::poly::reduce(&mut mp);

    crate::poly::to_msg(&mp)
}
