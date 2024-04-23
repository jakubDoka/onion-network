use crate::libc;
extern "C" {
    fn rust_memmove(
        _: *mut libc::c_void,
        _: *const libc::c_void,
        _: libc::c_ulong,
    ) -> *mut libc::c_void;
    fn shake256_inc_ctx_release(state: *mut shake256incctx);
    fn PQCLEAN_FALCON512_CLEAN_keygen(
        rng: *mut shake256incctx,
        f: *mut int8_t,
        g: *mut int8_t,
        F: *mut int8_t,
        G: *mut int8_t,
        h: *mut uint16_t,
        logn: libc::c_uint,
        tmp: *mut uint8_t,
    );
    fn shake256_inc_finalize(state: *mut shake256incctx);
    fn shake256_inc_absorb(
        state: *mut shake256incctx,
        input: *const uint8_t,
        inlen: size_t,
    );
    fn shake256_inc_init(state: *mut shake256incctx);
    fn PQCLEAN_FALCON512_CLEAN_modq_encode(
        out: *mut libc::c_void,
        max_out_len: size_t,
        x: *const uint16_t,
        logn: libc::c_uint,
    ) -> size_t;
    static PQCLEAN_FALCON512_CLEAN_max_FG_bits: [uint8_t; 0];
    fn PQCLEAN_FALCON512_CLEAN_trim_i8_encode(
        out: *mut libc::c_void,
        max_out_len: size_t,
        x: *const int8_t,
        logn: libc::c_uint,
        bits: libc::c_uint,
    ) -> size_t;
    static PQCLEAN_FALCON512_CLEAN_max_fg_bits: [uint8_t; 0];
    fn PQCLEAN_FALCON512_CLEAN_comp_encode(
        out: *mut libc::c_void,
        max_out_len: size_t,
        x: *const int16_t,
        logn: libc::c_uint,
    ) -> size_t;
    fn PQCLEAN_FALCON512_CLEAN_sign_dyn(
        sig: *mut int16_t,
        rng: *mut shake256incctx,
        f: *const int8_t,
        g: *const int8_t,
        F: *const int8_t,
        G: *const int8_t,
        hm: *const uint16_t,
        logn: libc::c_uint,
        tmp: *mut uint8_t,
    );
    fn PQCLEAN_FALCON512_CLEAN_hash_to_point_ct(
        sc: *mut shake256incctx,
        x: *mut uint16_t,
        logn: libc::c_uint,
        tmp: *mut uint8_t,
    );
    fn PQCLEAN_FALCON512_CLEAN_complete_private(
        G: *mut int8_t,
        f: *const int8_t,
        g: *const int8_t,
        F: *const int8_t,
        logn: libc::c_uint,
        tmp: *mut uint8_t,
    ) -> libc::c_int;
    fn PQCLEAN_FALCON512_CLEAN_trim_i8_decode(
        x: *mut int8_t,
        logn: libc::c_uint,
        bits: libc::c_uint,
        in_0: *const libc::c_void,
        max_in_len: size_t,
    ) -> size_t;
    fn PQCLEAN_FALCON512_CLEAN_verify_raw(
        c0: *const uint16_t,
        s2: *const int16_t,
        h: *const uint16_t,
        logn: libc::c_uint,
        tmp: *mut uint8_t,
    ) -> libc::c_int;
    fn PQCLEAN_FALCON512_CLEAN_comp_decode(
        x: *mut int16_t,
        logn: libc::c_uint,
        in_0: *const libc::c_void,
        max_in_len: size_t,
    ) -> size_t;
    fn PQCLEAN_FALCON512_CLEAN_to_ntt_monty(h: *mut uint16_t, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_modq_decode(
        x: *mut uint16_t,
        logn: libc::c_uint,
        in_0: *const libc::c_void,
        max_in_len: size_t,
    ) -> size_t;
    fn randombytes(output: *mut uint8_t, n: size_t) -> libc::c_int;
}
pub type size_t = libc::c_ulong;
pub type __int8_t = libc::c_schar;
pub type __uint8_t = libc::c_uchar;
pub type __int16_t = libc::c_short;
pub type __uint16_t = libc::c_ushort;
pub type __u64 = libc::c_ulong;
pub type int8_t = __int8_t;
pub type int16_t = __int16_t;
pub type uint8_t = __uint8_t;
pub type uint16_t = __uint16_t;

#[derive()]
#[repr(C)]
pub struct shake256incctx {
    pub ctx: crate::shake::Ctx,
}
#[derive()]
#[repr(C)]
pub union C2RustUnnamed {
    pub b: [uint8_t; 14336],
    pub dummy_u64: u64,
    pub dummy_fpr: fpr,
}
pub type fpr = u64;
#[derive()]
#[repr(C)]
pub struct C2RustUnnamed_0 {
    pub sig: [int16_t; 512],
    pub hm: [uint16_t; 512],
}
#[derive()]
#[repr(C)]
pub union C2RustUnnamed_1 {
    pub b: [uint8_t; 36864],
    pub dummy_u64: u64,
    pub dummy_fpr: fpr,
}
#[derive()]
#[repr(C)]
pub union C2RustUnnamed_2 {
    pub b: [uint8_t; 1024],
    pub dummy_u64: u64,
    pub dummy_fpr: fpr,
}

pub unsafe fn PQCLEAN_FALCON512_CLEAN_crypto_sign_keypair(mut randombytes: impl FnMut(*mut uint8_t, size_t) -> libc::c_int,
    mut pk: *mut uint8_t,
    mut sk: *mut uint8_t,
) -> libc::c_int {
    let mut tmp: C2RustUnnamed = C2RustUnnamed { b: [0; 14336] };
    let mut f: [int8_t; 512] = [0; 512];
    let mut g: [int8_t; 512] = [0; 512];
    let mut F: [int8_t; 512] = [0; 512];
    let mut h: [uint16_t; 512] = [0; 512];
    let mut seed: [libc::c_uchar; 48] = [0; 48];
    let mut rng: shake256incctx = shake256incctx {
        ctx: crate::shake::Ctx { uninit: (), },
    };
    let mut u: size_t = 0;
    let mut v: size_t = 0;
    randombytes(
        seed.as_mut_ptr(),
        ::core::mem::size_of::<[libc::c_uchar; 48]>() as libc::c_ulong,
    );
    shake256_inc_init(&mut rng);
    shake256_inc_absorb(
        &mut rng,
        seed.as_mut_ptr(),
        ::core::mem::size_of::<[libc::c_uchar; 48]>() as libc::c_ulong,
    );
    shake256_inc_finalize(&mut rng);
    PQCLEAN_FALCON512_CLEAN_keygen(
        &mut rng,
        f.as_mut_ptr(),
        g.as_mut_ptr(),
        F.as_mut_ptr(),
        0 as *mut int8_t,
        h.as_mut_ptr(),
        9 as libc::c_uint,
        (tmp.b).as_mut_ptr(),
    );
    shake256_inc_ctx_release(&mut rng);
    *sk
        .offset(
            0 as isize,
        ) = (0x50 + 9) as uint8_t;
    u = 1 as size_t;
    v = PQCLEAN_FALCON512_CLEAN_trim_i8_encode(
        sk.offset(u as isize) as *mut libc::c_void,
        (1281 as size_t).wrapping_sub(u),
        f.as_mut_ptr(),
        9 as libc::c_uint,
        *PQCLEAN_FALCON512_CLEAN_max_fg_bits.as_ptr().offset(9 as isize)
            as libc::c_uint,
    );
    if v == 0 as size_t {
        return -(1);
    }
    u = u.wrapping_add(v);
    v = PQCLEAN_FALCON512_CLEAN_trim_i8_encode(
        sk.offset(u as isize) as *mut libc::c_void,
        (1281 as size_t).wrapping_sub(u),
        g.as_mut_ptr(),
        9 as libc::c_uint,
        *PQCLEAN_FALCON512_CLEAN_max_fg_bits.as_ptr().offset(9 as isize)
            as libc::c_uint,
    );
    if v == 0 as size_t {
        return -(1);
    }
    u = u.wrapping_add(v);
    v = PQCLEAN_FALCON512_CLEAN_trim_i8_encode(
        sk.offset(u as isize) as *mut libc::c_void,
        (1281 as size_t).wrapping_sub(u),
        F.as_mut_ptr(),
        9 as libc::c_uint,
        *PQCLEAN_FALCON512_CLEAN_max_FG_bits.as_ptr().offset(9 as isize)
            as libc::c_uint,
    );
    if v == 0 as size_t {
        return -(1);
    }
    u = u.wrapping_add(v);
    if u != 1281 as size_t {
        return -(1);
    }
    *pk
        .offset(
            0 as isize,
        ) = (0 + 9) as uint8_t;
    v = PQCLEAN_FALCON512_CLEAN_modq_encode(
        pk.offset(1 as isize) as *mut libc::c_void,
        (897 - 1) as size_t,
        h.as_mut_ptr(),
        9 as libc::c_uint,
    );
    if v != (897 - 1) as size_t {
        return -(1);
    }
    return 0;
}
unsafe extern "C" fn do_sign(mut randombytes: impl FnMut(*mut uint8_t, size_t) -> libc::c_int,
    mut nonce: *mut uint8_t,
    mut sigbuf: *mut uint8_t,
    mut sigbuflen: *mut size_t,
    mut m: *const uint8_t,
    mut mlen: size_t,
    mut sk: *const uint8_t,
) -> libc::c_int {
    let mut tmp: C2RustUnnamed_1 = C2RustUnnamed_1 { b: [0; 36864] };
    let mut f: [int8_t; 512] = [0; 512];
    let mut g: [int8_t; 512] = [0; 512];
    let mut F: [int8_t; 512] = [0; 512];
    let mut G: [int8_t; 512] = [0; 512];
    let mut r: C2RustUnnamed_0 = C2RustUnnamed_0 {
        sig: [0; 512],
        hm: [0; 512],
    };
    let mut seed: [libc::c_uchar; 48] = [0; 48];
    let mut sc: shake256incctx = shake256incctx {
        ctx: crate::shake::Ctx { uninit: (), },
    };
    let mut u: size_t = 0;
    let mut v: size_t = 0;
    if *sk.offset(0 as isize) as libc::c_int
        != 0x50 + 9
    {
        return -(1);
    }
    u = 1 as size_t;
    v = PQCLEAN_FALCON512_CLEAN_trim_i8_decode(
        f.as_mut_ptr(),
        9 as libc::c_uint,
        *PQCLEAN_FALCON512_CLEAN_max_fg_bits.as_ptr().offset(9 as isize)
            as libc::c_uint,
        sk.offset(u as isize) as *const libc::c_void,
        (1281 as size_t).wrapping_sub(u),
    );
    if v == 0 as size_t {
        return -(1);
    }
    u = u.wrapping_add(v);
    v = PQCLEAN_FALCON512_CLEAN_trim_i8_decode(
        g.as_mut_ptr(),
        9 as libc::c_uint,
        *PQCLEAN_FALCON512_CLEAN_max_fg_bits.as_ptr().offset(9 as isize)
            as libc::c_uint,
        sk.offset(u as isize) as *const libc::c_void,
        (1281 as size_t).wrapping_sub(u),
    );
    if v == 0 as size_t {
        return -(1);
    }
    u = u.wrapping_add(v);
    v = PQCLEAN_FALCON512_CLEAN_trim_i8_decode(
        F.as_mut_ptr(),
        9 as libc::c_uint,
        *PQCLEAN_FALCON512_CLEAN_max_FG_bits.as_ptr().offset(9 as isize)
            as libc::c_uint,
        sk.offset(u as isize) as *const libc::c_void,
        (1281 as size_t).wrapping_sub(u),
    );
    if v == 0 as size_t {
        return -(1);
    }
    u = u.wrapping_add(v);
    if u != 1281 as size_t {
        return -(1);
    }
    if PQCLEAN_FALCON512_CLEAN_complete_private(
        G.as_mut_ptr(),
        f.as_mut_ptr(),
        g.as_mut_ptr(),
        F.as_mut_ptr(),
        9 as libc::c_uint,
        (tmp.b).as_mut_ptr(),
    ) == 0
    {
        return -(1);
    }
    randombytes(nonce, 40 as size_t);
    shake256_inc_init(&mut sc);
    shake256_inc_absorb(&mut sc, nonce, 40 as size_t);
    shake256_inc_absorb(&mut sc, m, mlen);
    shake256_inc_finalize(&mut sc);
    PQCLEAN_FALCON512_CLEAN_hash_to_point_ct(
        &mut sc,
        (r.hm).as_mut_ptr(),
        9 as libc::c_uint,
        (tmp.b).as_mut_ptr(),
    );
    shake256_inc_ctx_release(&mut sc);
    randombytes(
        seed.as_mut_ptr(),
        ::core::mem::size_of::<[libc::c_uchar; 48]>() as libc::c_ulong,
    );
    shake256_inc_init(&mut sc);
    shake256_inc_absorb(
        &mut sc,
        seed.as_mut_ptr(),
        ::core::mem::size_of::<[libc::c_uchar; 48]>() as libc::c_ulong,
    );
    shake256_inc_finalize(&mut sc);
    PQCLEAN_FALCON512_CLEAN_sign_dyn(
        (r.sig).as_mut_ptr(),
        &mut sc,
        f.as_mut_ptr(),
        g.as_mut_ptr(),
        F.as_mut_ptr(),
        G.as_mut_ptr(),
        (r.hm).as_mut_ptr(),
        9 as libc::c_uint,
        (tmp.b).as_mut_ptr(),
    );
    v = PQCLEAN_FALCON512_CLEAN_comp_encode(
        sigbuf as *mut libc::c_void,
        *sigbuflen,
        (r.sig).as_mut_ptr(),
        9 as libc::c_uint,
    );
    if v != 0 as size_t {
        shake256_inc_ctx_release(&mut sc);
        *sigbuflen = v;
        return 0;
    }
    return -(1);
}
unsafe extern "C" fn do_verify(
    mut nonce: *const uint8_t,
    mut sigbuf: *const uint8_t,
    mut sigbuflen: size_t,
    mut m: *const uint8_t,
    mut mlen: size_t,
    mut pk: *const uint8_t,
) -> libc::c_int {
    let mut tmp: C2RustUnnamed_2 = C2RustUnnamed_2 { b: [0; 1024] };
    let mut h: [uint16_t; 512] = [0; 512];
    let mut hm: [uint16_t; 512] = [0; 512];
    let mut sig: [int16_t; 512] = [0; 512];
    let mut sc: shake256incctx = shake256incctx {
        ctx: crate::shake::Ctx { uninit: (), },
    };
    let mut v: size_t = 0;
    if *pk.offset(0 as isize) as libc::c_int
        != 0 + 9
    {
        return -(1);
    }
    if PQCLEAN_FALCON512_CLEAN_modq_decode(
        h.as_mut_ptr(),
        9 as libc::c_uint,
        pk.offset(1 as isize) as *const libc::c_void,
        (897 - 1) as size_t,
    ) != (897 - 1) as size_t
    {
        return -(1);
    }
    PQCLEAN_FALCON512_CLEAN_to_ntt_monty(
        h.as_mut_ptr(),
        9 as libc::c_uint,
    );
    if sigbuflen == 0 as size_t {
        return -(1);
    }
    v = PQCLEAN_FALCON512_CLEAN_comp_decode(
        sig.as_mut_ptr(),
        9 as libc::c_uint,
        sigbuf as *const libc::c_void,
        sigbuflen,
    );
    if v == 0 as size_t {
        return -(1);
    }
    if v != sigbuflen {
        if sigbuflen
            == (666 - 40 - 1) as size_t
        {
            while v < sigbuflen {
                let fresh0 = v;
                v = v.wrapping_add(1);
                if *sigbuf.offset(fresh0 as isize) as libc::c_int != 0 {
                    return -(1);
                }
            }
        } else {
            return -(1)
        }
    }
    shake256_inc_init(&mut sc);
    shake256_inc_absorb(&mut sc, nonce, 40 as size_t);
    shake256_inc_absorb(&mut sc, m, mlen);
    shake256_inc_finalize(&mut sc);
    PQCLEAN_FALCON512_CLEAN_hash_to_point_ct(
        &mut sc,
        hm.as_mut_ptr(),
        9 as libc::c_uint,
        (tmp.b).as_mut_ptr(),
    );
    shake256_inc_ctx_release(&mut sc);
    if PQCLEAN_FALCON512_CLEAN_verify_raw(
        hm.as_mut_ptr(),
        sig.as_mut_ptr(),
        h.as_mut_ptr(),
        9 as libc::c_uint,
        (tmp.b).as_mut_ptr(),
    ) == 0
    {
        return -(1);
    }
    return 0;
}

pub unsafe fn PQCLEAN_FALCON512_CLEAN_crypto_sign_signature(mut randombytes: impl FnMut(*mut uint8_t, size_t) -> libc::c_int,
    mut sig: *mut uint8_t,
    mut siglen: *mut size_t,
    mut m: *const uint8_t,
    mut mlen: size_t,
    mut sk: *const uint8_t,
) -> libc::c_int {
    let mut vlen: size_t = 0;
    vlen = (752 - 40 - 1) as size_t;
    if do_sign(randombytes,
        sig.offset(1 as isize),
        sig.offset(1 as isize).offset(40 as isize),
        &mut vlen,
        m,
        mlen,
        sk,
    ) < 0
    {
        return -(1);
    }
    *sig
        .offset(
            0 as isize,
        ) = (0x30 + 9) as uint8_t;
    *siglen = ((1 + 40) as size_t).wrapping_add(vlen);
    return 0;
}

pub unsafe fn PQCLEAN_FALCON512_CLEAN_crypto_sign_verify(
    mut sig: *const uint8_t,
    mut siglen: size_t,
    mut m: *const uint8_t,
    mut mlen: size_t,
    mut pk: *const uint8_t,
) -> libc::c_int {
    if siglen < (1 + 40) as size_t {
        return -(1);
    }
    if *sig.offset(0 as isize) as libc::c_int
        != 0x30 + 9
    {
        return -(1);
    }
    return do_verify(
        sig.offset(1 as isize),
        sig.offset(1 as isize).offset(40 as isize),
        siglen
            .wrapping_sub(1 as size_t)
            .wrapping_sub(40 as size_t),
        m,
        mlen,
        pk,
    );
}

pub unsafe fn PQCLEAN_FALCON512_CLEAN_crypto_sign(mut randombytes: impl FnMut(*mut uint8_t, size_t) -> libc::c_int,
    mut sm: *mut uint8_t,
    mut smlen: *mut size_t,
    mut m: *const uint8_t,
    mut mlen: size_t,
    mut sk: *const uint8_t,
) -> libc::c_int {
    let mut pm: *mut uint8_t = 0 as *mut uint8_t;
    let mut sigbuf: *mut uint8_t = 0 as *mut uint8_t;
    let mut sigbuflen: size_t = 0;
    rust_memmove(
        sm.offset(2 as isize).offset(40 as isize)
            as *mut libc::c_void,
        m as *const libc::c_void,
        mlen,
    );
    pm = sm.offset(2 as isize).offset(40 as isize);
    sigbuf = pm.offset(1 as isize).offset(mlen as isize);
    sigbuflen = (752 - 40 - 3) as size_t;
    if do_sign(randombytes,
        sm.offset(2 as isize),
        sigbuf,
        &mut sigbuflen,
        pm,
        mlen,
        sk,
    ) < 0
    {
        return -(1);
    }
    *pm.offset(mlen as isize) = (0x20 + 9) as uint8_t;
    sigbuflen = sigbuflen.wrapping_add(1);
    sigbuflen;
    *sm.offset(0 as isize) = (sigbuflen >> 8) as uint8_t;
    *sm.offset(1 as isize) = sigbuflen as uint8_t;
    *smlen = mlen
        .wrapping_add(2 as size_t)
        .wrapping_add(40 as size_t)
        .wrapping_add(sigbuflen);
    return 0;
}

pub unsafe fn PQCLEAN_FALCON512_CLEAN_crypto_sign_open(
    mut m: *mut uint8_t,
    mut mlen: *mut size_t,
    mut sm: *const uint8_t,
    mut smlen: size_t,
    mut pk: *const uint8_t,
) -> libc::c_int {
    let mut sigbuf: *const uint8_t = 0 as *const uint8_t;
    let mut pmlen: size_t = 0;
    let mut sigbuflen: size_t = 0;
    if smlen < (3 + 40) as size_t {
        return -(1);
    }
    sigbuflen = (*sm.offset(0 as isize) as size_t) << 8
        | *sm.offset(1 as isize) as size_t;
    if sigbuflen < 2 as size_t
        || sigbuflen
            > smlen
                .wrapping_sub(40 as size_t)
                .wrapping_sub(2 as size_t)
    {
        return -(1);
    }
    sigbuflen = sigbuflen.wrapping_sub(1);
    sigbuflen;
    pmlen = smlen
        .wrapping_sub(40 as size_t)
        .wrapping_sub(3 as size_t)
        .wrapping_sub(sigbuflen);
    if *sm
        .offset(
            ((2 + 40) as size_t).wrapping_add(pmlen)
                as isize,
        ) as libc::c_int != 0x20 + 9
    {
        return -(1);
    }
    sigbuf = sm
        .offset(2 as isize)
        .offset(40 as isize)
        .offset(pmlen as isize)
        .offset(1 as isize);
    if do_verify(
        sm.offset(2 as isize),
        sigbuf,
        sigbuflen,
        sm.offset(2 as isize).offset(40 as isize),
        pmlen,
        pk,
    ) < 0
    {
        return -(1);
    }
    rust_memmove(
        m as *mut libc::c_void,
        sm.offset(2 as isize).offset(40 as isize)
            as *const libc::c_void,
        pmlen,
    );
    *mlen = pmlen;
    return 0;
}
