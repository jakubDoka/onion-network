use crate::libc;
extern "C" {
    fn shake256_inc_squeeze(output: *mut uint8_t, outlen: size_t, state: *mut shake256incctx);
}
pub type __uint8_t = libc::c_uchar;
pub type __int16_t = libc::c_short;
pub type __uint16_t = libc::c_ushort;
pub type __int32_t = libc::c_int;
pub type __u32 = libc::c_uint;
pub type __u64 = libc::c_ulong;
pub type int16_t = __int16_t;
pub type int32_t = __int32_t;
pub type uint8_t = __uint8_t;
pub type uint16_t = __uint16_t;

pub type size_t = libc::c_ulong;
#[derive()]
#[repr(C)]
pub struct shake256incctx {
    pub ctx: crate::shake::Ctx,
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_hash_to_point_vartime(
    mut sc: *mut shake256incctx,
    mut x: *mut uint16_t,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    n = (1 as size_t) << logn;
    while n > 0 as size_t {
        let mut buf: [uint8_t; 2] = [0; 2];
        let mut w: u32 = 0;
        shake256_inc_squeeze(
            buf.as_mut_ptr() as *mut libc::c_void as *mut uint8_t,
            ::core::mem::size_of::<[uint8_t; 2]>() as libc::c_ulong,
            sc,
        );
        w = (buf[0 as usize] as libc::c_uint) << 8 | buf[1 as usize] as libc::c_uint;
        if w < 61445 as u32 {
            while w >= 12289 as u32 {
                w = w.wrapping_sub(12289 as u32);
            }
            let fresh0 = x;
            x = x.offset(1);
            *fresh0 = w as uint16_t;
            n = n.wrapping_sub(1);
            n;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_hash_to_point_ct(
    mut sc: *mut shake256incctx,
    mut x: *mut uint16_t,
    mut logn: libc::c_uint,
    mut tmp: *mut uint8_t,
) {
    static mut overtab: [uint16_t; 11] = [
        0 as uint16_t,
        65 as uint16_t,
        67 as uint16_t,
        71 as uint16_t,
        77 as uint16_t,
        86 as uint16_t,
        100 as uint16_t,
        122 as uint16_t,
        154 as uint16_t,
        205 as uint16_t,
        287 as uint16_t,
    ];
    let mut n: libc::c_uint = 0;
    let mut n2: libc::c_uint = 0;
    let mut u: libc::c_uint = 0;
    let mut m: libc::c_uint = 0;
    let mut p: libc::c_uint = 0;
    let mut over: libc::c_uint = 0;
    let mut tt1: *mut uint16_t = 0 as *mut uint16_t;
    let mut tt2: [uint16_t; 63] = [0; 63];
    n = (1 as libc::c_uint) << logn;
    n2 = n << 1;
    over = overtab[logn as usize] as libc::c_uint;
    m = n.wrapping_add(over);
    tt1 = tmp as *mut uint16_t;
    u = 0 as libc::c_uint;
    while u < m {
        let mut buf: [uint8_t; 2] = [0; 2];
        let mut w: u32 = 0;
        let mut wr: u32 = 0;
        shake256_inc_squeeze(
            buf.as_mut_ptr(),
            ::core::mem::size_of::<[uint8_t; 2]>() as libc::c_ulong,
            sc,
        );
        w = (buf[0 as usize] as u32) << 8 | buf[1 as usize] as u32;
        wr = w.wrapping_sub(
            24578 as u32 & (w.wrapping_sub(24578 as u32) >> 31).wrapping_sub(1 as u32),
        );
        wr = wr.wrapping_sub(
            24578 as u32 & (wr.wrapping_sub(24578 as u32) >> 31).wrapping_sub(1 as u32),
        );
        wr = wr.wrapping_sub(
            12289 as u32 & (wr.wrapping_sub(12289 as u32) >> 31).wrapping_sub(1 as u32),
        );
        wr |= (w.wrapping_sub(61445 as u32) >> 31).wrapping_sub(1 as u32);
        if u < n {
            *x.offset(u as isize) = wr as uint16_t;
        } else if u < n2 {
            *tt1.offset(u.wrapping_sub(n) as isize) = wr as uint16_t;
        } else {
            tt2[u.wrapping_sub(n2) as usize] = wr as uint16_t;
        }
        u = u.wrapping_add(1);
        u;
    }
    p = 1 as libc::c_uint;
    while p <= over {
        let mut v: libc::c_uint = 0;
        v = 0 as libc::c_uint;
        u = 0 as libc::c_uint;
        while u < m {
            let mut s: *mut uint16_t = 0 as *mut uint16_t;
            let mut d: *mut uint16_t = 0 as *mut uint16_t;
            let mut j: libc::c_uint = 0;
            let mut sv: libc::c_uint = 0;
            let mut dv: libc::c_uint = 0;
            let mut mk: libc::c_uint = 0;
            if u < n {
                s = &mut *x.offset(u as isize) as *mut uint16_t;
            } else if u < n2 {
                s = &mut *tt1.offset(u.wrapping_sub(n) as isize) as *mut uint16_t;
            } else {
                s = &mut *tt2.as_mut_ptr().offset(u.wrapping_sub(n2) as isize) as *mut uint16_t;
            }
            sv = *s as libc::c_uint;
            j = u.wrapping_sub(v);
            mk = (sv >> 15).wrapping_sub(1 as libc::c_uint);
            v = v.wrapping_sub(mk);
            if !(u < p) {
                if u.wrapping_sub(p) < n {
                    d = &mut *x.offset(u.wrapping_sub(p) as isize) as *mut uint16_t;
                } else if u.wrapping_sub(p) < n2 {
                    d = &mut *tt1.offset(u.wrapping_sub(p).wrapping_sub(n) as isize)
                        as *mut uint16_t;
                } else {
                    d = &mut *tt2.as_mut_ptr().offset(u.wrapping_sub(p).wrapping_sub(n2) as isize)
                        as *mut uint16_t;
                }
                dv = *d as libc::c_uint;
                mk &= ((j & p).wrapping_add(0x1ff as libc::c_uint) >> 9).wrapping_neg();
                *s = (sv ^ mk & (sv ^ dv)) as uint16_t;
                *d = (dv ^ mk & (sv ^ dv)) as uint16_t;
            }
            u = u.wrapping_add(1);
            u;
        }
        p <<= 1;
    }
}
static mut l2bound: [u32; 11] = [
    0 as u32,
    101498 as u32,
    208714 as u32,
    428865 as u32,
    892039 as u32,
    1852696 as u32,
    3842630 as u32,
    7959734 as u32,
    16468416 as u32,
    34034726 as u32,
    70265242 as u32,
];
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_is_short(
    mut s1: *const int16_t,
    mut s2: *const int16_t,
    mut logn: libc::c_uint,
) -> libc::c_int {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut s: u32 = 0;
    let mut ng: u32 = 0;
    n = (1 as size_t) << logn;
    s = 0 as u32;
    ng = 0 as u32;
    u = 0 as size_t;
    while u < n {
        let mut z: int32_t = 0;
        z = *s1.offset(u as isize) as int32_t;
        s = s.wrapping_add((z * z) as u32);
        ng |= s;
        z = *s2.offset(u as isize) as int32_t;
        s = s.wrapping_add((z * z) as u32);
        ng |= s;
        u = u.wrapping_add(1);
        u;
    }
    s |= (ng >> 31).wrapping_neg();
    return (s <= l2bound[logn as usize]) as libc::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_is_short_half(
    mut sqn: u32,
    mut s2: *const int16_t,
    mut logn: libc::c_uint,
) -> libc::c_int {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut ng: u32 = 0;
    n = (1 as size_t) << logn;
    ng = (sqn >> 31).wrapping_neg();
    u = 0 as size_t;
    while u < n {
        let mut z: int32_t = 0;
        z = *s2.offset(u as isize) as int32_t;
        sqn = sqn.wrapping_add((z * z) as u32);
        ng |= sqn;
        u = u.wrapping_add(1);
        u;
    }
    sqn |= (ng >> 31).wrapping_neg();
    return (sqn <= l2bound[logn as usize]) as libc::c_int;
}
