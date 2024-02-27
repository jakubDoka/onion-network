use crate::libc;
extern "C" {
    fn rust_memcpy(
        _: *mut libc::c_void,
        _: *const libc::c_void,
        _: libc::c_ulong,
    ) -> *mut libc::c_void;
    fn shake256_inc_squeeze(output: *mut uint8_t, outlen: size_t, state: *mut shake256incctx);
}
pub type __uint8_t = libc::c_uchar;
pub type __u32 = libc::c_uint;
pub type __u64 = libc::c_ulong;
pub type uint8_t = __uint8_t;

pub type size_t = libc::c_ulong;
#[derive()]
#[repr(C)]
pub struct shake256incctx {
    pub ctx: crate::shake::Ctx,
}
#[derive()]
#[repr(C)]
pub struct prng {
    pub buf: C2RustUnnamed_0,
    pub ptr: size_t,
    pub state: C2RustUnnamed,
    pub type_0: libc::c_int,
}
#[derive()]
#[repr(C)]
pub union C2RustUnnamed {
    pub d: [uint8_t; 256],
    pub dummy_u64: u64,
}
#[derive()]
#[repr(C)]
pub union C2RustUnnamed_0 {
    pub d: [uint8_t; 512],
    pub dummy_u64: u64,
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_prng_init(
    mut p: *mut prng,
    mut src: *mut shake256incctx,
) {
    let mut tmp: [uint8_t; 56] = [0; 56];
    let mut th: u64 = 0;
    let mut tl: u64 = 0;
    let mut i: libc::c_int = 0;
    let mut d32: *mut u32 = ((*p).state.d).as_mut_ptr() as *mut u32;
    let mut d64: *mut u64 = ((*p).state.d).as_mut_ptr() as *mut u64;
    shake256_inc_squeeze(tmp.as_mut_ptr(), 56 as size_t, src);
    i = 0;
    while i < 14 {
        let mut w: u32 = 0;
        w = tmp[((i << 2) + 0) as usize] as u32
            | (tmp[((i << 2) + 1) as usize] as u32) << 8
            | (tmp[((i << 2) + 2) as usize] as u32) << 16
            | (tmp[((i << 2) + 3) as usize] as u32) << 24;
        *d32.offset(i as isize) = w;
        i += 1;
        i;
    }
    tl = *d32
        .offset((48 as libc::c_ulong).wrapping_div(::core::mem::size_of::<u32>() as libc::c_ulong)
            as isize) as u64;
    th = *d32
        .offset((52 as libc::c_ulong).wrapping_div(::core::mem::size_of::<u32>() as libc::c_ulong)
            as isize) as u64;
    *d64.offset(
        (48 as libc::c_ulong).wrapping_div(::core::mem::size_of::<u64>() as libc::c_ulong) as isize,
    ) = tl.wrapping_add(th << 32);
    PQCLEAN_FALCON512_CLEAN_prng_refill(p);
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_prng_refill(mut p: *mut prng) {
    static mut CW: [u32; 4] =
        [0x61707865 as u32, 0x3320646e as u32, 0x79622d32 as u32, 0x6b206574 as u32];
    let mut cc: u64 = 0;
    let mut u: size_t = 0;
    cc = *(((*p).state.d).as_mut_ptr().offset(48 as isize) as *mut u64);
    u = 0 as size_t;
    while u < 8 as size_t {
        let mut state: [u32; 16] = [0; 16];
        let mut v: size_t = 0;
        let mut i: libc::c_int = 0;
        rust_memcpy(
            &mut *state.as_mut_ptr().offset(0 as isize) as *mut u32 as *mut libc::c_void,
            CW.as_ptr() as *const libc::c_void,
            ::core::mem::size_of::<[u32; 4]>() as libc::c_ulong,
        );
        rust_memcpy(
            &mut *state.as_mut_ptr().offset(4 as isize) as *mut u32 as *mut libc::c_void,
            ((*p).state.d).as_mut_ptr() as *const libc::c_void,
            48 as libc::c_ulong,
        );
        state[14 as usize] ^= cc as u32;
        state[15 as usize] ^= (cc >> 32) as u32;
        i = 0;
        while i < 10 {
            state[0 as usize] = (state[0 as usize]).wrapping_add(state[4 as usize]);
            state[12 as usize] ^= state[0 as usize];
            state[12 as usize] = state[12 as usize] << 16 | state[12 as usize] >> 16;
            state[8 as usize] = (state[8 as usize]).wrapping_add(state[12 as usize]);
            state[4 as usize] ^= state[8 as usize];
            state[4 as usize] = state[4 as usize] << 12 | state[4 as usize] >> 20;
            state[0 as usize] = (state[0 as usize]).wrapping_add(state[4 as usize]);
            state[12 as usize] ^= state[0 as usize];
            state[12 as usize] = state[12 as usize] << 8 | state[12 as usize] >> 24;
            state[8 as usize] = (state[8 as usize]).wrapping_add(state[12 as usize]);
            state[4 as usize] ^= state[8 as usize];
            state[4 as usize] = state[4 as usize] << 7 | state[4 as usize] >> 25;
            state[1 as usize] = (state[1 as usize]).wrapping_add(state[5 as usize]);
            state[13 as usize] ^= state[1 as usize];
            state[13 as usize] = state[13 as usize] << 16 | state[13 as usize] >> 16;
            state[9 as usize] = (state[9 as usize]).wrapping_add(state[13 as usize]);
            state[5 as usize] ^= state[9 as usize];
            state[5 as usize] = state[5 as usize] << 12 | state[5 as usize] >> 20;
            state[1 as usize] = (state[1 as usize]).wrapping_add(state[5 as usize]);
            state[13 as usize] ^= state[1 as usize];
            state[13 as usize] = state[13 as usize] << 8 | state[13 as usize] >> 24;
            state[9 as usize] = (state[9 as usize]).wrapping_add(state[13 as usize]);
            state[5 as usize] ^= state[9 as usize];
            state[5 as usize] = state[5 as usize] << 7 | state[5 as usize] >> 25;
            state[2 as usize] = (state[2 as usize]).wrapping_add(state[6 as usize]);
            state[14 as usize] ^= state[2 as usize];
            state[14 as usize] = state[14 as usize] << 16 | state[14 as usize] >> 16;
            state[10 as usize] = (state[10 as usize]).wrapping_add(state[14 as usize]);
            state[6 as usize] ^= state[10 as usize];
            state[6 as usize] = state[6 as usize] << 12 | state[6 as usize] >> 20;
            state[2 as usize] = (state[2 as usize]).wrapping_add(state[6 as usize]);
            state[14 as usize] ^= state[2 as usize];
            state[14 as usize] = state[14 as usize] << 8 | state[14 as usize] >> 24;
            state[10 as usize] = (state[10 as usize]).wrapping_add(state[14 as usize]);
            state[6 as usize] ^= state[10 as usize];
            state[6 as usize] = state[6 as usize] << 7 | state[6 as usize] >> 25;
            state[3 as usize] = (state[3 as usize]).wrapping_add(state[7 as usize]);
            state[15 as usize] ^= state[3 as usize];
            state[15 as usize] = state[15 as usize] << 16 | state[15 as usize] >> 16;
            state[11 as usize] = (state[11 as usize]).wrapping_add(state[15 as usize]);
            state[7 as usize] ^= state[11 as usize];
            state[7 as usize] = state[7 as usize] << 12 | state[7 as usize] >> 20;
            state[3 as usize] = (state[3 as usize]).wrapping_add(state[7 as usize]);
            state[15 as usize] ^= state[3 as usize];
            state[15 as usize] = state[15 as usize] << 8 | state[15 as usize] >> 24;
            state[11 as usize] = (state[11 as usize]).wrapping_add(state[15 as usize]);
            state[7 as usize] ^= state[11 as usize];
            state[7 as usize] = state[7 as usize] << 7 | state[7 as usize] >> 25;
            state[0 as usize] = (state[0 as usize]).wrapping_add(state[5 as usize]);
            state[15 as usize] ^= state[0 as usize];
            state[15 as usize] = state[15 as usize] << 16 | state[15 as usize] >> 16;
            state[10 as usize] = (state[10 as usize]).wrapping_add(state[15 as usize]);
            state[5 as usize] ^= state[10 as usize];
            state[5 as usize] = state[5 as usize] << 12 | state[5 as usize] >> 20;
            state[0 as usize] = (state[0 as usize]).wrapping_add(state[5 as usize]);
            state[15 as usize] ^= state[0 as usize];
            state[15 as usize] = state[15 as usize] << 8 | state[15 as usize] >> 24;
            state[10 as usize] = (state[10 as usize]).wrapping_add(state[15 as usize]);
            state[5 as usize] ^= state[10 as usize];
            state[5 as usize] = state[5 as usize] << 7 | state[5 as usize] >> 25;
            state[1 as usize] = (state[1 as usize]).wrapping_add(state[6 as usize]);
            state[12 as usize] ^= state[1 as usize];
            state[12 as usize] = state[12 as usize] << 16 | state[12 as usize] >> 16;
            state[11 as usize] = (state[11 as usize]).wrapping_add(state[12 as usize]);
            state[6 as usize] ^= state[11 as usize];
            state[6 as usize] = state[6 as usize] << 12 | state[6 as usize] >> 20;
            state[1 as usize] = (state[1 as usize]).wrapping_add(state[6 as usize]);
            state[12 as usize] ^= state[1 as usize];
            state[12 as usize] = state[12 as usize] << 8 | state[12 as usize] >> 24;
            state[11 as usize] = (state[11 as usize]).wrapping_add(state[12 as usize]);
            state[6 as usize] ^= state[11 as usize];
            state[6 as usize] = state[6 as usize] << 7 | state[6 as usize] >> 25;
            state[2 as usize] = (state[2 as usize]).wrapping_add(state[7 as usize]);
            state[13 as usize] ^= state[2 as usize];
            state[13 as usize] = state[13 as usize] << 16 | state[13 as usize] >> 16;
            state[8 as usize] = (state[8 as usize]).wrapping_add(state[13 as usize]);
            state[7 as usize] ^= state[8 as usize];
            state[7 as usize] = state[7 as usize] << 12 | state[7 as usize] >> 20;
            state[2 as usize] = (state[2 as usize]).wrapping_add(state[7 as usize]);
            state[13 as usize] ^= state[2 as usize];
            state[13 as usize] = state[13 as usize] << 8 | state[13 as usize] >> 24;
            state[8 as usize] = (state[8 as usize]).wrapping_add(state[13 as usize]);
            state[7 as usize] ^= state[8 as usize];
            state[7 as usize] = state[7 as usize] << 7 | state[7 as usize] >> 25;
            state[3 as usize] = (state[3 as usize]).wrapping_add(state[4 as usize]);
            state[14 as usize] ^= state[3 as usize];
            state[14 as usize] = state[14 as usize] << 16 | state[14 as usize] >> 16;
            state[9 as usize] = (state[9 as usize]).wrapping_add(state[14 as usize]);
            state[4 as usize] ^= state[9 as usize];
            state[4 as usize] = state[4 as usize] << 12 | state[4 as usize] >> 20;
            state[3 as usize] = (state[3 as usize]).wrapping_add(state[4 as usize]);
            state[14 as usize] ^= state[3 as usize];
            state[14 as usize] = state[14 as usize] << 8 | state[14 as usize] >> 24;
            state[9 as usize] = (state[9 as usize]).wrapping_add(state[14 as usize]);
            state[4 as usize] ^= state[9 as usize];
            state[4 as usize] = state[4 as usize] << 7 | state[4 as usize] >> 25;
            i += 1;
            i;
        }
        v = 0 as size_t;
        while v < 4 as size_t {
            state[v as usize] = (state[v as usize]).wrapping_add(CW[v as usize]);
            v = v.wrapping_add(1);
            v;
        }
        v = 4 as size_t;
        while v < 14 as size_t {
            state[v as usize] = (state[v as usize]).wrapping_add(
                *(((*p).state.d).as_mut_ptr() as *mut u32)
                    .offset(v.wrapping_sub(4 as size_t) as isize),
            );
            v = v.wrapping_add(1);
            v;
        }
        state[14 as usize] = (state[14 as usize]).wrapping_add(
            *(((*p).state.d).as_mut_ptr() as *mut u32).offset(10 as isize) ^ cc as u32,
        );
        state[15 as usize] = (state[15 as usize]).wrapping_add(
            *(((*p).state.d).as_mut_ptr() as *mut u32).offset(11 as isize) ^ (cc >> 32) as u32,
        );
        cc = cc.wrapping_add(1);
        cc;
        v = 0 as size_t;
        while v < 16 as size_t {
            (*p).buf.d[(u << 2).wrapping_add(v << 5).wrapping_add(0 as size_t) as usize] =
                state[v as usize] as uint8_t;
            (*p).buf.d[(u << 2).wrapping_add(v << 5).wrapping_add(1 as size_t) as usize] =
                (state[v as usize] >> 8) as uint8_t;
            (*p).buf.d[(u << 2).wrapping_add(v << 5).wrapping_add(2 as size_t) as usize] =
                (state[v as usize] >> 16) as uint8_t;
            (*p).buf.d[(u << 2).wrapping_add(v << 5).wrapping_add(3 as size_t) as usize] =
                (state[v as usize] >> 24) as uint8_t;
            v = v.wrapping_add(1);
            v;
        }
        u = u.wrapping_add(1);
        u;
    }
    *(((*p).state.d).as_mut_ptr().offset(48 as isize) as *mut u64) = cc;
    (*p).ptr = 0 as size_t;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_prng_get_bytes(
    mut p: *mut prng,
    mut dst: *mut libc::c_void,
    mut len: size_t,
) {
    let mut buf: *mut uint8_t = 0 as *mut uint8_t;
    buf = dst as *mut uint8_t;
    while len > 0 as size_t {
        let mut clen: size_t = 0;
        clen = (::core::mem::size_of::<[uint8_t; 512]>() as libc::c_ulong).wrapping_sub((*p).ptr);
        if clen > len {
            clen = len;
        }
        rust_memcpy(
            buf as *mut libc::c_void,
            ((*p).buf.d).as_mut_ptr() as *const libc::c_void,
            clen,
        );
        buf = buf.offset(clen as isize);
        len = len.wrapping_sub(clen);
        (*p).ptr = ((*p).ptr).wrapping_add(clen);
        if (*p).ptr == ::core::mem::size_of::<[uint8_t; 512]>() as libc::c_ulong {
            PQCLEAN_FALCON512_CLEAN_prng_refill(p);
        }
    }
}
