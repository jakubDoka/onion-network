use crate::libc;
pub type __int8_t = libc::c_schar;
pub type __uint8_t = libc::c_uchar;
pub type __int16_t = libc::c_short;
pub type __uint16_t = libc::c_ushort;
pub type __int32_t = libc::c_int;
pub type __u32 = libc::c_uint;
pub type int8_t = __int8_t;
pub type int16_t = __int16_t;
pub type int32_t = __int32_t;
pub type uint8_t = __uint8_t;
pub type uint16_t = __uint16_t;

pub type size_t = libc::c_ulong;
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_modq_encode(
    mut out: *mut libc::c_void,
    mut max_out_len: size_t,
    mut x: *const uint16_t,
    mut logn: libc::c_uint,
) -> size_t {
    let mut n: size_t = 0;
    let mut out_len: size_t = 0;
    let mut u: size_t = 0;
    let mut buf: *mut uint8_t = 0 as *mut uint8_t;
    let mut acc: u32 = 0;
    let mut acc_len: libc::c_int = 0;
    n = (1 as size_t) << logn;
    u = 0 as size_t;
    while u < n {
        if *x.offset(u as isize) as libc::c_int >= 12289 {
            return 0 as size_t;
        }
        u = u.wrapping_add(1);
        u;
    }
    out_len = (n * 14 as size_t).wrapping_add(7 as size_t)
        >> 3;
    if out.is_null() {
        return out_len;
    }
    if out_len > max_out_len {
        return 0 as size_t;
    }
    buf = out as *mut uint8_t;
    acc = 0 as u32;
    acc_len = 0;
    u = 0 as size_t;
    while u < n {
        acc = acc << 14 | *x.offset(u as isize) as u32;
        acc_len += 14;
        while acc_len >= 8 {
            acc_len -= 8;
            let fresh0 = buf;
            buf = buf.offset(1);
            *fresh0 = (acc >> acc_len) as uint8_t;
        }
        u = u.wrapping_add(1);
        u;
    }
    if acc_len > 0 {
        *buf = (acc << 8 - acc_len) as uint8_t;
    }
    return out_len;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_modq_decode(
    mut x: *mut uint16_t,
    mut logn: libc::c_uint,
    mut in_0: *const libc::c_void,
    mut max_in_len: size_t,
) -> size_t {
    let mut n: size_t = 0;
    let mut in_len: size_t = 0;
    let mut u: size_t = 0;
    let mut buf: *const uint8_t = 0 as *const uint8_t;
    let mut acc: u32 = 0;
    let mut acc_len: libc::c_int = 0;
    n = (1 as size_t) << logn;
    in_len = (n * 14 as size_t).wrapping_add(7 as size_t)
        >> 3;
    if in_len > max_in_len {
        return 0 as size_t;
    }
    buf = in_0 as *const uint8_t;
    acc = 0 as u32;
    acc_len = 0;
    u = 0 as size_t;
    while u < n {
        let fresh1 = buf;
        buf = buf.offset(1);
        acc = acc << 8 | *fresh1 as u32;
        acc_len += 8;
        if acc_len >= 14 {
            let mut w: libc::c_uint = 0;
            acc_len -= 14;
            w = acc >> acc_len & 0x3fff as u32;
            if w >= 12289 as libc::c_uint {
                return 0 as size_t;
            }
            let fresh2 = u;
            u = u.wrapping_add(1);
            *x.offset(fresh2 as isize) = w as uint16_t;
        }
    }
    if acc
        & ((1 as u32) << acc_len)
            .wrapping_sub(1 as u32) != 0 as u32
    {
        return 0 as size_t;
    }
    return in_len;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_trim_i16_encode(
    mut out: *mut libc::c_void,
    mut max_out_len: size_t,
    mut x: *const int16_t,
    mut logn: libc::c_uint,
    mut bits: libc::c_uint,
) -> size_t {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut out_len: size_t = 0;
    let mut minv: libc::c_int = 0;
    let mut maxv: libc::c_int = 0;
    let mut buf: *mut uint8_t = 0 as *mut uint8_t;
    let mut acc: u32 = 0;
    let mut mask: u32 = 0;
    let mut acc_len: libc::c_uint = 0;
    n = (1 as size_t) << logn;
    maxv = ((1) << bits.wrapping_sub(1 as libc::c_uint))
        - 1;
    minv = -maxv;
    u = 0 as size_t;
    while u < n {
        if (*x.offset(u as isize) as libc::c_int) < minv
            || *x.offset(u as isize) as libc::c_int > maxv
        {
            return 0 as size_t;
        }
        u = u.wrapping_add(1);
        u;
    }
    out_len = (n * bits as size_t).wrapping_add(7 as size_t)
        >> 3;
    if out.is_null() {
        return out_len;
    }
    if out_len > max_out_len {
        return 0 as size_t;
    }
    buf = out as *mut uint8_t;
    acc = 0 as u32;
    acc_len = 0 as libc::c_uint;
    mask = ((1 as u32) << bits)
        .wrapping_sub(1 as u32);
    u = 0 as size_t;
    while u < n {
        acc = acc << bits | *x.offset(u as isize) as uint16_t as u32 & mask;
        acc_len = acc_len.wrapping_add(bits);
        while acc_len >= 8 as libc::c_uint {
            acc_len = acc_len.wrapping_sub(8 as libc::c_uint);
            let fresh3 = buf;
            buf = buf.offset(1);
            *fresh3 = (acc >> acc_len) as uint8_t;
        }
        u = u.wrapping_add(1);
        u;
    }
    if acc_len > 0 as libc::c_uint {
        let fresh4 = buf;
        buf = buf.offset(1);
        *fresh4 = (acc << (8 as libc::c_uint).wrapping_sub(acc_len))
            as uint8_t;
    }
    return out_len;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_trim_i16_decode(
    mut x: *mut int16_t,
    mut logn: libc::c_uint,
    mut bits: libc::c_uint,
    mut in_0: *const libc::c_void,
    mut max_in_len: size_t,
) -> size_t {
    let mut n: size_t = 0;
    let mut in_len: size_t = 0;
    let mut buf: *const uint8_t = 0 as *const uint8_t;
    let mut u: size_t = 0;
    let mut acc: u32 = 0;
    let mut mask1: u32 = 0;
    let mut mask2: u32 = 0;
    let mut acc_len: libc::c_uint = 0;
    n = (1 as size_t) << logn;
    in_len = (n * bits as size_t).wrapping_add(7 as size_t)
        >> 3;
    if in_len > max_in_len {
        return 0 as size_t;
    }
    buf = in_0 as *const uint8_t;
    u = 0 as size_t;
    acc = 0 as u32;
    acc_len = 0 as libc::c_uint;
    mask1 = ((1 as u32) << bits)
        .wrapping_sub(1 as u32);
    mask2 = (1 as u32)
        << bits.wrapping_sub(1 as libc::c_uint);
    while u < n {
        let fresh5 = buf;
        buf = buf.offset(1);
        acc = acc << 8 | *fresh5 as u32;
        acc_len = acc_len.wrapping_add(8 as libc::c_uint);
        while acc_len >= bits && u < n {
            let mut w: u32 = 0;
            acc_len = acc_len.wrapping_sub(bits);
            w = acc >> acc_len & mask1;
            w |= (w & mask2).wrapping_neg();
            if w == mask2.wrapping_neg() {
                return 0 as size_t;
            }
            w |= (w & mask2).wrapping_neg();
            let fresh6 = u;
            u = u.wrapping_add(1);
            *x
                .offset(
                    fresh6 as isize,
                ) = *(&mut w as *mut u32 as *mut int32_t) as int16_t;
        }
    }
    if acc
        & ((1 as u32) << acc_len)
            .wrapping_sub(1 as u32) != 0 as u32
    {
        return 0 as size_t;
    }
    return in_len;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_trim_i8_encode(
    mut out: *mut libc::c_void,
    mut max_out_len: size_t,
    mut x: *const int8_t,
    mut logn: libc::c_uint,
    mut bits: libc::c_uint,
) -> size_t {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut out_len: size_t = 0;
    let mut minv: libc::c_int = 0;
    let mut maxv: libc::c_int = 0;
    let mut buf: *mut uint8_t = 0 as *mut uint8_t;
    let mut acc: u32 = 0;
    let mut mask: u32 = 0;
    let mut acc_len: libc::c_uint = 0;
    n = (1 as size_t) << logn;
    maxv = ((1) << bits.wrapping_sub(1 as libc::c_uint))
        - 1;
    minv = -maxv;
    u = 0 as size_t;
    while u < n {
        if (*x.offset(u as isize) as libc::c_int) < minv
            || *x.offset(u as isize) as libc::c_int > maxv
        {
            return 0 as size_t;
        }
        u = u.wrapping_add(1);
        u;
    }
    out_len = (n * bits as size_t).wrapping_add(7 as size_t)
        >> 3;
    if out.is_null() {
        return out_len;
    }
    if out_len > max_out_len {
        return 0 as size_t;
    }
    buf = out as *mut uint8_t;
    acc = 0 as u32;
    acc_len = 0 as libc::c_uint;
    mask = ((1 as u32) << bits)
        .wrapping_sub(1 as u32);
    u = 0 as size_t;
    while u < n {
        acc = acc << bits | *x.offset(u as isize) as uint8_t as u32 & mask;
        acc_len = acc_len.wrapping_add(bits);
        while acc_len >= 8 as libc::c_uint {
            acc_len = acc_len.wrapping_sub(8 as libc::c_uint);
            let fresh7 = buf;
            buf = buf.offset(1);
            *fresh7 = (acc >> acc_len) as uint8_t;
        }
        u = u.wrapping_add(1);
        u;
    }
    if acc_len > 0 as libc::c_uint {
        let fresh8 = buf;
        buf = buf.offset(1);
        *fresh8 = (acc << (8 as libc::c_uint).wrapping_sub(acc_len))
            as uint8_t;
    }
    return out_len;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_trim_i8_decode(
    mut x: *mut int8_t,
    mut logn: libc::c_uint,
    mut bits: libc::c_uint,
    mut in_0: *const libc::c_void,
    mut max_in_len: size_t,
) -> size_t {
    let mut n: size_t = 0;
    let mut in_len: size_t = 0;
    let mut buf: *const uint8_t = 0 as *const uint8_t;
    let mut u: size_t = 0;
    let mut acc: u32 = 0;
    let mut mask1: u32 = 0;
    let mut mask2: u32 = 0;
    let mut acc_len: libc::c_uint = 0;
    n = (1 as size_t) << logn;
    in_len = (n * bits as size_t).wrapping_add(7 as size_t)
        >> 3;
    if in_len > max_in_len {
        return 0 as size_t;
    }
    buf = in_0 as *const uint8_t;
    u = 0 as size_t;
    acc = 0 as u32;
    acc_len = 0 as libc::c_uint;
    mask1 = ((1 as u32) << bits)
        .wrapping_sub(1 as u32);
    mask2 = (1 as u32)
        << bits.wrapping_sub(1 as libc::c_uint);
    while u < n {
        let fresh9 = buf;
        buf = buf.offset(1);
        acc = acc << 8 | *fresh9 as u32;
        acc_len = acc_len.wrapping_add(8 as libc::c_uint);
        while acc_len >= bits && u < n {
            let mut w: u32 = 0;
            acc_len = acc_len.wrapping_sub(bits);
            w = acc >> acc_len & mask1;
            w |= (w & mask2).wrapping_neg();
            if w == mask2.wrapping_neg() {
                return 0 as size_t;
            }
            let fresh10 = u;
            u = u.wrapping_add(1);
            *x
                .offset(
                    fresh10 as isize,
                ) = *(&mut w as *mut u32 as *mut int32_t) as int8_t;
        }
    }
    if acc
        & ((1 as u32) << acc_len)
            .wrapping_sub(1 as u32) != 0 as u32
    {
        return 0 as size_t;
    }
    return in_len;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_comp_encode(
    mut out: *mut libc::c_void,
    mut max_out_len: size_t,
    mut x: *const int16_t,
    mut logn: libc::c_uint,
) -> size_t {
    let mut buf: *mut uint8_t = 0 as *mut uint8_t;
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut v: size_t = 0;
    let mut acc: u32 = 0;
    let mut acc_len: libc::c_uint = 0;
    n = (1 as size_t) << logn;
    buf = out as *mut uint8_t;
    u = 0 as size_t;
    while u < n {
        if (*x.offset(u as isize) as libc::c_int) < -(2047)
            || *x.offset(u as isize) as libc::c_int > 2047
        {
            return 0 as size_t;
        }
        u = u.wrapping_add(1);
        u;
    }
    acc = 0 as u32;
    acc_len = 0 as libc::c_uint;
    v = 0 as size_t;
    u = 0 as size_t;
    while u < n {
        let mut t: libc::c_int = 0;
        let mut w: libc::c_uint = 0;
        acc <<= 1;
        t = *x.offset(u as isize) as libc::c_int;
        if t < 0 {
            t = -t;
            acc |= 1 as u32;
        }
        w = t as libc::c_uint;
        acc <<= 7;
        acc |= w & 127 as libc::c_uint;
        w >>= 7;
        acc_len = acc_len.wrapping_add(8 as libc::c_uint);
        acc <<= w.wrapping_add(1 as libc::c_uint);
        acc |= 1 as u32;
        acc_len = acc_len.wrapping_add(w.wrapping_add(1 as libc::c_uint));
        while acc_len >= 8 as libc::c_uint {
            acc_len = acc_len.wrapping_sub(8 as libc::c_uint);
            if !buf.is_null() {
                if v >= max_out_len {
                    return 0 as size_t;
                }
                *buf.offset(v as isize) = (acc >> acc_len) as uint8_t;
            }
            v = v.wrapping_add(1);
            v;
        }
        u = u.wrapping_add(1);
        u;
    }
    if acc_len > 0 as libc::c_uint {
        if !buf.is_null() {
            if v >= max_out_len {
                return 0 as size_t;
            }
            *buf
                .offset(
                    v as isize,
                ) = (acc << (8 as libc::c_uint).wrapping_sub(acc_len))
                as uint8_t;
        }
        v = v.wrapping_add(1);
        v;
    }
    return v;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_comp_decode(
    mut x: *mut int16_t,
    mut logn: libc::c_uint,
    mut in_0: *const libc::c_void,
    mut max_in_len: size_t,
) -> size_t {
    let mut buf: *const uint8_t = 0 as *const uint8_t;
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut v: size_t = 0;
    let mut acc: u32 = 0;
    let mut acc_len: libc::c_uint = 0;
    n = (1 as size_t) << logn;
    buf = in_0 as *const uint8_t;
    acc = 0 as u32;
    acc_len = 0 as libc::c_uint;
    v = 0 as size_t;
    u = 0 as size_t;
    while u < n {
        let mut b: libc::c_uint = 0;
        let mut s: libc::c_uint = 0;
        let mut m: libc::c_uint = 0;
        if v >= max_in_len {
            return 0 as size_t;
        }
        let fresh11 = v;
        v = v.wrapping_add(1);
        acc = acc << 8 | *buf.offset(fresh11 as isize) as u32;
        b = acc >> acc_len;
        s = b & 128 as libc::c_uint;
        m = b & 127 as libc::c_uint;
        loop {
            if acc_len == 0 as libc::c_uint {
                if v >= max_in_len {
                    return 0 as size_t;
                }
                let fresh12 = v;
                v = v.wrapping_add(1);
                acc = acc << 8
                    | *buf.offset(fresh12 as isize) as u32;
                acc_len = 8 as libc::c_uint;
            }
            acc_len = acc_len.wrapping_sub(1);
            acc_len;
            if acc >> acc_len & 1 as u32
                != 0 as u32
            {
                break;
            }
            m = m.wrapping_add(128 as libc::c_uint);
            if m > 2047 as libc::c_uint {
                return 0 as size_t;
            }
        }
        if s != 0 && m == 0 as libc::c_uint {
            return 0 as size_t;
        }
        if s != 0 {
            *x.offset(u as isize) = m.wrapping_neg() as int16_t;
        } else {
            *x.offset(u as isize) = m as int16_t;
        }
        u = u.wrapping_add(1);
        u;
    }
    if acc & ((1 as libc::c_uint) << acc_len).wrapping_sub(1 as libc::c_uint)
        != 0 as libc::c_uint
    {
        return 0 as size_t;
    }
    return v;
}
#[no_mangle]
pub static mut PQCLEAN_FALCON512_CLEAN_max_fg_bits: [uint8_t; 11] = [
    0 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    7 as uint8_t,
    7 as uint8_t,
    6 as uint8_t,
    6 as uint8_t,
    5 as uint8_t,
];
#[no_mangle]
pub static mut PQCLEAN_FALCON512_CLEAN_max_FG_bits: [uint8_t; 11] = [
    0 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
    8 as uint8_t,
];
#[no_mangle]
pub static mut PQCLEAN_FALCON512_CLEAN_max_sig_bits: [uint8_t; 11] = [
    0 as uint8_t,
    10 as uint8_t,
    11 as uint8_t,
    11 as uint8_t,
    12 as uint8_t,
    12 as uint8_t,
    12 as uint8_t,
    12 as uint8_t,
    12 as uint8_t,
    12 as uint8_t,
    12 as uint8_t,
];
