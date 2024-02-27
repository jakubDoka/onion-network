use crate::libc;
extern "C" {
    fn shake256_inc_squeeze(output: *mut uint8_t, outlen: size_t, state: *mut shake256incctx);
    fn PQCLEAN_FALCON512_CLEAN_poly_div_autoadj_fft(a: *mut fpr, b: *const fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_mul_autoadj_fft(a: *mut fpr, b: *const fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_add_muladj_fft(
        d: *mut fpr,
        F: *const fpr,
        G: *const fpr,
        f: *const fpr,
        g: *const fpr,
        logn: libc::c_uint,
    );
    fn PQCLEAN_FALCON512_CLEAN_poly_invnorm2_fft(
        d: *mut fpr,
        a: *const fpr,
        b: *const fpr,
        logn: libc::c_uint,
    );
    fn PQCLEAN_FALCON512_CLEAN_poly_mulconst(a: *mut fpr, x: fpr, logn: libc::c_uint);
    fn rust_memcpy(
        _: *mut libc::c_void,
        _: *const libc::c_void,
        _: libc::c_ulong,
    ) -> *mut libc::c_void;
    fn rust_memmove(
        _: *mut libc::c_void,
        _: *const libc::c_void,
        _: libc::c_ulong,
    ) -> *mut libc::c_void;
    fn rust_memset(_: *mut libc::c_void, _: libc::c_int, _: libc::c_ulong) -> *mut libc::c_void;
    fn PQCLEAN_FALCON512_CLEAN_poly_mul_fft(a: *mut fpr, b: *const fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_adj_fft(a: *mut fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_sub(a: *mut fpr, b: *const fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_add(a: *mut fpr, b: *const fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_iFFT(f: *mut fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_FFT(f: *mut fpr, logn: libc::c_uint);
    static PQCLEAN_FALCON512_CLEAN_max_fg_bits: [uint8_t; 0];
    static PQCLEAN_FALCON512_CLEAN_max_FG_bits: [uint8_t; 0];
    fn PQCLEAN_FALCON512_CLEAN_compute_public(
        h: *mut uint16_t,
        f: *const int8_t,
        g: *const int8_t,
        logn: libc::c_uint,
        tmp: *mut uint8_t,
    ) -> libc::c_int;
    fn PQCLEAN_FALCON512_CLEAN_fpr_scaled(i: i64, sc: libc::c_int) -> fpr;
    fn PQCLEAN_FALCON512_CLEAN_fpr_add(x: fpr, y: fpr) -> fpr;
    fn PQCLEAN_FALCON512_CLEAN_fpr_mul(x: fpr, y: fpr) -> fpr;
}
pub type __int8_t = libc::c_schar;
pub type __uint8_t = libc::c_uchar;
pub type __uint16_t = libc::c_ushort;
pub type __int32_t = libc::c_int;
pub type __u32 = libc::c_uint;
pub type __i64 = libc::c_long;
pub type __u64 = libc::c_ulong;
pub type int8_t = __int8_t;
pub type int32_t = __int32_t;

pub type uint8_t = __uint8_t;
pub type uint16_t = __uint16_t;

pub type size_t = libc::c_ulong;
#[derive()]
#[repr(C)]
pub struct shake256incctx {
    pub ctx: crate::shake::Ctx,
}
pub type fpr = u64;
#[derive()]
#[repr(C)]
pub struct small_prime {
    pub p: u32,
    pub g: u32,
    pub s: u32,
}
#[derive()]
#[repr(C)]
pub struct C2RustUnnamed {
    pub avg: libc::c_int,
    pub std: libc::c_int,
}
#[inline]
unsafe extern "C" fn fpr_lt(mut x: fpr, mut y: fpr) -> libc::c_int {
    let mut cc0: libc::c_int = 0;
    let mut cc1: libc::c_int = 0;
    let mut sx: i64 = 0;
    let mut sy: i64 = 0;
    sx = *(&mut x as *mut fpr as *mut i64);
    sy = *(&mut y as *mut fpr as *mut i64);
    sy &= !((sx ^ sy) >> 63);
    cc0 = (sx - sy >> 63) as libc::c_int & 1;
    cc1 = (sy - sx >> 63) as libc::c_int & 1;
    return cc0 ^ (cc0 ^ cc1) & ((x & y) >> 63) as libc::c_int;
}
#[inline]
unsafe extern "C" fn fpr_ursh(mut x: u64, mut n: libc::c_int) -> u64 {
    x ^= (x ^ x >> 32) & ((n >> 5) as u64).wrapping_neg();
    return x >> (n & 31);
}
#[inline]
unsafe extern "C" fn fpr_ulsh(mut x: u64, mut n: libc::c_int) -> u64 {
    x ^= (x ^ x << 32) & ((n >> 5) as u64).wrapping_neg();
    return x << (n & 31);
}
#[inline]
unsafe extern "C" fn fpr_of(mut i: i64) -> fpr {
    return PQCLEAN_FALCON512_CLEAN_fpr_scaled(i, 0);
}
static mut fpr_q: fpr = 4667981563525332992 as fpr;
static mut fpr_bnorm_max: fpr = 4670353323383631276 as fpr;
static mut fpr_zero: fpr = 0 as fpr;
static mut fpr_one: fpr = 4607182418800017408 as fpr;
static mut fpr_two: fpr = 4611686018427387904 as fpr;
static mut fpr_onehalf: fpr = 4602678819172646912 as fpr;
static mut fpr_ptwo31: fpr = 4746794007248502784 as fpr;
static mut fpr_ptwo31m1: fpr = 4746794007244308480 as fpr;
static mut fpr_mtwo31m1: fpr = 13970166044099084288;
static mut fpr_ptwo63m1: fpr = 4890909195324358656 as fpr;
static mut fpr_mtwo63m1: fpr = 14114281232179134464;
#[inline]
unsafe extern "C" fn fpr_rint(mut x: fpr) -> i64 {
    let mut m: u64 = 0;
    let mut d: u64 = 0;
    let mut e: libc::c_int = 0;
    let mut s: u32 = 0;
    let mut dd: u32 = 0;
    let mut f: u32 = 0;
    m = (x << 10 | (1 as u64) << 62) & ((1 as u64) << 63).wrapping_sub(1 as u64);
    e = 1085 - ((x >> 52) as libc::c_int & 0x7ff);
    m &= (((e - 64) as u32 >> 31) as u64).wrapping_neg();
    e &= 63;
    d = fpr_ulsh(m, 63 - e);
    dd = d as u32 | (d >> 32) as u32 & 0x1fffffff as u32;
    f = (d >> 61) as u32 | (dd | dd.wrapping_neg()) >> 31;
    m = (fpr_ursh(m, e)).wrapping_add((0xc8 as libc::c_uint >> f & 1 as libc::c_uint) as u64);
    s = (x >> 63) as u32;
    return (m as i64 ^ -(s as i64)) + s as i64;
}
#[inline]
unsafe extern "C" fn fpr_sqr(mut x: fpr) -> fpr {
    return PQCLEAN_FALCON512_CLEAN_fpr_mul(x, x);
}
static mut PRIMES: [small_prime; 522] = [
    {
        let mut init = small_prime { p: 2147473409 as u32, g: 383167813 as u32, s: 10239 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147389441 as u32, g: 211808905 as u32, s: 471403745 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147387393 as u32, g: 37672282 as u32, s: 1329335065 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147377153 as u32, g: 1977035326 as u32, s: 968223422 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147358721 as u32, g: 1067163706 as u32, s: 132460015 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147352577 as u32, g: 1606082042 as u32, s: 598693809 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147346433 as u32, g: 2033915641 as u32, s: 1056257184 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147338241 as u32, g: 1653770625 as u32, s: 421286710 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147309569 as u32, g: 631200819 as u32, s: 1111201074 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147297281 as u32, g: 2038364663 as u32, s: 1042003613 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147295233 as u32, g: 1962540515 as u32, s: 19440033 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147239937 as u32, g: 2100082663 as u32, s: 353296760 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147235841 as u32, g: 1991153006 as u32, s: 1703918027 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147217409 as u32, g: 516405114 as u32, s: 1258919613 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147205121 as u32, g: 409347988 as u32, s: 1089726929 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147196929 as u32, g: 927788991 as u32, s: 1946238668 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147178497 as u32, g: 1136922411 as u32, s: 1347028164 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147100673 as u32, g: 868626236 as u32, s: 701164723 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147082241 as u32, g: 1897279176 as u32, s: 617820870 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147074049 as u32, g: 1888819123 as u32, s: 158382189 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147051521 as u32, g: 25006327 as u32, s: 522758543 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147043329 as u32, g: 327546255 as u32, s: 37227845 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2147039233 as u32, g: 766324424 as u32, s: 1133356428 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146988033 as u32, g: 1862817362 as u32, s: 73861329 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146963457 as u32, g: 404622040 as u32, s: 653019435 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146959361 as u32, g: 1936581214 as u32, s: 995143093 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146938881 as u32, g: 1559770096 as u32, s: 634921513 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146908161 as u32, g: 422623708 as u32, s: 1985060172 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146885633 as u32, g: 1751189170 as u32, s: 298238186 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146871297 as u32, g: 578919515 as u32, s: 291810829 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146846721 as u32, g: 1114060353 as u32, s: 915902322 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146834433 as u32, g: 2069565474 as u32, s: 47859524 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146818049 as u32, g: 1552824584 as u32, s: 646281055 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146775041 as u32, g: 1906267847 as u32, s: 1597832891 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146756609 as u32, g: 1847414714 as u32, s: 1228090888 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146744321 as u32, g: 1818792070 as u32, s: 1176377637 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146738177 as u32, g: 1118066398 as u32, s: 1054971214 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146736129 as u32, g: 52057278 as u32, s: 933422153 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146713601 as u32, g: 592259376 as u32, s: 1406621510 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146695169 as u32, g: 263161877 as u32, s: 1514178701 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146656257 as u32, g: 685363115 as u32, s: 384505091 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146650113 as u32, g: 927727032 as u32, s: 537575289 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146646017 as u32, g: 52575506 as u32, s: 1799464037 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146643969 as u32, g: 1276803876 as u32, s: 1348954416 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146603009 as u32, g: 814028633 as u32, s: 1521547704 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146572289 as u32, g: 1846678872 as u32, s: 1310832121 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146547713 as u32, g: 919368090 as u32, s: 1019041349 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146508801 as u32, g: 671847612 as u32, s: 38582496 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146492417 as u32, g: 283911680 as u32, s: 532424562 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146490369 as u32, g: 1780044827 as u32, s: 896447978 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146459649 as u32, g: 327980850 as u32, s: 1327906900 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146447361 as u32, g: 1310561493 as u32, s: 958645253 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146441217 as u32, g: 412148926 as u32, s: 287271128 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146437121 as u32, g: 293186449 as u32, s: 2009822534 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146430977 as u32, g: 179034356 as u32, s: 1359155584 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146418689 as u32, g: 1517345488 as u32, s: 1790248672 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146406401 as u32, g: 1615820390 as u32, s: 1584833571 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146404353 as u32, g: 826651445 as u32, s: 607120498 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146379777 as u32, g: 3816988 as u32, s: 1897049071 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146363393 as u32, g: 1221409784 as u32, s: 1986921567 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146355201 as u32, g: 1388081168 as u32, s: 849968120 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146336769 as u32, g: 1803473237 as u32, s: 1655544036 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146312193 as u32, g: 1023484977 as u32, s: 273671831 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146293761 as u32, g: 1074591448 as u32, s: 467406983 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146283521 as u32, g: 831604668 as u32, s: 1523950494 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146203649 as u32, g: 712865423 as u32, s: 1170834574 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146154497 as u32, g: 1764991362 as u32, s: 1064856763 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146142209 as u32, g: 627386213 as u32, s: 1406840151 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146127873 as u32, g: 1638674429 as u32, s: 2088393537 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146099201 as u32, g: 1516001018 as u32, s: 690673370 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146093057 as u32, g: 1294931393 as u32, s: 315136610 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146091009 as u32, g: 1942399533 as u32, s: 973539425 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146078721 as u32, g: 1843461814 as u32, s: 2132275436 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146060289 as u32, g: 1098740778 as u32, s: 360423481 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146048001 as u32, g: 1617213232 as u32, s: 1951981294 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146041857 as u32, g: 1805783169 as u32, s: 2075683489 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2146019329 as u32, g: 272027909 as u32, s: 1753219918 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145986561 as u32, g: 1206530344 as u32, s: 2034028118 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145976321 as u32, g: 1243769360 as u32, s: 1173377644 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145964033 as u32, g: 887200839 as u32, s: 1281344586 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145906689 as u32, g: 1651026455 as u32, s: 906178216 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145875969 as u32, g: 1673238256 as u32, s: 1043521212 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145871873 as u32, g: 1226591210 as u32, s: 1399796492 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145841153 as u32, g: 1465353397 as u32, s: 1324527802 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145832961 as u32, g: 1150638905 as u32, s: 554084759 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145816577 as u32, g: 221601706 as u32, s: 427340863 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145785857 as u32, g: 608896761 as u32, s: 316590738 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145755137 as u32, g: 1712054942 as u32, s: 1684294304 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145742849 as u32, g: 1302302867 as u32, s: 724873116 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145728513 as u32, g: 516717693 as u32, s: 431671476 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145699841 as u32, g: 524575579 as u32, s: 1619722537 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145691649 as u32, g: 1925625239 as u32, s: 982974435 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145687553 as u32, g: 463795662 as u32, s: 1293154300 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145673217 as u32, g: 771716636 as u32, s: 881778029 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145630209 as u32, g: 1509556977 as u32, s: 837364988 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145595393 as u32, g: 229091856 as u32, s: 851648427 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145587201 as u32, g: 1796903241 as u32, s: 635342424 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145525761 as u32, g: 715310882 as u32, s: 1677228081 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145495041 as u32, g: 1040930522 as u32, s: 200685896 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145466369 as u32, g: 949804237 as u32, s: 1809146322 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145445889 as u32, g: 1673903706 as u32, s: 95316881 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145390593 as u32, g: 806941852 as u32, s: 1428671135 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145372161 as u32, g: 1402525292 as u32, s: 159350694 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145361921 as u32, g: 2124760298 as u32, s: 1589134749 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145359873 as u32, g: 1217503067 as u32, s: 1561543010 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145355777 as u32, g: 338341402 as u32, s: 83865711 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145343489 as u32, g: 1381532164 as u32, s: 641430002 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145325057 as u32, g: 1883895478 as u32, s: 1528469895 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145318913 as u32, g: 1335370424 as u32, s: 65809740 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145312769 as u32, g: 2000008042 as u32, s: 1919775760 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145300481 as u32, g: 961450962 as u32, s: 1229540578 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145282049 as u32, g: 910466767 as u32, s: 1964062701 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145232897 as u32, g: 816527501 as u32, s: 450152063 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145218561 as u32, g: 1435128058 as u32, s: 1794509700 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145187841 as u32, g: 33505311 as u32, s: 1272467582 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145181697 as u32, g: 269767433 as u32, s: 1380363849 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145175553 as u32, g: 56386299 as u32, s: 1316870546 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145079297 as u32, g: 2106880293 as u32, s: 1391797340 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145021953 as u32, g: 1347906152 as u32, s: 720510798 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145015809 as u32, g: 206769262 as u32, s: 1651459955 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2145003521 as u32, g: 1885513236 as u32, s: 1393381284 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144960513 as u32, g: 1810381315 as u32, s: 31937275 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144944129 as u32, g: 1306487838 as u32, s: 2019419520 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144935937 as u32, g: 37304730 as u32, s: 1841489054 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144894977 as u32, g: 1601434616 as u32, s: 157985831 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144888833 as u32, g: 98749330 as u32, s: 2128592228 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144880641 as u32, g: 1772327002 as u32, s: 2076128344 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144864257 as u32, g: 1404514762 as u32, s: 2029969964 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144827393 as u32, g: 801236594 as u32, s: 406627220 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144806913 as u32, g: 349217443 as u32, s: 1501080290 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144796673 as u32, g: 1542656776 as u32, s: 2084736519 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144778241 as u32, g: 1210734884 as u32, s: 1746416203 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144759809 as u32, g: 1146598851 as u32, s: 716464489 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144757761 as u32, g: 286328400 as u32, s: 1823728177 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144729089 as u32, g: 1347555695 as u32, s: 1836644881 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144727041 as u32, g: 1795703790 as u32, s: 520296412 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144696321 as u32, g: 1302475157 as u32, s: 852964281 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144667649 as u32, g: 1075877614 as u32, s: 504992927 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144573441 as u32, g: 198765808 as u32, s: 1617144982 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144555009 as u32, g: 321528767 as u32, s: 155821259 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144550913 as u32, g: 814139516 as u32, s: 1819937644 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144536577 as u32, g: 571143206 as u32, s: 962942255 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144524289 as u32, g: 1746733766 as u32, s: 2471321 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144512001 as u32, g: 1821415077 as u32, s: 124190939 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144468993 as u32, g: 917871546 as u32, s: 1260072806 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144458753 as u32, g: 378417981 as u32, s: 1569240563 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144421889 as u32, g: 175229668 as u32, s: 1825620763 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144409601 as u32, g: 1699216963 as u32, s: 351648117 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144370689 as u32, g: 1071885991 as u32, s: 958186029 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144348161 as u32, g: 1763151227 as u32, s: 540353574 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144335873 as u32, g: 1060214804 as u32, s: 919598847 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144329729 as u32, g: 663515846 as u32, s: 1448552668 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144327681 as u32, g: 1057776305 as u32, s: 590222840 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144309249 as u32, g: 1705149168 as u32, s: 1459294624 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144296961 as u32, g: 325823721 as u32, s: 1649016934 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144290817 as u32, g: 738775789 as u32, s: 447427206 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144243713 as u32, g: 962347618 as u32, s: 893050215 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144237569 as u32, g: 1655257077 as u32, s: 900860862 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144161793 as u32, g: 242206694 as u32, s: 1567868672 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144155649 as u32, g: 769415308 as u32, s: 1247993134 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144137217 as u32, g: 320492023 as u32, s: 515841070 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144120833 as u32, g: 1639388522 as u32, s: 770877302 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144071681 as u32, g: 1761785233 as u32, s: 964296120 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144065537 as u32, g: 419817825 as u32, s: 204564472 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144028673 as u32, g: 666050597 as u32, s: 2091019760 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2144010241 as u32, g: 1413657615 as u32, s: 1518702610 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143952897 as u32, g: 1238327946 as u32, s: 475672271 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143940609 as u32, g: 307063413 as u32, s: 1176750846 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143918081 as u32, g: 2062905559 as u32, s: 786785803 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143899649 as u32, g: 1338112849 as u32, s: 1562292083 as u32 };
        init
    },
    {
        let mut init = small_prime { p: 2143891457 as u32, g: 68149545 as u32, s: 87166451 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143885313 as u32, g: 921750778 as u32, s: 394460854 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143854593 as u32, g: 719766593 as u32, s: 133877196 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143836161 as u32, g: 1149399850 as u32, s: 1861591875 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143762433 as u32, g: 1848739366 as u32, s: 1335934145 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143756289 as u32, g: 1326674710 as u32, s: 102999236 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143713281 as u32, g: 808061791 as u32, s: 1156900308 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143690753 as u32, g: 388399459 as u32, s: 1926468019 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143670273 as u32, g: 1427891374 as u32, s: 1756689401 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143666177 as u32, g: 1912173949 as u32, s: 986629565 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143645697 as u32, g: 2041160111 as u32, s: 371842865 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143641601 as u32, g: 1279906897 as u32, s: 2023974350 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143635457 as u32, g: 720473174 as u32, s: 1389027526 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143621121 as u32, g: 1298309455 as u32, s: 1732632006 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143598593 as u32, g: 1548762216 as u32, s: 1825417506 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143567873 as u32, g: 620475784 as u32, s: 1073787233 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143561729 as u32, g: 1932954575 as u32, s: 949167309 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143553537 as u32, g: 354315656 as u32, s: 1652037534 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143541249 as u32, g: 577424288 as u32, s: 1097027618 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143531009 as u32, g: 357862822 as u32, s: 478640055 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143522817 as u32, g: 2017706025 as u32, s: 1550531668 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143506433 as u32, g: 2078127419 as u32, s: 1824320165 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143488001 as u32, g: 613475285 as u32, s: 1604011510 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143469569 as u32, g: 1466594987 as u32, s: 502095196 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143426561 as u32, g: 1115430331 as u32, s: 1044637111 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143383553 as u32, g: 9778045 as u32, s: 1902463734 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143377409 as u32, g: 1557401276 as u32, s: 2056861771 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143363073 as u32, g: 652036455 as u32, s: 1965915971 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143260673 as u32, g: 1464581171 as u32, s: 1523257541 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143246337 as u32, g: 1876119649 as u32, s: 764541916 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143209473 as u32, g: 1614992673 as u32, s: 1920672844 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143203329 as u32, g: 981052047 as u32, s: 2049774209 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143160321 as u32, g: 1847355533 as u32, s: 728535665 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143129601 as u32, g: 965558457 as u32, s: 603052992 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143123457 as u32, g: 2140817191 as u32, s: 8348679 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143100929 as u32, g: 1547263683 as u32, s: 694209023 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143092737 as u32, g: 643459066 as u32, s: 1979934533 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143082497 as u32, g: 188603778 as u32, s: 2026175670 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143062017 as u32, g: 1657329695 as u32, s: 377451099 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143051777 as u32, g: 114967950 as u32, s: 979255473 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143025153 as u32, g: 1698431342 as u32, s: 1449196896 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2143006721 as u32, g: 1862741675 as u32, s: 1739650365 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142996481 as u32, g: 756660457 as u32, s: 996160050 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142976001 as u32, g: 927864010 as u32, s: 1166847574 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142965761 as u32, g: 905070557 as u32, s: 661974566 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142916609 as u32, g: 40932754 as u32, s: 1787161127 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142892033 as u32, g: 1987985648 as u32, s: 675335382 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142885889 as u32, g: 797497211 as u32, s: 1323096997 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142871553 as u32, g: 2068025830 as u32, s: 1411877159 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142861313 as u32, g: 1217177090 as u32, s: 1438410687 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142830593 as u32, g: 409906375 as u32, s: 1767860634 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142803969 as u32, g: 1197788993 as u32, s: 359782919 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142785537 as u32, g: 643817365 as u32, s: 513932862 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142779393 as u32, g: 1717046338 as u32, s: 218943121 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142724097 as u32, g: 89336830 as u32, s: 416687049 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142707713 as u32, g: 5944581 as u32, s: 1356813523 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142658561 as u32, g: 887942135 as u32, s: 2074011722 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142638081 as u32, g: 151851972 as u32, s: 1647339939 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142564353 as u32, g: 1691505537 as u32, s: 1483107336 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142533633 as u32, g: 1989920200 as u32, s: 1135938817 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142529537 as u32, g: 959263126 as u32, s: 1531961857 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142527489 as u32, g: 453251129 as u32, s: 1725566162 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142502913 as u32, g: 1536028102 as u32, s: 182053257 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142498817 as u32, g: 570138730 as u32, s: 701443447 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142416897 as u32, g: 326965800 as u32, s: 411931819 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142363649 as u32, g: 1675665410 as u32, s: 1517191733 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142351361 as u32, g: 968529566 as u32, s: 1575712703 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142330881 as u32, g: 1384953238 as u32, s: 1769087884 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142314497 as u32, g: 1977173242 as u32, s: 1833745524 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142289921 as u32, g: 95082313 as u32, s: 1714775493 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142283777 as u32, g: 109377615 as u32, s: 1070584533 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142277633 as u32, g: 16960510 as u32, s: 702157145 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142263297 as u32, g: 553850819 as u32, s: 431364395 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142208001 as u32, g: 241466367 as u32, s: 2053967982 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142164993 as u32, g: 1795661326 as u32, s: 1031836848 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142097409 as u32, g: 1212530046 as u32, s: 712772031 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142087169 as u32, g: 1763869720 as u32, s: 822276067 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142078977 as u32, g: 644065713 as u32, s: 1765268066 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142074881 as u32, g: 112671944 as u32, s: 643204925 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142044161 as u32, g: 1387785471 as u32, s: 1297890174 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142025729 as u32, g: 783885537 as u32, s: 1000425730 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2142011393 as u32, g: 905662232 as u32, s: 1679401033 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141974529 as u32, g: 799788433 as u32, s: 468119557 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141943809 as u32, g: 1932544124 as u32, s: 449305555 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141933569 as u32, g: 1527403256 as u32, s: 841867925 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141931521 as u32, g: 1247076451 as u32, s: 743823916 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141902849 as u32, g: 1199660531 as u32, s: 401687910 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141890561 as u32, g: 150132350 as u32, s: 1720336972 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141857793 as u32, g: 1287438162 as u32, s: 663880489 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141833217 as u32, g: 618017731 as u32, s: 1819208266 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141820929 as u32, g: 999578638 as u32, s: 1403090096 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141786113 as u32, g: 81834325 as u32, s: 1523542501 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141771777 as u32, g: 120001928 as u32, s: 463556492 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141759489 as u32, g: 122455485 as u32, s: 2124928282 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141749249 as u32, g: 141986041 as u32, s: 940339153 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141685761 as u32, g: 889088734 as u32, s: 477141499 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141673473 as u32, g: 324212681 as u32, s: 1122558298 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141669377 as u32, g: 1175806187 as u32, s: 1373818177 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141655041 as u32, g: 1113654822 as u32, s: 296887082 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141587457 as u32, g: 991103258 as u32, s: 1585913875 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141583361 as u32, g: 1401451409 as u32, s: 1802457360 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141575169 as u32, g: 1571977166 as u32, s: 712760980 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141546497 as u32, g: 1107849376 as u32, s: 1250270109 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141515777 as u32, g: 196544219 as u32, s: 356001130 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141495297 as u32, g: 1733571506 as u32, s: 1060744866 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141483009 as u32, g: 321552363 as u32, s: 1168297026 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141458433 as u32, g: 505818251 as u32, s: 733225819 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141360129 as u32, g: 1026840098 as u32, s: 948342276 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141325313 as u32, g: 945133744 as u32, s: 2129965998 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141317121 as u32, g: 1871100260 as u32, s: 1843844634 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141286401 as u32, g: 1790639498 as u32, s: 1750465696 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141267969 as u32, g: 1376858592 as u32, s: 186160720 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141255681 as u32, g: 2129698296 as u32, s: 1876677959 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141243393 as u32, g: 2138900688 as u32, s: 1340009628 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141214721 as u32, g: 1933049835 as u32, s: 1087819477 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141212673 as u32, g: 1898664939 as u32, s: 1786328049 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141202433 as u32, g: 990234828 as u32, s: 940682169 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141175809 as u32, g: 1406392421 as u32, s: 993089586 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141165569 as u32, g: 1263518371 as u32, s: 289019479 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141073409 as u32, g: 1485624211 as u32, s: 507864514 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141052929 as u32, g: 1885134788 as u32, s: 311252465 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141040641 as u32, g: 1285021247 as u32, s: 280941862 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141028353 as u32, g: 1527610374 as u32, s: 375035110 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2141011969 as u32, g: 1400626168 as u32, s: 164696620 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140999681 as u32, g: 632959608 as u32, s: 966175067 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140997633 as u32, g: 2045628978 as u32, s: 1290889438 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140993537 as u32, g: 1412755491 as u32, s: 375366253 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140942337 as u32, g: 719477232 as u32, s: 785367828 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140925953 as u32, g: 45224252 as u32, s: 836552317 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140917761 as u32, g: 1157376588 as u32, s: 1001839569 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140887041 as u32, g: 278480752 as u32, s: 2098732796 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140837889 as u32, g: 1663139953 as u32, s: 924094810 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140788737 as u32, g: 802501511 as u32, s: 2045368990 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140766209 as u32, g: 1820083885 as u32, s: 1800295504 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140764161 as u32, g: 1169561905 as u32, s: 2106792035 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140696577 as u32, g: 127781498 as u32, s: 1885987531 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140684289 as u32, g: 16014477 as u32, s: 1098116827 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140653569 as u32, g: 665960598 as u32, s: 1796728247 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140594177 as u32, g: 1043085491 as u32, s: 377310938 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140579841 as u32, g: 1732838211 as u32, s: 1504505945 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140569601 as u32, g: 302071939 as u32, s: 358291016 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140567553 as u32, g: 192393733 as u32, s: 1909137143 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140557313 as u32, g: 406595731 as u32, s: 1175330270 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140549121 as u32, g: 1748850918 as u32, s: 525007007 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140477441 as u32, g: 499436566 as u32, s: 1031159814 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140469249 as u32, g: 1886004401 as u32, s: 1029951320 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140426241 as u32, g: 1483168100 as u32, s: 1676273461 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140420097 as u32, g: 1779917297 as u32, s: 846024476 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140413953 as u32, g: 522948893 as u32, s: 1816354149 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140383233 as u32, g: 1931364473 as u32, s: 1296921241 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140366849 as u32, g: 1917356555 as u32, s: 147196204 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140354561 as u32, g: 16466177 as u32, s: 1349052107 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140348417 as u32, g: 1875366972 as u32, s: 1860485634 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140323841 as u32, g: 456498717 as u32, s: 1790256483 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140321793 as u32, g: 1629493973 as u32, s: 150031888 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140315649 as u32, g: 1904063898 as u32, s: 395510935 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140280833 as u32, g: 1784104328 as u32, s: 831417909 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140250113 as u32, g: 256087139 as u32, s: 697349101 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140229633 as u32, g: 388553070 as u32, s: 243875754 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140223489 as u32, g: 747459608 as u32, s: 1396270850 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140200961 as u32, g: 507423743 as u32, s: 1895572209 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140162049 as u32, g: 580106016 as u32, s: 2045297469 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140149761 as u32, g: 712426444 as u32, s: 785217995 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140137473 as u32, g: 1441607584 as u32, s: 536866543 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140119041 as u32, g: 346538902 as u32, s: 1740434653 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140090369 as u32, g: 282642885 as u32, s: 21051094 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140076033 as u32, g: 1407456228 as u32, s: 319910029 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140047361 as u32, g: 1619330500 as u32, s: 1488632070 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140041217 as u32, g: 2089408064 as u32, s: 2012026134 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2140008449 as u32, g: 1705524800 as u32, s: 1613440760 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139924481 as u32, g: 1846208233 as u32, s: 1280649481 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139906049 as u32, g: 989438755 as u32, s: 1185646076 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139867137 as u32, g: 1522314850 as u32, s: 372783595 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139842561 as u32, g: 1681587377 as u32, s: 216848235 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139826177 as u32, g: 2066284988 as u32, s: 1784999464 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139824129 as u32, g: 480888214 as u32, s: 1513323027 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139789313 as u32, g: 847937200 as u32, s: 858192859 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139783169 as u32, g: 1642000434 as u32, s: 1583261448 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139770881 as u32, g: 940699589 as u32, s: 179702100 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139768833 as u32, g: 315623242 as u32, s: 964612676 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139666433 as u32, g: 331649203 as u32, s: 764666914 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139641857 as u32, g: 2118730799 as u32, s: 1313764644 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139635713 as u32, g: 519149027 as u32, s: 519212449 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139598849 as u32, g: 1526413634 as u32, s: 1769667104 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139574273 as u32, g: 551148610 as u32, s: 820739925 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139568129 as u32, g: 1386800242 as u32, s: 472447405 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139549697 as u32, g: 813760130 as u32, s: 1412328531 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139537409 as u32, g: 1615286260 as u32, s: 1609362979 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139475969 as u32, g: 1352559299 as u32, s: 1696720421 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139455489 as u32, g: 1048691649 as u32, s: 1584935400 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139432961 as u32, g: 836025845 as u32, s: 950121150 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139424769 as u32, g: 1558281165 as u32, s: 1635486858 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139406337 as u32, g: 1728402143 as u32, s: 1674423301 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139396097 as u32, g: 1727715782 as u32, s: 1483470544 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139383809 as u32, g: 1092853491 as u32, s: 1741699084 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139369473 as u32, g: 690776899 as u32, s: 1242798709 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139351041 as u32, g: 1768782380 as u32, s: 2120712049 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139334657 as u32, g: 1739968247 as u32, s: 1427249225 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139332609 as u32, g: 1547189119 as u32, s: 623011170 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139310081 as u32, g: 1346827917 as u32, s: 1605466350 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139303937 as u32, g: 369317948 as u32, s: 828392831 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139301889 as u32, g: 1560417239 as u32, s: 1788073219 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139283457 as u32, g: 1303121623 as u32, s: 595079358 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139248641 as u32, g: 1354555286 as u32, s: 573424177 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139240449 as u32, g: 60974056 as u32, s: 885781403 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139222017 as u32, g: 355573421 as u32, s: 1221054839 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139215873 as u32, g: 566477826 as u32, s: 1724006500 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139150337 as u32, g: 871437673 as u32, s: 1609133294 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139144193 as u32, g: 1478130914 as u32, s: 1137491905 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139117569 as u32, g: 1854880922 as u32, s: 964728507 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139076609 as u32, g: 202405335 as u32, s: 756508944 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139062273 as u32, g: 1399715741 as u32, s: 884826059 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139045889 as u32, g: 1051045798 as u32, s: 1202295476 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139033601 as u32, g: 1707715206 as u32, s: 632234634 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2139006977 as u32, g: 2035853139 as u32, s: 231626690 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138951681 as u32, g: 183867876 as u32, s: 838350879 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138945537 as u32, g: 1403254661 as u32, s: 404460202 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138920961 as u32, g: 310865011 as u32, s: 1282911681 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138910721 as u32, g: 1328496553 as u32, s: 103472415 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138904577 as u32, g: 78831681 as u32, s: 993513549 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138902529 as u32, g: 1319697451 as u32, s: 1055904361 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138816513 as u32, g: 384338872 as u32, s: 1706202469 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138810369 as u32, g: 1084868275 as u32, s: 405677177 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138787841 as u32, g: 401181788 as u32, s: 1964773901 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138775553 as u32, g: 1850532988 as u32, s: 1247087473 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138767361 as u32, g: 874261901 as u32, s: 1576073565 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138757121 as u32, g: 1187474742 as u32, s: 993541415 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138748929 as u32, g: 1782458888 as u32, s: 1043206483 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138744833 as u32, g: 1221500487 as u32, s: 800141243 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138738689 as u32, g: 413465368 as u32, s: 1450660558 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138695681 as u32, g: 739045140 as u32, s: 342611472 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138658817 as u32, g: 1355845756 as u32, s: 672674190 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138644481 as u32, g: 608379162 as u32, s: 1538874380 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138632193 as u32, g: 1444914034 as u32, s: 686911254 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138607617 as u32, g: 484707818 as u32, s: 1435142134 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138591233 as u32, g: 539460669 as u32, s: 1290458549 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138572801 as u32, g: 2093538990 as u32, s: 2011138646 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138552321 as u32, g: 1149786988 as u32, s: 1076414907 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138546177 as u32, g: 840688206 as u32, s: 2108985273 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138533889 as u32, g: 209669619 as u32, s: 198172413 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138523649 as u32, g: 1975879426 as u32, s: 1277003968 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138490881 as u32, g: 1351891144 as u32, s: 1976858109 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138460161 as u32, g: 1817321013 as u32, s: 1979278293 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138429441 as u32, g: 1950077177 as u32, s: 203441928 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138400769 as u32, g: 908970113 as u32, s: 628395069 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138398721 as u32, g: 219890864 as u32, s: 758486760 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138376193 as u32, g: 1306654379 as u32, s: 977554090 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138351617 as u32, g: 298822498 as u32, s: 2004708503 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138337281 as u32, g: 441457816 as u32, s: 1049002108 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138320897 as u32, g: 1517731724 as u32, s: 1442269609 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138290177 as u32, g: 1355911197 as u32, s: 1647139103 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138234881 as u32, g: 531313247 as u32, s: 1746591962 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138214401 as u32, g: 1899410930 as u32, s: 781416444 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138202113 as u32, g: 1813477173 as u32, s: 1622508515 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138191873 as u32, g: 1086458299 as u32, s: 1025408615 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138183681 as u32, g: 1998800427 as u32, s: 827063290 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138173441 as u32, g: 1921308898 as u32, s: 749670117 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138103809 as u32, g: 1620902804 as u32, s: 2126787647 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138099713 as u32, g: 828647069 as u32, s: 1892961817 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138085377 as u32, g: 179405355 as u32, s: 1525506535 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138060801 as u32, g: 615683235 as u32, s: 1259580138 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138044417 as u32, g: 2030277840 as u32, s: 1731266562 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138042369 as u32, g: 2087222316 as u32, s: 1627902259 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138032129 as u32, g: 126388712 as u32, s: 1108640984 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2138011649 as u32, g: 715026550 as u32, s: 1017980050 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137993217 as u32, g: 1693714349 as u32, s: 1351778704 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137888769 as u32, g: 1289762259 as u32, s: 1053090405 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137853953 as u32, g: 199991890 as u32, s: 1254192789 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137833473 as u32, g: 941421685 as u32, s: 896995556 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137817089 as u32, g: 750416446 as u32, s: 1251031181 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137792513 as u32, g: 798075119 as u32, s: 368077456 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137786369 as u32, g: 878543495 as u32, s: 1035375025 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137767937 as u32, g: 9351178 as u32, s: 1156563902 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137755649 as u32, g: 1382297614 as u32, s: 1686559583 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137724929 as u32, g: 1345472850 as u32, s: 1681096331 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137704449 as u32, g: 834666929 as u32, s: 630551727 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137673729 as u32, g: 1646165729 as u32, s: 1892091571 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137620481 as u32, g: 778943821 as u32, s: 48456461 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137618433 as u32, g: 1730837875 as u32, s: 1713336725 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137581569 as u32, g: 805610339 as u32, s: 1378891359 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137538561 as u32, g: 204342388 as u32, s: 1950165220 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137526273 as u32, g: 1947629754 as u32, s: 1500789441 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137516033 as u32, g: 719902645 as u32, s: 1499525372 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137491457 as u32, g: 230451261 as u32, s: 556382829 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137440257 as u32, g: 979573541 as u32, s: 412760291 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137374721 as u32, g: 927841248 as u32, s: 1954137185 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137362433 as u32, g: 1243778559 as u32, s: 861024672 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137313281 as u32, g: 1341338501 as u32, s: 980638386 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137311233 as u32, g: 937415182 as u32, s: 1793212117 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137255937 as u32, g: 795331324 as u32, s: 1410253405 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137243649 as u32, g: 150756339 as u32, s: 1966999887 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137182209 as u32, g: 163346914 as u32, s: 1939301431 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137171969 as u32, g: 1952552395 as u32, s: 758913141 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137159681 as u32, g: 570788721 as u32, s: 218668666 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137147393 as u32, g: 1896656810 as u32, s: 2045670345 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137141249 as u32, g: 358493842 as u32, s: 518199643 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137139201 as u32, g: 1505023029 as u32, s: 674695848 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137133057 as u32, g: 27911103 as u32, s: 830956306 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137122817 as u32, g: 439771337 as u32, s: 1555268614 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137116673 as u32, g: 790988579 as u32, s: 1871449599 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137110529 as u32, g: 432109234 as u32, s: 811805080 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137102337 as u32, g: 1357900653 as u32, s: 1184997641 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137098241 as u32, g: 515119035 as u32, s: 1715693095 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137090049 as u32, g: 408575203 as u32, s: 2085660657 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137085953 as u32, g: 2097793407 as u32, s: 1349626963 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137055233 as u32, g: 1556739954 as u32, s: 1449960883 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2137030657 as u32, g: 1545758650 as u32, s: 1369303716 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136987649 as u32, g: 332602570 as u32, s: 103875114 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136969217 as u32, g: 1499989506 as u32, s: 1662964115 as u32 };
        init
    },
    {
        let mut init = small_prime { p: 2136924161 as u32, g: 857040753 as u32, s: 4738842 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136895489 as u32, g: 1948872712 as u32, s: 570436091 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136893441 as u32, g: 58969960 as u32, s: 1568349634 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136887297 as u32, g: 2127193379 as u32, s: 273612548 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136850433 as u32, g: 111208983 as u32, s: 1181257116 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136809473 as u32, g: 1627275942 as u32, s: 1680317971 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136764417 as u32, g: 1574888217 as u32, s: 14011331 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136741889 as u32, g: 14011055 as u32, s: 1129154251 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136727553 as u32, g: 35862563 as u32, s: 1838555253 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136721409 as u32, g: 310235666 as u32, s: 1363928244 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136698881 as u32, g: 1612429202 as u32, s: 1560383828 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136649729 as u32, g: 1138540131 as u32, s: 800014364 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136606721 as u32, g: 602323503 as u32, s: 1433096652 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136563713 as u32, g: 182209265 as u32, s: 1919611038 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136555521 as u32, g: 324156477 as u32, s: 165591039 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136549377 as u32, g: 195513113 as u32, s: 217165345 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136526849 as u32, g: 1050768046 as u32, s: 939647887 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136508417 as u32, g: 1886286237 as u32, s: 1619926572 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136477697 as u32, g: 609647664 as u32, s: 35065157 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136471553 as u32, g: 679352216 as u32, s: 1452259468 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136457217 as u32, g: 128630031 as u32, s: 824816521 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136422401 as u32, g: 19787464 as u32, s: 1526049830 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136420353 as u32, g: 698316836 as u32, s: 1530623527 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136371201 as u32, g: 1651862373 as u32, s: 1804812805 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136334337 as u32, g: 326596005 as u32, s: 336977082 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136322049 as u32, g: 63253370 as u32, s: 1904972151 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136297473 as u32, g: 312176076 as u32, s: 172182411 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136248321 as u32, g: 381261841 as u32, s: 369032670 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136242177 as u32, g: 358688773 as u32, s: 1640007994 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136229889 as u32, g: 512677188 as u32, s: 75585225 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136219649 as u32, g: 2095003250 as u32, s: 1970086149 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136207361 as u32, g: 1909650722 as u32, s: 537760675 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136176641 as u32, g: 1334616195 as u32, s: 1533487619 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136158209 as u32, g: 2096285632 as u32, s: 1793285210 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136143873 as u32, g: 1897347517 as u32, s: 293843959 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136133633 as u32, g: 923586222 as u32, s: 1022655978 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136096769 as u32, g: 1464868191 as u32, s: 1515074410 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136094721 as u32, g: 2020679520 as u32, s: 2061636104 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136076289 as u32, g: 290798503 as u32, s: 1814726809 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2136041473 as u32, g: 156415894 as u32, s: 1250757633 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2135996417 as u32, g: 297459940 as u32, s: 1132158924 as u32 };
        init
    },
    {
        let mut init =
            small_prime { p: 2135955457 as u32, g: 538755304 as u32, s: 1688831340 as u32 };
        init
    },
    {
        let mut init = small_prime { p: 0 as u32, g: 0 as u32, s: 0 as u32 };
        init
    },
];
#[inline]
unsafe extern "C" fn modp_set(mut x: int32_t, mut p: u32) -> u32 {
    let mut w: u32 = 0;
    w = x as u32;
    w = w.wrapping_add(p & (w >> 31).wrapping_neg());
    return w;
}
#[inline]
unsafe extern "C" fn modp_norm(mut x: u32, mut p: u32) -> int32_t {
    return x.wrapping_sub(
        p & (x.wrapping_sub(p.wrapping_add(1 as u32) >> 1) >> 31).wrapping_sub(1 as u32),
    ) as int32_t;
}
unsafe extern "C" fn modp_ninv31(mut p: u32) -> u32 {
    let mut y: u32 = 0;
    y = (2 as u32).wrapping_sub(p);
    y = y * (2 as u32).wrapping_sub(p * y);
    y = y * (2 as u32).wrapping_sub(p * y);
    y = y * (2 as u32).wrapping_sub(p * y);
    y = y * (2 as u32).wrapping_sub(p * y);
    return 0x7fffffff as u32 & y.wrapping_neg();
}
#[inline]
unsafe extern "C" fn modp_R(mut p: u32) -> u32 {
    return ((1 as u32) << 31).wrapping_sub(p);
}
#[inline]
unsafe extern "C" fn modp_add(mut a: u32, mut b: u32, mut p: u32) -> u32 {
    let mut d: u32 = 0;
    d = a.wrapping_add(b).wrapping_sub(p);
    d = d.wrapping_add(p & (d >> 31).wrapping_neg());
    return d;
}
#[inline]
unsafe extern "C" fn modp_sub(mut a: u32, mut b: u32, mut p: u32) -> u32 {
    let mut d: u32 = 0;
    d = a.wrapping_sub(b);
    d = d.wrapping_add(p & (d >> 31).wrapping_neg());
    return d;
}
#[inline]
unsafe extern "C" fn modp_montymul(mut a: u32, mut b: u32, mut p: u32, mut p0i: u32) -> u32 {
    let mut z: u64 = 0;
    let mut w: u64 = 0;
    let mut d: u32 = 0;
    z = a as u64 * b as u64;
    w = (z * p0i as u64 & 0x7fffffff as u64) * p as u64;
    d = ((z.wrapping_add(w) >> 31) as u32).wrapping_sub(p);
    d = d.wrapping_add(p & (d >> 31).wrapping_neg());
    return d;
}
unsafe extern "C" fn modp_R2(mut p: u32, mut p0i: u32) -> u32 {
    let mut z: u32 = 0;
    z = modp_R(p);
    z = modp_add(z, z, p);
    z = modp_montymul(z, z, p, p0i);
    z = modp_montymul(z, z, p, p0i);
    z = modp_montymul(z, z, p, p0i);
    z = modp_montymul(z, z, p, p0i);
    z = modp_montymul(z, z, p, p0i);
    z = z.wrapping_add(p & (z & 1 as u32).wrapping_neg()) >> 1;
    return z;
}
#[inline]
unsafe extern "C" fn modp_Rx(mut x: libc::c_uint, mut p: u32, mut p0i: u32, mut R2: u32) -> u32 {
    let mut i: libc::c_int = 0;
    let mut r: u32 = 0;
    let mut z: u32 = 0;
    x = x.wrapping_sub(1);
    x;
    r = R2;
    z = modp_R(p);
    i = 0;
    while (1 as libc::c_uint) << i <= x {
        if x & (1 as libc::c_uint) << i != 0 as libc::c_uint {
            z = modp_montymul(z, r, p, p0i);
        }
        r = modp_montymul(r, r, p, p0i);
        i += 1;
        i;
    }
    return z;
}
unsafe extern "C" fn modp_div(mut a: u32, mut b: u32, mut p: u32, mut p0i: u32, mut R: u32) -> u32 {
    let mut z: u32 = 0;
    let mut e: u32 = 0;
    let mut i: libc::c_int = 0;
    e = p.wrapping_sub(2 as u32);
    z = R;
    i = 30;
    while i >= 0 {
        let mut z2: u32 = 0;
        z = modp_montymul(z, z, p, p0i);
        z2 = modp_montymul(z, b, p, p0i);
        z ^= (z ^ z2) & (e >> i & 1 as u32).wrapping_neg();
        i -= 1;
        i;
    }
    z = modp_montymul(z, 1 as u32, p, p0i);
    return modp_montymul(a, z, p, p0i);
}
static mut REV10: [uint16_t; 1024] = [
    0 as uint16_t,
    512 as uint16_t,
    256 as uint16_t,
    768 as uint16_t,
    128 as uint16_t,
    640 as uint16_t,
    384 as uint16_t,
    896 as uint16_t,
    64 as uint16_t,
    576 as uint16_t,
    320 as uint16_t,
    832 as uint16_t,
    192 as uint16_t,
    704 as uint16_t,
    448 as uint16_t,
    960 as uint16_t,
    32 as uint16_t,
    544 as uint16_t,
    288 as uint16_t,
    800 as uint16_t,
    160 as uint16_t,
    672 as uint16_t,
    416 as uint16_t,
    928 as uint16_t,
    96 as uint16_t,
    608 as uint16_t,
    352 as uint16_t,
    864 as uint16_t,
    224 as uint16_t,
    736 as uint16_t,
    480 as uint16_t,
    992 as uint16_t,
    16 as uint16_t,
    528 as uint16_t,
    272 as uint16_t,
    784 as uint16_t,
    144 as uint16_t,
    656 as uint16_t,
    400 as uint16_t,
    912 as uint16_t,
    80 as uint16_t,
    592 as uint16_t,
    336 as uint16_t,
    848 as uint16_t,
    208 as uint16_t,
    720 as uint16_t,
    464 as uint16_t,
    976 as uint16_t,
    48 as uint16_t,
    560 as uint16_t,
    304 as uint16_t,
    816 as uint16_t,
    176 as uint16_t,
    688 as uint16_t,
    432 as uint16_t,
    944 as uint16_t,
    112 as uint16_t,
    624 as uint16_t,
    368 as uint16_t,
    880 as uint16_t,
    240 as uint16_t,
    752 as uint16_t,
    496 as uint16_t,
    1008 as uint16_t,
    8 as uint16_t,
    520 as uint16_t,
    264 as uint16_t,
    776 as uint16_t,
    136 as uint16_t,
    648 as uint16_t,
    392 as uint16_t,
    904 as uint16_t,
    72 as uint16_t,
    584 as uint16_t,
    328 as uint16_t,
    840 as uint16_t,
    200 as uint16_t,
    712 as uint16_t,
    456 as uint16_t,
    968 as uint16_t,
    40 as uint16_t,
    552 as uint16_t,
    296 as uint16_t,
    808 as uint16_t,
    168 as uint16_t,
    680 as uint16_t,
    424 as uint16_t,
    936 as uint16_t,
    104 as uint16_t,
    616 as uint16_t,
    360 as uint16_t,
    872 as uint16_t,
    232 as uint16_t,
    744 as uint16_t,
    488 as uint16_t,
    1000 as uint16_t,
    24 as uint16_t,
    536 as uint16_t,
    280 as uint16_t,
    792 as uint16_t,
    152 as uint16_t,
    664 as uint16_t,
    408 as uint16_t,
    920 as uint16_t,
    88 as uint16_t,
    600 as uint16_t,
    344 as uint16_t,
    856 as uint16_t,
    216 as uint16_t,
    728 as uint16_t,
    472 as uint16_t,
    984 as uint16_t,
    56 as uint16_t,
    568 as uint16_t,
    312 as uint16_t,
    824 as uint16_t,
    184 as uint16_t,
    696 as uint16_t,
    440 as uint16_t,
    952 as uint16_t,
    120 as uint16_t,
    632 as uint16_t,
    376 as uint16_t,
    888 as uint16_t,
    248 as uint16_t,
    760 as uint16_t,
    504 as uint16_t,
    1016 as uint16_t,
    4 as uint16_t,
    516 as uint16_t,
    260 as uint16_t,
    772 as uint16_t,
    132 as uint16_t,
    644 as uint16_t,
    388 as uint16_t,
    900 as uint16_t,
    68 as uint16_t,
    580 as uint16_t,
    324 as uint16_t,
    836 as uint16_t,
    196 as uint16_t,
    708 as uint16_t,
    452 as uint16_t,
    964 as uint16_t,
    36 as uint16_t,
    548 as uint16_t,
    292 as uint16_t,
    804 as uint16_t,
    164 as uint16_t,
    676 as uint16_t,
    420 as uint16_t,
    932 as uint16_t,
    100 as uint16_t,
    612 as uint16_t,
    356 as uint16_t,
    868 as uint16_t,
    228 as uint16_t,
    740 as uint16_t,
    484 as uint16_t,
    996 as uint16_t,
    20 as uint16_t,
    532 as uint16_t,
    276 as uint16_t,
    788 as uint16_t,
    148 as uint16_t,
    660 as uint16_t,
    404 as uint16_t,
    916 as uint16_t,
    84 as uint16_t,
    596 as uint16_t,
    340 as uint16_t,
    852 as uint16_t,
    212 as uint16_t,
    724 as uint16_t,
    468 as uint16_t,
    980 as uint16_t,
    52 as uint16_t,
    564 as uint16_t,
    308 as uint16_t,
    820 as uint16_t,
    180 as uint16_t,
    692 as uint16_t,
    436 as uint16_t,
    948 as uint16_t,
    116 as uint16_t,
    628 as uint16_t,
    372 as uint16_t,
    884 as uint16_t,
    244 as uint16_t,
    756 as uint16_t,
    500 as uint16_t,
    1012 as uint16_t,
    12 as uint16_t,
    524 as uint16_t,
    268 as uint16_t,
    780 as uint16_t,
    140 as uint16_t,
    652 as uint16_t,
    396 as uint16_t,
    908 as uint16_t,
    76 as uint16_t,
    588 as uint16_t,
    332 as uint16_t,
    844 as uint16_t,
    204 as uint16_t,
    716 as uint16_t,
    460 as uint16_t,
    972 as uint16_t,
    44 as uint16_t,
    556 as uint16_t,
    300 as uint16_t,
    812 as uint16_t,
    172 as uint16_t,
    684 as uint16_t,
    428 as uint16_t,
    940 as uint16_t,
    108 as uint16_t,
    620 as uint16_t,
    364 as uint16_t,
    876 as uint16_t,
    236 as uint16_t,
    748 as uint16_t,
    492 as uint16_t,
    1004 as uint16_t,
    28 as uint16_t,
    540 as uint16_t,
    284 as uint16_t,
    796 as uint16_t,
    156 as uint16_t,
    668 as uint16_t,
    412 as uint16_t,
    924 as uint16_t,
    92 as uint16_t,
    604 as uint16_t,
    348 as uint16_t,
    860 as uint16_t,
    220 as uint16_t,
    732 as uint16_t,
    476 as uint16_t,
    988 as uint16_t,
    60 as uint16_t,
    572 as uint16_t,
    316 as uint16_t,
    828 as uint16_t,
    188 as uint16_t,
    700 as uint16_t,
    444 as uint16_t,
    956 as uint16_t,
    124 as uint16_t,
    636 as uint16_t,
    380 as uint16_t,
    892 as uint16_t,
    252 as uint16_t,
    764 as uint16_t,
    508 as uint16_t,
    1020 as uint16_t,
    2 as uint16_t,
    514 as uint16_t,
    258 as uint16_t,
    770 as uint16_t,
    130 as uint16_t,
    642 as uint16_t,
    386 as uint16_t,
    898 as uint16_t,
    66 as uint16_t,
    578 as uint16_t,
    322 as uint16_t,
    834 as uint16_t,
    194 as uint16_t,
    706 as uint16_t,
    450 as uint16_t,
    962 as uint16_t,
    34 as uint16_t,
    546 as uint16_t,
    290 as uint16_t,
    802 as uint16_t,
    162 as uint16_t,
    674 as uint16_t,
    418 as uint16_t,
    930 as uint16_t,
    98 as uint16_t,
    610 as uint16_t,
    354 as uint16_t,
    866 as uint16_t,
    226 as uint16_t,
    738 as uint16_t,
    482 as uint16_t,
    994 as uint16_t,
    18 as uint16_t,
    530 as uint16_t,
    274 as uint16_t,
    786 as uint16_t,
    146 as uint16_t,
    658 as uint16_t,
    402 as uint16_t,
    914 as uint16_t,
    82 as uint16_t,
    594 as uint16_t,
    338 as uint16_t,
    850 as uint16_t,
    210 as uint16_t,
    722 as uint16_t,
    466 as uint16_t,
    978 as uint16_t,
    50 as uint16_t,
    562 as uint16_t,
    306 as uint16_t,
    818 as uint16_t,
    178 as uint16_t,
    690 as uint16_t,
    434 as uint16_t,
    946 as uint16_t,
    114 as uint16_t,
    626 as uint16_t,
    370 as uint16_t,
    882 as uint16_t,
    242 as uint16_t,
    754 as uint16_t,
    498 as uint16_t,
    1010 as uint16_t,
    10 as uint16_t,
    522 as uint16_t,
    266 as uint16_t,
    778 as uint16_t,
    138 as uint16_t,
    650 as uint16_t,
    394 as uint16_t,
    906 as uint16_t,
    74 as uint16_t,
    586 as uint16_t,
    330 as uint16_t,
    842 as uint16_t,
    202 as uint16_t,
    714 as uint16_t,
    458 as uint16_t,
    970 as uint16_t,
    42 as uint16_t,
    554 as uint16_t,
    298 as uint16_t,
    810 as uint16_t,
    170 as uint16_t,
    682 as uint16_t,
    426 as uint16_t,
    938 as uint16_t,
    106 as uint16_t,
    618 as uint16_t,
    362 as uint16_t,
    874 as uint16_t,
    234 as uint16_t,
    746 as uint16_t,
    490 as uint16_t,
    1002 as uint16_t,
    26 as uint16_t,
    538 as uint16_t,
    282 as uint16_t,
    794 as uint16_t,
    154 as uint16_t,
    666 as uint16_t,
    410 as uint16_t,
    922 as uint16_t,
    90 as uint16_t,
    602 as uint16_t,
    346 as uint16_t,
    858 as uint16_t,
    218 as uint16_t,
    730 as uint16_t,
    474 as uint16_t,
    986 as uint16_t,
    58 as uint16_t,
    570 as uint16_t,
    314 as uint16_t,
    826 as uint16_t,
    186 as uint16_t,
    698 as uint16_t,
    442 as uint16_t,
    954 as uint16_t,
    122 as uint16_t,
    634 as uint16_t,
    378 as uint16_t,
    890 as uint16_t,
    250 as uint16_t,
    762 as uint16_t,
    506 as uint16_t,
    1018 as uint16_t,
    6 as uint16_t,
    518 as uint16_t,
    262 as uint16_t,
    774 as uint16_t,
    134 as uint16_t,
    646 as uint16_t,
    390 as uint16_t,
    902 as uint16_t,
    70 as uint16_t,
    582 as uint16_t,
    326 as uint16_t,
    838 as uint16_t,
    198 as uint16_t,
    710 as uint16_t,
    454 as uint16_t,
    966 as uint16_t,
    38 as uint16_t,
    550 as uint16_t,
    294 as uint16_t,
    806 as uint16_t,
    166 as uint16_t,
    678 as uint16_t,
    422 as uint16_t,
    934 as uint16_t,
    102 as uint16_t,
    614 as uint16_t,
    358 as uint16_t,
    870 as uint16_t,
    230 as uint16_t,
    742 as uint16_t,
    486 as uint16_t,
    998 as uint16_t,
    22 as uint16_t,
    534 as uint16_t,
    278 as uint16_t,
    790 as uint16_t,
    150 as uint16_t,
    662 as uint16_t,
    406 as uint16_t,
    918 as uint16_t,
    86 as uint16_t,
    598 as uint16_t,
    342 as uint16_t,
    854 as uint16_t,
    214 as uint16_t,
    726 as uint16_t,
    470 as uint16_t,
    982 as uint16_t,
    54 as uint16_t,
    566 as uint16_t,
    310 as uint16_t,
    822 as uint16_t,
    182 as uint16_t,
    694 as uint16_t,
    438 as uint16_t,
    950 as uint16_t,
    118 as uint16_t,
    630 as uint16_t,
    374 as uint16_t,
    886 as uint16_t,
    246 as uint16_t,
    758 as uint16_t,
    502 as uint16_t,
    1014 as uint16_t,
    14 as uint16_t,
    526 as uint16_t,
    270 as uint16_t,
    782 as uint16_t,
    142 as uint16_t,
    654 as uint16_t,
    398 as uint16_t,
    910 as uint16_t,
    78 as uint16_t,
    590 as uint16_t,
    334 as uint16_t,
    846 as uint16_t,
    206 as uint16_t,
    718 as uint16_t,
    462 as uint16_t,
    974 as uint16_t,
    46 as uint16_t,
    558 as uint16_t,
    302 as uint16_t,
    814 as uint16_t,
    174 as uint16_t,
    686 as uint16_t,
    430 as uint16_t,
    942 as uint16_t,
    110 as uint16_t,
    622 as uint16_t,
    366 as uint16_t,
    878 as uint16_t,
    238 as uint16_t,
    750 as uint16_t,
    494 as uint16_t,
    1006 as uint16_t,
    30 as uint16_t,
    542 as uint16_t,
    286 as uint16_t,
    798 as uint16_t,
    158 as uint16_t,
    670 as uint16_t,
    414 as uint16_t,
    926 as uint16_t,
    94 as uint16_t,
    606 as uint16_t,
    350 as uint16_t,
    862 as uint16_t,
    222 as uint16_t,
    734 as uint16_t,
    478 as uint16_t,
    990 as uint16_t,
    62 as uint16_t,
    574 as uint16_t,
    318 as uint16_t,
    830 as uint16_t,
    190 as uint16_t,
    702 as uint16_t,
    446 as uint16_t,
    958 as uint16_t,
    126 as uint16_t,
    638 as uint16_t,
    382 as uint16_t,
    894 as uint16_t,
    254 as uint16_t,
    766 as uint16_t,
    510 as uint16_t,
    1022 as uint16_t,
    1 as uint16_t,
    513 as uint16_t,
    257 as uint16_t,
    769 as uint16_t,
    129 as uint16_t,
    641 as uint16_t,
    385 as uint16_t,
    897 as uint16_t,
    65 as uint16_t,
    577 as uint16_t,
    321 as uint16_t,
    833 as uint16_t,
    193 as uint16_t,
    705 as uint16_t,
    449 as uint16_t,
    961 as uint16_t,
    33 as uint16_t,
    545 as uint16_t,
    289 as uint16_t,
    801 as uint16_t,
    161 as uint16_t,
    673 as uint16_t,
    417 as uint16_t,
    929 as uint16_t,
    97 as uint16_t,
    609 as uint16_t,
    353 as uint16_t,
    865 as uint16_t,
    225 as uint16_t,
    737 as uint16_t,
    481 as uint16_t,
    993 as uint16_t,
    17 as uint16_t,
    529 as uint16_t,
    273 as uint16_t,
    785 as uint16_t,
    145 as uint16_t,
    657 as uint16_t,
    401 as uint16_t,
    913 as uint16_t,
    81 as uint16_t,
    593 as uint16_t,
    337 as uint16_t,
    849 as uint16_t,
    209 as uint16_t,
    721 as uint16_t,
    465 as uint16_t,
    977 as uint16_t,
    49 as uint16_t,
    561 as uint16_t,
    305 as uint16_t,
    817 as uint16_t,
    177 as uint16_t,
    689 as uint16_t,
    433 as uint16_t,
    945 as uint16_t,
    113 as uint16_t,
    625 as uint16_t,
    369 as uint16_t,
    881 as uint16_t,
    241 as uint16_t,
    753 as uint16_t,
    497 as uint16_t,
    1009 as uint16_t,
    9 as uint16_t,
    521 as uint16_t,
    265 as uint16_t,
    777 as uint16_t,
    137 as uint16_t,
    649 as uint16_t,
    393 as uint16_t,
    905 as uint16_t,
    73 as uint16_t,
    585 as uint16_t,
    329 as uint16_t,
    841 as uint16_t,
    201 as uint16_t,
    713 as uint16_t,
    457 as uint16_t,
    969 as uint16_t,
    41 as uint16_t,
    553 as uint16_t,
    297 as uint16_t,
    809 as uint16_t,
    169 as uint16_t,
    681 as uint16_t,
    425 as uint16_t,
    937 as uint16_t,
    105 as uint16_t,
    617 as uint16_t,
    361 as uint16_t,
    873 as uint16_t,
    233 as uint16_t,
    745 as uint16_t,
    489 as uint16_t,
    1001 as uint16_t,
    25 as uint16_t,
    537 as uint16_t,
    281 as uint16_t,
    793 as uint16_t,
    153 as uint16_t,
    665 as uint16_t,
    409 as uint16_t,
    921 as uint16_t,
    89 as uint16_t,
    601 as uint16_t,
    345 as uint16_t,
    857 as uint16_t,
    217 as uint16_t,
    729 as uint16_t,
    473 as uint16_t,
    985 as uint16_t,
    57 as uint16_t,
    569 as uint16_t,
    313 as uint16_t,
    825 as uint16_t,
    185 as uint16_t,
    697 as uint16_t,
    441 as uint16_t,
    953 as uint16_t,
    121 as uint16_t,
    633 as uint16_t,
    377 as uint16_t,
    889 as uint16_t,
    249 as uint16_t,
    761 as uint16_t,
    505 as uint16_t,
    1017 as uint16_t,
    5 as uint16_t,
    517 as uint16_t,
    261 as uint16_t,
    773 as uint16_t,
    133 as uint16_t,
    645 as uint16_t,
    389 as uint16_t,
    901 as uint16_t,
    69 as uint16_t,
    581 as uint16_t,
    325 as uint16_t,
    837 as uint16_t,
    197 as uint16_t,
    709 as uint16_t,
    453 as uint16_t,
    965 as uint16_t,
    37 as uint16_t,
    549 as uint16_t,
    293 as uint16_t,
    805 as uint16_t,
    165 as uint16_t,
    677 as uint16_t,
    421 as uint16_t,
    933 as uint16_t,
    101 as uint16_t,
    613 as uint16_t,
    357 as uint16_t,
    869 as uint16_t,
    229 as uint16_t,
    741 as uint16_t,
    485 as uint16_t,
    997 as uint16_t,
    21 as uint16_t,
    533 as uint16_t,
    277 as uint16_t,
    789 as uint16_t,
    149 as uint16_t,
    661 as uint16_t,
    405 as uint16_t,
    917 as uint16_t,
    85 as uint16_t,
    597 as uint16_t,
    341 as uint16_t,
    853 as uint16_t,
    213 as uint16_t,
    725 as uint16_t,
    469 as uint16_t,
    981 as uint16_t,
    53 as uint16_t,
    565 as uint16_t,
    309 as uint16_t,
    821 as uint16_t,
    181 as uint16_t,
    693 as uint16_t,
    437 as uint16_t,
    949 as uint16_t,
    117 as uint16_t,
    629 as uint16_t,
    373 as uint16_t,
    885 as uint16_t,
    245 as uint16_t,
    757 as uint16_t,
    501 as uint16_t,
    1013 as uint16_t,
    13 as uint16_t,
    525 as uint16_t,
    269 as uint16_t,
    781 as uint16_t,
    141 as uint16_t,
    653 as uint16_t,
    397 as uint16_t,
    909 as uint16_t,
    77 as uint16_t,
    589 as uint16_t,
    333 as uint16_t,
    845 as uint16_t,
    205 as uint16_t,
    717 as uint16_t,
    461 as uint16_t,
    973 as uint16_t,
    45 as uint16_t,
    557 as uint16_t,
    301 as uint16_t,
    813 as uint16_t,
    173 as uint16_t,
    685 as uint16_t,
    429 as uint16_t,
    941 as uint16_t,
    109 as uint16_t,
    621 as uint16_t,
    365 as uint16_t,
    877 as uint16_t,
    237 as uint16_t,
    749 as uint16_t,
    493 as uint16_t,
    1005 as uint16_t,
    29 as uint16_t,
    541 as uint16_t,
    285 as uint16_t,
    797 as uint16_t,
    157 as uint16_t,
    669 as uint16_t,
    413 as uint16_t,
    925 as uint16_t,
    93 as uint16_t,
    605 as uint16_t,
    349 as uint16_t,
    861 as uint16_t,
    221 as uint16_t,
    733 as uint16_t,
    477 as uint16_t,
    989 as uint16_t,
    61 as uint16_t,
    573 as uint16_t,
    317 as uint16_t,
    829 as uint16_t,
    189 as uint16_t,
    701 as uint16_t,
    445 as uint16_t,
    957 as uint16_t,
    125 as uint16_t,
    637 as uint16_t,
    381 as uint16_t,
    893 as uint16_t,
    253 as uint16_t,
    765 as uint16_t,
    509 as uint16_t,
    1021 as uint16_t,
    3 as uint16_t,
    515 as uint16_t,
    259 as uint16_t,
    771 as uint16_t,
    131 as uint16_t,
    643 as uint16_t,
    387 as uint16_t,
    899 as uint16_t,
    67 as uint16_t,
    579 as uint16_t,
    323 as uint16_t,
    835 as uint16_t,
    195 as uint16_t,
    707 as uint16_t,
    451 as uint16_t,
    963 as uint16_t,
    35 as uint16_t,
    547 as uint16_t,
    291 as uint16_t,
    803 as uint16_t,
    163 as uint16_t,
    675 as uint16_t,
    419 as uint16_t,
    931 as uint16_t,
    99 as uint16_t,
    611 as uint16_t,
    355 as uint16_t,
    867 as uint16_t,
    227 as uint16_t,
    739 as uint16_t,
    483 as uint16_t,
    995 as uint16_t,
    19 as uint16_t,
    531 as uint16_t,
    275 as uint16_t,
    787 as uint16_t,
    147 as uint16_t,
    659 as uint16_t,
    403 as uint16_t,
    915 as uint16_t,
    83 as uint16_t,
    595 as uint16_t,
    339 as uint16_t,
    851 as uint16_t,
    211 as uint16_t,
    723 as uint16_t,
    467 as uint16_t,
    979 as uint16_t,
    51 as uint16_t,
    563 as uint16_t,
    307 as uint16_t,
    819 as uint16_t,
    179 as uint16_t,
    691 as uint16_t,
    435 as uint16_t,
    947 as uint16_t,
    115 as uint16_t,
    627 as uint16_t,
    371 as uint16_t,
    883 as uint16_t,
    243 as uint16_t,
    755 as uint16_t,
    499 as uint16_t,
    1011 as uint16_t,
    11 as uint16_t,
    523 as uint16_t,
    267 as uint16_t,
    779 as uint16_t,
    139 as uint16_t,
    651 as uint16_t,
    395 as uint16_t,
    907 as uint16_t,
    75 as uint16_t,
    587 as uint16_t,
    331 as uint16_t,
    843 as uint16_t,
    203 as uint16_t,
    715 as uint16_t,
    459 as uint16_t,
    971 as uint16_t,
    43 as uint16_t,
    555 as uint16_t,
    299 as uint16_t,
    811 as uint16_t,
    171 as uint16_t,
    683 as uint16_t,
    427 as uint16_t,
    939 as uint16_t,
    107 as uint16_t,
    619 as uint16_t,
    363 as uint16_t,
    875 as uint16_t,
    235 as uint16_t,
    747 as uint16_t,
    491 as uint16_t,
    1003 as uint16_t,
    27 as uint16_t,
    539 as uint16_t,
    283 as uint16_t,
    795 as uint16_t,
    155 as uint16_t,
    667 as uint16_t,
    411 as uint16_t,
    923 as uint16_t,
    91 as uint16_t,
    603 as uint16_t,
    347 as uint16_t,
    859 as uint16_t,
    219 as uint16_t,
    731 as uint16_t,
    475 as uint16_t,
    987 as uint16_t,
    59 as uint16_t,
    571 as uint16_t,
    315 as uint16_t,
    827 as uint16_t,
    187 as uint16_t,
    699 as uint16_t,
    443 as uint16_t,
    955 as uint16_t,
    123 as uint16_t,
    635 as uint16_t,
    379 as uint16_t,
    891 as uint16_t,
    251 as uint16_t,
    763 as uint16_t,
    507 as uint16_t,
    1019 as uint16_t,
    7 as uint16_t,
    519 as uint16_t,
    263 as uint16_t,
    775 as uint16_t,
    135 as uint16_t,
    647 as uint16_t,
    391 as uint16_t,
    903 as uint16_t,
    71 as uint16_t,
    583 as uint16_t,
    327 as uint16_t,
    839 as uint16_t,
    199 as uint16_t,
    711 as uint16_t,
    455 as uint16_t,
    967 as uint16_t,
    39 as uint16_t,
    551 as uint16_t,
    295 as uint16_t,
    807 as uint16_t,
    167 as uint16_t,
    679 as uint16_t,
    423 as uint16_t,
    935 as uint16_t,
    103 as uint16_t,
    615 as uint16_t,
    359 as uint16_t,
    871 as uint16_t,
    231 as uint16_t,
    743 as uint16_t,
    487 as uint16_t,
    999 as uint16_t,
    23 as uint16_t,
    535 as uint16_t,
    279 as uint16_t,
    791 as uint16_t,
    151 as uint16_t,
    663 as uint16_t,
    407 as uint16_t,
    919 as uint16_t,
    87 as uint16_t,
    599 as uint16_t,
    343 as uint16_t,
    855 as uint16_t,
    215 as uint16_t,
    727 as uint16_t,
    471 as uint16_t,
    983 as uint16_t,
    55 as uint16_t,
    567 as uint16_t,
    311 as uint16_t,
    823 as uint16_t,
    183 as uint16_t,
    695 as uint16_t,
    439 as uint16_t,
    951 as uint16_t,
    119 as uint16_t,
    631 as uint16_t,
    375 as uint16_t,
    887 as uint16_t,
    247 as uint16_t,
    759 as uint16_t,
    503 as uint16_t,
    1015 as uint16_t,
    15 as uint16_t,
    527 as uint16_t,
    271 as uint16_t,
    783 as uint16_t,
    143 as uint16_t,
    655 as uint16_t,
    399 as uint16_t,
    911 as uint16_t,
    79 as uint16_t,
    591 as uint16_t,
    335 as uint16_t,
    847 as uint16_t,
    207 as uint16_t,
    719 as uint16_t,
    463 as uint16_t,
    975 as uint16_t,
    47 as uint16_t,
    559 as uint16_t,
    303 as uint16_t,
    815 as uint16_t,
    175 as uint16_t,
    687 as uint16_t,
    431 as uint16_t,
    943 as uint16_t,
    111 as uint16_t,
    623 as uint16_t,
    367 as uint16_t,
    879 as uint16_t,
    239 as uint16_t,
    751 as uint16_t,
    495 as uint16_t,
    1007 as uint16_t,
    31 as uint16_t,
    543 as uint16_t,
    287 as uint16_t,
    799 as uint16_t,
    159 as uint16_t,
    671 as uint16_t,
    415 as uint16_t,
    927 as uint16_t,
    95 as uint16_t,
    607 as uint16_t,
    351 as uint16_t,
    863 as uint16_t,
    223 as uint16_t,
    735 as uint16_t,
    479 as uint16_t,
    991 as uint16_t,
    63 as uint16_t,
    575 as uint16_t,
    319 as uint16_t,
    831 as uint16_t,
    191 as uint16_t,
    703 as uint16_t,
    447 as uint16_t,
    959 as uint16_t,
    127 as uint16_t,
    639 as uint16_t,
    383 as uint16_t,
    895 as uint16_t,
    255 as uint16_t,
    767 as uint16_t,
    511 as uint16_t,
    1023 as uint16_t,
];
unsafe extern "C" fn modp_mkgm2(
    mut gm: *mut u32,
    mut igm: *mut u32,
    mut logn: libc::c_uint,
    mut g: u32,
    mut p: u32,
    mut p0i: u32,
) {
    let mut u: size_t = 0;
    let mut n: size_t = 0;
    let mut k: libc::c_uint = 0;
    let mut ig: u32 = 0;
    let mut x1: u32 = 0;
    let mut x2: u32 = 0;
    let mut R2: u32 = 0;
    n = (1 as size_t) << logn;
    R2 = modp_R2(p, p0i);
    g = modp_montymul(g, R2, p, p0i);
    k = logn;
    while k < 10 as libc::c_uint {
        g = modp_montymul(g, g, p, p0i);
        k = k.wrapping_add(1);
        k;
    }
    ig = modp_div(R2, g, p, p0i, modp_R(p));
    k = (10 as libc::c_uint).wrapping_sub(logn);
    x2 = modp_R(p);
    x1 = x2;
    u = 0 as size_t;
    while u < n {
        let mut v: size_t = 0;
        v = REV10[(u << k) as usize] as size_t;
        *gm.offset(v as isize) = x1;
        *igm.offset(v as isize) = x2;
        x1 = modp_montymul(x1, g, p, p0i);
        x2 = modp_montymul(x2, ig, p, p0i);
        u = u.wrapping_add(1);
        u;
    }
}
unsafe extern "C" fn modp_NTT2_ext(
    mut a: *mut u32,
    mut stride: size_t,
    mut gm: *const u32,
    mut logn: libc::c_uint,
    mut p: u32,
    mut p0i: u32,
) {
    let mut t: size_t = 0;
    let mut m: size_t = 0;
    let mut n: size_t = 0;
    if logn == 0 as libc::c_uint {
        return;
    }
    n = (1 as size_t) << logn;
    t = n;
    m = 1 as size_t;
    while m < n {
        let mut ht: size_t = 0;
        let mut u: size_t = 0;
        let mut v1: size_t = 0;
        ht = t >> 1;
        u = 0 as size_t;
        v1 = 0 as size_t;
        while u < m {
            let mut s: u32 = 0;
            let mut v: size_t = 0;
            let mut r1: *mut u32 = 0 as *mut u32;
            let mut r2: *mut u32 = 0 as *mut u32;
            s = *gm.offset(m.wrapping_add(u) as isize);
            r1 = a.offset((v1 * stride) as isize);
            r2 = r1.offset((ht * stride) as isize);
            v = 0 as size_t;
            while v < ht {
                let mut x: u32 = 0;
                let mut y: u32 = 0;
                x = *r1;
                y = modp_montymul(*r2, s, p, p0i);
                *r1 = modp_add(x, y, p);
                *r2 = modp_sub(x, y, p);
                v = v.wrapping_add(1);
                v;
                r1 = r1.offset(stride as isize);
                r2 = r2.offset(stride as isize);
            }
            u = u.wrapping_add(1);
            u;
            v1 = v1.wrapping_add(t);
        }
        t = ht;
        m <<= 1;
    }
}
unsafe extern "C" fn modp_iNTT2_ext(
    mut a: *mut u32,
    mut stride: size_t,
    mut igm: *const u32,
    mut logn: libc::c_uint,
    mut p: u32,
    mut p0i: u32,
) {
    let mut t: size_t = 0;
    let mut m: size_t = 0;
    let mut n: size_t = 0;
    let mut k: size_t = 0;
    let mut ni: u32 = 0;
    let mut r: *mut u32 = 0 as *mut u32;
    if logn == 0 as libc::c_uint {
        return;
    }
    n = (1 as size_t) << logn;
    t = 1 as size_t;
    m = n;
    while m > 1 as size_t {
        let mut hm: size_t = 0;
        let mut dt: size_t = 0;
        let mut u: size_t = 0;
        let mut v1: size_t = 0;
        hm = m >> 1;
        dt = t << 1;
        u = 0 as size_t;
        v1 = 0 as size_t;
        while u < hm {
            let mut s: u32 = 0;
            let mut v: size_t = 0;
            let mut r1: *mut u32 = 0 as *mut u32;
            let mut r2: *mut u32 = 0 as *mut u32;
            s = *igm.offset(hm.wrapping_add(u) as isize);
            r1 = a.offset((v1 * stride) as isize);
            r2 = r1.offset((t * stride) as isize);
            v = 0 as size_t;
            while v < t {
                let mut x: u32 = 0;
                let mut y: u32 = 0;
                x = *r1;
                y = *r2;
                *r1 = modp_add(x, y, p);
                *r2 = modp_montymul(modp_sub(x, y, p), s, p, p0i);
                v = v.wrapping_add(1);
                v;
                r1 = r1.offset(stride as isize);
                r2 = r2.offset(stride as isize);
            }
            u = u.wrapping_add(1);
            u;
            v1 = v1.wrapping_add(dt);
        }
        t = dt;
        m >>= 1;
    }
    ni = (1 as u32) << (31 as libc::c_uint).wrapping_sub(logn);
    k = 0 as size_t;
    r = a;
    while k < n {
        *r = modp_montymul(*r, ni, p, p0i);
        k = k.wrapping_add(1);
        k;
        r = r.offset(stride as isize);
    }
}
unsafe extern "C" fn modp_poly_rec_res(
    mut f: *mut u32,
    mut logn: libc::c_uint,
    mut p: u32,
    mut p0i: u32,
    mut R2: u32,
) {
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    hn = (1 as size_t) << logn.wrapping_sub(1 as libc::c_uint);
    u = 0 as size_t;
    while u < hn {
        let mut w0: u32 = 0;
        let mut w1: u32 = 0;
        w0 = *f.offset((u << 1).wrapping_add(0 as size_t) as isize);
        w1 = *f.offset((u << 1).wrapping_add(1 as size_t) as isize);
        *f.offset(u as isize) = modp_montymul(modp_montymul(w0, w1, p, p0i), R2, p, p0i);
        u = u.wrapping_add(1);
        u;
    }
}
unsafe extern "C" fn zint_sub(
    mut a: *mut u32,
    mut b: *const u32,
    mut len: size_t,
    mut ctl: u32,
) -> u32 {
    let mut u: size_t = 0;
    let mut cc: u32 = 0;
    let mut m: u32 = 0;
    cc = 0 as u32;
    m = ctl.wrapping_neg();
    u = 0 as size_t;
    while u < len {
        let mut aw: u32 = 0;
        let mut w: u32 = 0;
        aw = *a.offset(u as isize);
        w = aw.wrapping_sub(*b.offset(u as isize)).wrapping_sub(cc);
        cc = w >> 31;
        aw ^= (w & 0x7fffffff as u32 ^ aw) & m;
        *a.offset(u as isize) = aw;
        u = u.wrapping_add(1);
        u;
    }
    return cc;
}
unsafe extern "C" fn zint_mul_small(mut m: *mut u32, mut mlen: size_t, mut x: u32) -> u32 {
    let mut u: size_t = 0;
    let mut cc: u32 = 0;
    cc = 0 as u32;
    u = 0 as size_t;
    while u < mlen {
        let mut z: u64 = 0;
        z = (*m.offset(u as isize) as u64 * x as u64).wrapping_add(cc as u64);
        *m.offset(u as isize) = z as u32 & 0x7fffffff as u32;
        cc = (z >> 31) as u32;
        u = u.wrapping_add(1);
        u;
    }
    return cc;
}
unsafe extern "C" fn zint_mod_small_unsigned(
    mut d: *const u32,
    mut dlen: size_t,
    mut p: u32,
    mut p0i: u32,
    mut R2: u32,
) -> u32 {
    let mut x: u32 = 0;
    let mut u: size_t = 0;
    x = 0 as u32;
    u = dlen;
    loop {
        let fresh0 = u;
        u = u.wrapping_sub(1);
        if !(fresh0 > 0 as size_t) {
            break;
        }
        let mut w: u32 = 0;
        x = modp_montymul(x, R2, p, p0i);
        w = (*d.offset(u as isize)).wrapping_sub(p);
        w = w.wrapping_add(p & (w >> 31).wrapping_neg());
        x = modp_add(x, w, p);
    }
    return x;
}
unsafe extern "C" fn zint_mod_small_signed(
    mut d: *const u32,
    mut dlen: size_t,
    mut p: u32,
    mut p0i: u32,
    mut R2: u32,
    mut Rx: u32,
) -> u32 {
    let mut z: u32 = 0;
    if dlen == 0 as size_t {
        return 0 as u32;
    }
    z = zint_mod_small_unsigned(d, dlen, p, p0i, R2);
    z = modp_sub(
        z,
        Rx & (*d.offset(dlen.wrapping_sub(1 as size_t) as isize) >> 30).wrapping_neg(),
        p,
    );
    return z;
}
unsafe extern "C" fn zint_add_mul_small(
    mut x: *mut u32,
    mut y: *const u32,
    mut len: size_t,
    mut s: u32,
) {
    let mut u: size_t = 0;
    let mut cc: u32 = 0;
    cc = 0 as u32;
    u = 0 as size_t;
    while u < len {
        let mut xw: u32 = 0;
        let mut yw: u32 = 0;
        let mut z: u64 = 0;
        xw = *x.offset(u as isize);
        yw = *y.offset(u as isize);
        z = (yw as u64 * s as u64).wrapping_add(xw as u64).wrapping_add(cc as u64);
        *x.offset(u as isize) = z as u32 & 0x7fffffff as u32;
        cc = (z >> 31) as u32;
        u = u.wrapping_add(1);
        u;
    }
    *x.offset(len as isize) = cc;
}
unsafe extern "C" fn zint_norm_zero(mut x: *mut u32, mut p: *const u32, mut len: size_t) {
    let mut u: size_t = 0;
    let mut r: u32 = 0;
    let mut bb: u32 = 0;
    r = 0 as u32;
    bb = 0 as u32;
    u = len;
    loop {
        let fresh1 = u;
        u = u.wrapping_sub(1);
        if !(fresh1 > 0 as size_t) {
            break;
        }
        let mut wx: u32 = 0;
        let mut wp: u32 = 0;
        let mut cc: u32 = 0;
        wx = *x.offset(u as isize);
        wp = *p.offset(u as isize) >> 1 | bb << 30;
        bb = *p.offset(u as isize) & 1 as u32;
        cc = wp.wrapping_sub(wx);
        cc = cc.wrapping_neg() >> 31 | (cc >> 31).wrapping_neg();
        r |= cc & (r & 1 as u32).wrapping_sub(1 as u32);
    }
    zint_sub(x, p, len, r >> 31);
}
unsafe extern "C" fn zint_rebuild_CRT(
    mut xx: *mut u32,
    mut xlen: size_t,
    mut xstride: size_t,
    mut num: size_t,
    mut primes: *const small_prime,
    mut normalize_signed: libc::c_int,
    mut tmp: *mut u32,
) {
    let mut u: size_t = 0;
    let mut x: *mut u32 = 0 as *mut u32;
    *tmp.offset(0 as isize) = (*primes.offset(0 as isize)).p;
    u = 1 as size_t;
    while u < xlen {
        let mut p: u32 = 0;
        let mut p0i: u32 = 0;
        let mut s: u32 = 0;
        let mut R2: u32 = 0;
        let mut v: size_t = 0;
        p = (*primes.offset(u as isize)).p;
        s = (*primes.offset(u as isize)).s;
        p0i = modp_ninv31(p);
        R2 = modp_R2(p, p0i);
        v = 0 as size_t;
        x = xx;
        while v < num {
            let mut xp: u32 = 0;
            let mut xq: u32 = 0;
            let mut xr: u32 = 0;
            xp = *x.offset(u as isize);
            xq = zint_mod_small_unsigned(x, u, p, p0i, R2);
            xr = modp_montymul(s, modp_sub(xp, xq, p), p, p0i);
            zint_add_mul_small(x, tmp, u, xr);
            v = v.wrapping_add(1);
            v;
            x = x.offset(xstride as isize);
        }
        *tmp.offset(u as isize) = zint_mul_small(tmp, u, p);
        u = u.wrapping_add(1);
        u;
    }
    if normalize_signed != 0 {
        u = 0 as size_t;
        x = xx;
        while u < num {
            zint_norm_zero(x, tmp, xlen);
            u = u.wrapping_add(1);
            u;
            x = x.offset(xstride as isize);
        }
    }
}
unsafe extern "C" fn zint_negate(mut a: *mut u32, mut len: size_t, mut ctl: u32) {
    let mut u: size_t = 0;
    let mut cc: u32 = 0;
    let mut m: u32 = 0;
    cc = ctl;
    m = ctl.wrapping_neg() >> 1;
    u = 0 as size_t;
    while u < len {
        let mut aw: u32 = 0;
        aw = *a.offset(u as isize);
        aw = (aw ^ m).wrapping_add(cc);
        *a.offset(u as isize) = aw & 0x7fffffff as u32;
        cc = aw >> 31;
        u = u.wrapping_add(1);
        u;
    }
}
unsafe extern "C" fn zint_co_reduce(
    mut a: *mut u32,
    mut b: *mut u32,
    mut len: size_t,
    mut xa: i64,
    mut xb: i64,
    mut ya: i64,
    mut yb: i64,
) -> u32 {
    let mut u: size_t = 0;
    let mut cca: i64 = 0;
    let mut ccb: i64 = 0;
    let mut nega: u32 = 0;
    let mut negb: u32 = 0;
    cca = 0 as i64;
    ccb = 0 as i64;
    u = 0 as size_t;
    while u < len {
        let mut wa: u32 = 0;
        let mut wb: u32 = 0;
        let mut za: u64 = 0;
        let mut zb: u64 = 0;
        wa = *a.offset(u as isize);
        wb = *b.offset(u as isize);
        za = (wa as u64 * xa as u64).wrapping_add(wb as u64 * xb as u64).wrapping_add(cca as u64);
        zb = (wa as u64 * ya as u64).wrapping_add(wb as u64 * yb as u64).wrapping_add(ccb as u64);
        if u > 0 as size_t {
            *a.offset(u.wrapping_sub(1 as size_t) as isize) = za as u32 & 0x7fffffff as u32;
            *b.offset(u.wrapping_sub(1 as size_t) as isize) = zb as u32 & 0x7fffffff as u32;
        }
        cca = *(&mut za as *mut u64 as *mut i64) >> 31;
        ccb = *(&mut zb as *mut u64 as *mut i64) >> 31;
        u = u.wrapping_add(1);
        u;
    }
    *a.offset(len.wrapping_sub(1 as size_t) as isize) = cca as u32;
    *b.offset(len.wrapping_sub(1 as size_t) as isize) = ccb as u32;
    nega = (cca as u64 >> 63) as u32;
    negb = (ccb as u64 >> 63) as u32;
    zint_negate(a, len, nega);
    zint_negate(b, len, negb);
    return nega | negb << 1;
}
unsafe extern "C" fn zint_finish_mod(
    mut a: *mut u32,
    mut len: size_t,
    mut m: *const u32,
    mut neg: u32,
) {
    let mut u: size_t = 0;
    let mut cc: u32 = 0;
    let mut xm: u32 = 0;
    let mut ym: u32 = 0;
    cc = 0 as u32;
    u = 0 as size_t;
    while u < len {
        cc = (*a.offset(u as isize)).wrapping_sub(*m.offset(u as isize)).wrapping_sub(cc) >> 31;
        u = u.wrapping_add(1);
        u;
    }
    xm = neg.wrapping_neg() >> 1;
    ym = (neg | (1 as u32).wrapping_sub(cc)).wrapping_neg();
    cc = neg;
    u = 0 as size_t;
    while u < len {
        let mut aw: u32 = 0;
        let mut mw: u32 = 0;
        aw = *a.offset(u as isize);
        mw = (*m.offset(u as isize) ^ xm) & ym;
        aw = aw.wrapping_sub(mw).wrapping_sub(cc);
        *a.offset(u as isize) = aw & 0x7fffffff as u32;
        cc = aw >> 31;
        u = u.wrapping_add(1);
        u;
    }
}
unsafe extern "C" fn zint_co_reduce_mod(
    mut a: *mut u32,
    mut b: *mut u32,
    mut m: *const u32,
    mut len: size_t,
    mut m0i: u32,
    mut xa: i64,
    mut xb: i64,
    mut ya: i64,
    mut yb: i64,
) {
    let mut u: size_t = 0;
    let mut cca: i64 = 0;
    let mut ccb: i64 = 0;
    let mut fa: u32 = 0;
    let mut fb: u32 = 0;
    cca = 0 as i64;
    ccb = 0 as i64;
    fa = (*a.offset(0 as isize) * xa as u32).wrapping_add(*b.offset(0 as isize) * xb as u32) * m0i
        & 0x7fffffff as u32;
    fb = (*a.offset(0 as isize) * ya as u32).wrapping_add(*b.offset(0 as isize) * yb as u32) * m0i
        & 0x7fffffff as u32;
    u = 0 as size_t;
    while u < len {
        let mut wa: u32 = 0;
        let mut wb: u32 = 0;
        let mut za: u64 = 0;
        let mut zb: u64 = 0;
        wa = *a.offset(u as isize);
        wb = *b.offset(u as isize);
        za = (wa as u64 * xa as u64)
            .wrapping_add(wb as u64 * xb as u64)
            .wrapping_add(*m.offset(u as isize) as u64 * fa as u64)
            .wrapping_add(cca as u64);
        zb = (wa as u64 * ya as u64)
            .wrapping_add(wb as u64 * yb as u64)
            .wrapping_add(*m.offset(u as isize) as u64 * fb as u64)
            .wrapping_add(ccb as u64);
        if u > 0 as size_t {
            *a.offset(u.wrapping_sub(1 as size_t) as isize) = za as u32 & 0x7fffffff as u32;
            *b.offset(u.wrapping_sub(1 as size_t) as isize) = zb as u32 & 0x7fffffff as u32;
        }
        cca = *(&mut za as *mut u64 as *mut i64) >> 31;
        ccb = *(&mut zb as *mut u64 as *mut i64) >> 31;
        u = u.wrapping_add(1);
        u;
    }
    *a.offset(len.wrapping_sub(1 as size_t) as isize) = cca as u32;
    *b.offset(len.wrapping_sub(1 as size_t) as isize) = ccb as u32;
    zint_finish_mod(a, len, m, (cca as u64 >> 63) as u32);
    zint_finish_mod(b, len, m, (ccb as u64 >> 63) as u32);
}
unsafe extern "C" fn zint_bezout(
    mut u: *mut u32,
    mut v: *mut u32,
    mut x: *const u32,
    mut y: *const u32,
    mut len: size_t,
    mut tmp: *mut u32,
) -> libc::c_int {
    let mut u0: *mut u32 = 0 as *mut u32;
    let mut u1: *mut u32 = 0 as *mut u32;
    let mut v0: *mut u32 = 0 as *mut u32;
    let mut v1: *mut u32 = 0 as *mut u32;
    let mut a: *mut u32 = 0 as *mut u32;
    let mut b: *mut u32 = 0 as *mut u32;
    let mut x0i: u32 = 0;
    let mut y0i: u32 = 0;
    let mut num: u32 = 0;
    let mut rc: u32 = 0;
    let mut j: size_t = 0;
    if len == 0 as size_t {
        return 0;
    }
    u0 = u;
    v0 = v;
    u1 = tmp;
    v1 = u1.offset(len as isize);
    a = v1.offset(len as isize);
    b = a.offset(len as isize);
    x0i = modp_ninv31(*x.offset(0 as isize));
    y0i = modp_ninv31(*y.offset(0 as isize));
    rust_memcpy(
        a as *mut libc::c_void,
        x as *const libc::c_void,
        len.wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    rust_memcpy(
        b as *mut libc::c_void,
        y as *const libc::c_void,
        len.wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    *u0.offset(0 as isize) = 1 as u32;
    rust_memset(
        u0.offset(1 as isize) as *mut libc::c_void,
        0,
        len.wrapping_sub(1 as size_t).wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    rust_memset(
        v0 as *mut libc::c_void,
        0,
        len.wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    rust_memcpy(
        u1 as *mut libc::c_void,
        y as *const libc::c_void,
        len.wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    rust_memcpy(
        v1 as *mut libc::c_void,
        x as *const libc::c_void,
        len.wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    let ref mut fresh2 = *v1.offset(0 as isize);
    *fresh2 = (*fresh2).wrapping_sub(1);
    *fresh2;
    num = (62 as u32 * len as u32).wrapping_add(30 as u32);
    while num >= 30 as u32 {
        let mut c0: u32 = 0;
        let mut c1: u32 = 0;
        let mut a0: u32 = 0;
        let mut a1: u32 = 0;
        let mut b0: u32 = 0;
        let mut b1: u32 = 0;
        let mut a_hi: u64 = 0;
        let mut b_hi: u64 = 0;
        let mut a_lo: u32 = 0;
        let mut b_lo: u32 = 0;
        let mut pa: i64 = 0;
        let mut pb: i64 = 0;
        let mut qa: i64 = 0;
        let mut qb: i64 = 0;
        let mut i: libc::c_int = 0;
        let mut r: u32 = 0;
        c0 = u32::MAX;
        c1 = u32::MAX;
        a0 = 0 as u32;
        a1 = 0 as u32;
        b0 = 0 as u32;
        b1 = 0 as u32;
        j = len;
        loop {
            let fresh3 = j;
            j = j.wrapping_sub(1);
            if !(fresh3 > 0 as size_t) {
                break;
            }
            let mut aw: u32 = 0;
            let mut bw: u32 = 0;
            aw = *a.offset(j as isize);
            bw = *b.offset(j as isize);
            a0 ^= (a0 ^ aw) & c0;
            a1 ^= (a1 ^ aw) & c1;
            b0 ^= (b0 ^ bw) & c0;
            b1 ^= (b1 ^ bw) & c1;
            c1 = c0;
            c0 &= ((aw | bw).wrapping_add(0x7fffffff as u32) >> 31).wrapping_sub(1 as u32);
        }
        a1 |= a0 & c1;
        a0 &= !c1;
        b1 |= b0 & c1;
        b0 &= !c1;
        a_hi = ((a0 as u64) << 31).wrapping_add(a1 as u64);
        b_hi = ((b0 as u64) << 31).wrapping_add(b1 as u64);
        a_lo = *a.offset(0 as isize);
        b_lo = *b.offset(0 as isize);
        pa = 1 as i64;
        pb = 0 as i64;
        qa = 0 as i64;
        qb = 1 as i64;
        i = 0;
        while i < 31 {
            let mut rt: u32 = 0;
            let mut oa: u32 = 0;
            let mut ob: u32 = 0;
            let mut cAB: u32 = 0;
            let mut cBA: u32 = 0;
            let mut cA: u32 = 0;
            let mut rz: u64 = 0;
            rz = b_hi.wrapping_sub(a_hi);
            rt = ((rz ^ (a_hi ^ b_hi) & (a_hi ^ rz)) >> 63) as u32;
            oa = a_lo >> i & 1 as u32;
            ob = b_lo >> i & 1 as u32;
            cAB = oa & ob & rt;
            cBA = oa & ob & !rt;
            cA = cAB | oa ^ 1 as u32;
            a_lo = a_lo.wrapping_sub(b_lo & cAB.wrapping_neg());
            a_hi = a_hi.wrapping_sub(b_hi & (cAB as u64).wrapping_neg());
            pa -= qa & -(cAB as i64);
            pb -= qb & -(cAB as i64);
            b_lo = b_lo.wrapping_sub(a_lo & cBA.wrapping_neg());
            b_hi = b_hi.wrapping_sub(a_hi & (cBA as u64).wrapping_neg());
            qa -= pa & -(cBA as i64);
            qb -= pb & -(cBA as i64);
            a_lo = a_lo.wrapping_add(a_lo & cA.wrapping_sub(1 as u32));
            pa += pa & cA as i64 - 1 as i64;
            pb += pb & cA as i64 - 1 as i64;
            a_hi ^= (a_hi ^ a_hi >> 1) & (cA as u64).wrapping_neg();
            b_lo = b_lo.wrapping_add(b_lo & cA.wrapping_neg());
            qa += qa & -(cA as i64);
            qb += qb & -(cA as i64);
            b_hi ^= (b_hi ^ b_hi >> 1) & (cA as u64).wrapping_sub(1 as u64);
            i += 1;
            i;
        }
        r = zint_co_reduce(a, b, len, pa, pb, qa, qb);
        pa -= pa + pa & -((r & 1 as u32) as i64);
        pb -= pb + pb & -((r & 1 as u32) as i64);
        qa -= qa + qa & -((r >> 1) as i64);
        qb -= qb + qb & -((r >> 1) as i64);
        zint_co_reduce_mod(u0, u1, y, len, y0i, pa, pb, qa, qb);
        zint_co_reduce_mod(v0, v1, x, len, x0i, pa, pb, qa, qb);
        num = num.wrapping_sub(30 as u32);
    }
    rc = *a.offset(0 as isize) ^ 1 as u32;
    j = 1 as size_t;
    while j < len {
        rc |= *a.offset(j as isize);
        j = j.wrapping_add(1);
        j;
    }
    return ((1 as u32).wrapping_sub((rc | rc.wrapping_neg()) >> 31)
        & *x.offset(0 as isize)
        & *y.offset(0 as isize)) as libc::c_int;
}
unsafe extern "C" fn zint_add_scaled_mul_small(
    mut x: *mut u32,
    mut xlen: size_t,
    mut y: *const u32,
    mut ylen: size_t,
    mut k: int32_t,
    mut sch: u32,
    mut scl: u32,
) {
    let mut u: size_t = 0;
    let mut ysign: u32 = 0;
    let mut tw: u32 = 0;
    let mut cc: int32_t = 0;
    if ylen == 0 as size_t {
        return;
    }
    ysign = (*y.offset(ylen.wrapping_sub(1 as size_t) as isize) >> 30).wrapping_neg() >> 1;
    tw = 0 as u32;
    cc = 0;
    u = sch as size_t;
    while u < xlen {
        let mut v: size_t = 0;
        let mut wy: u32 = 0;
        let mut wys: u32 = 0;
        let mut ccu: u32 = 0;
        let mut z: u64 = 0;
        v = u.wrapping_sub(sch as size_t);
        if v < ylen {
            wy = *y.offset(v as isize);
        } else {
            wy = ysign;
        }
        wys = wy << scl & 0x7fffffff as u32 | tw;
        tw = wy >> (31 as u32).wrapping_sub(scl);
        z = (wys as i64 * k as i64 + *x.offset(u as isize) as i64 + cc as i64) as u64;
        *x.offset(u as isize) = z as u32 & 0x7fffffff as u32;
        ccu = (z >> 31) as u32;
        cc = *(&mut ccu as *mut u32 as *mut int32_t);
        u = u.wrapping_add(1);
        u;
    }
}
unsafe extern "C" fn zint_sub_scaled(
    mut x: *mut u32,
    mut xlen: size_t,
    mut y: *const u32,
    mut ylen: size_t,
    mut sch: u32,
    mut scl: u32,
) {
    let mut u: size_t = 0;
    let mut ysign: u32 = 0;
    let mut tw: u32 = 0;
    let mut cc: u32 = 0;
    if ylen == 0 as size_t {
        return;
    }
    ysign = (*y.offset(ylen.wrapping_sub(1 as size_t) as isize) >> 30).wrapping_neg() >> 1;
    tw = 0 as u32;
    cc = 0 as u32;
    u = sch as size_t;
    while u < xlen {
        let mut v: size_t = 0;
        let mut w: u32 = 0;
        let mut wy: u32 = 0;
        let mut wys: u32 = 0;
        v = u.wrapping_sub(sch as size_t);
        if v < ylen {
            wy = *y.offset(v as isize);
        } else {
            wy = ysign;
        }
        wys = wy << scl & 0x7fffffff as u32 | tw;
        tw = wy >> (31 as u32).wrapping_sub(scl);
        w = (*x.offset(u as isize)).wrapping_sub(wys).wrapping_sub(cc);
        *x.offset(u as isize) = w & 0x7fffffff as u32;
        cc = w >> 31;
        u = u.wrapping_add(1);
        u;
    }
}
#[inline]
unsafe extern "C" fn zint_one_to_plain(mut x: *const u32) -> int32_t {
    let mut w: u32 = 0;
    w = *x.offset(0 as isize);
    w |= (w & 0x40000000 as u32) << 1;
    return *(&mut w as *mut u32 as *mut int32_t);
}
unsafe extern "C" fn poly_big_to_fp(
    mut d: *mut fpr,
    mut f: *const u32,
    mut flen: size_t,
    mut fstride: size_t,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    if flen == 0 as size_t {
        u = 0 as size_t;
        while u < n {
            *d.offset(u as isize) = fpr_zero;
            u = u.wrapping_add(1);
            u;
        }
        return;
    }
    u = 0 as size_t;
    while u < n {
        let mut v: size_t = 0;
        let mut neg: u32 = 0;
        let mut cc: u32 = 0;
        let mut xm: u32 = 0;
        let mut x: fpr = 0;
        let mut fsc: fpr = 0;
        neg = (*f.offset(flen.wrapping_sub(1 as size_t) as isize) >> 30).wrapping_neg();
        xm = neg >> 1;
        cc = neg & 1 as u32;
        x = fpr_zero;
        fsc = fpr_one;
        v = 0 as size_t;
        while v < flen {
            let mut w: u32 = 0;
            w = (*f.offset(v as isize) ^ xm).wrapping_add(cc);
            cc = w >> 31;
            w &= 0x7fffffff as u32;
            w = w.wrapping_sub(w << 1 & neg);
            x = PQCLEAN_FALCON512_CLEAN_fpr_add(
                x,
                PQCLEAN_FALCON512_CLEAN_fpr_mul(
                    fpr_of(*(&mut w as *mut u32 as *mut int32_t) as i64),
                    fsc,
                ),
            );
            v = v.wrapping_add(1);
            v;
            fsc = PQCLEAN_FALCON512_CLEAN_fpr_mul(fsc, fpr_ptwo31);
        }
        *d.offset(u as isize) = x;
        u = u.wrapping_add(1);
        u;
        f = f.offset(fstride as isize);
    }
}
unsafe extern "C" fn poly_big_to_small(
    mut d: *mut int8_t,
    mut s: *const u32,
    mut lim: libc::c_int,
    mut logn: libc::c_uint,
) -> libc::c_int {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    u = 0 as size_t;
    while u < n {
        let mut z: int32_t = 0;
        z = zint_one_to_plain(s.offset(u as isize));
        if z < -lim || z > lim {
            return 0;
        }
        *d.offset(u as isize) = z as int8_t;
        u = u.wrapping_add(1);
        u;
    }
    return 1;
}
unsafe extern "C" fn poly_sub_scaled(
    mut F: *mut u32,
    mut Flen: size_t,
    mut Fstride: size_t,
    mut f: *const u32,
    mut flen: size_t,
    mut fstride: size_t,
    mut k: *const int32_t,
    mut sch: u32,
    mut scl: u32,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    u = 0 as size_t;
    while u < n {
        let mut kf: int32_t = 0;
        let mut v: size_t = 0;
        let mut x: *mut u32 = 0 as *mut u32;
        let mut y: *const u32 = 0 as *const u32;
        kf = -*k.offset(u as isize);
        x = F.offset((u * Fstride) as isize);
        y = f;
        v = 0 as size_t;
        while v < n {
            zint_add_scaled_mul_small(x, Flen, y, flen, kf, sch, scl);
            if u.wrapping_add(v) == n.wrapping_sub(1 as size_t) {
                x = F;
                kf = -kf;
            } else {
                x = x.offset(Fstride as isize);
            }
            y = y.offset(fstride as isize);
            v = v.wrapping_add(1);
            v;
        }
        u = u.wrapping_add(1);
        u;
    }
}
unsafe extern "C" fn poly_sub_scaled_ntt(
    mut F: *mut u32,
    mut Flen: size_t,
    mut Fstride: size_t,
    mut f: *const u32,
    mut flen: size_t,
    mut fstride: size_t,
    mut k: *const int32_t,
    mut sch: u32,
    mut scl: u32,
    mut logn: libc::c_uint,
    mut tmp: *mut u32,
) {
    let mut gm: *mut u32 = 0 as *mut u32;
    let mut igm: *mut u32 = 0 as *mut u32;
    let mut fk: *mut u32 = 0 as *mut u32;
    let mut t1: *mut u32 = 0 as *mut u32;
    let mut x: *mut u32 = 0 as *mut u32;
    let mut y: *const u32 = 0 as *const u32;
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut tlen: size_t = 0;
    let mut primes: *const small_prime = 0 as *const small_prime;
    n = (1 as size_t) << logn;
    tlen = flen.wrapping_add(1 as size_t);
    gm = tmp;
    igm = gm.offset(((1 as size_t) << logn) as isize);
    fk = igm.offset(((1 as size_t) << logn) as isize);
    t1 = fk.offset((n * tlen) as isize);
    primes = PRIMES.as_ptr();
    u = 0 as size_t;
    while u < tlen {
        let mut p: u32 = 0;
        let mut p0i: u32 = 0;
        let mut R2: u32 = 0;
        let mut Rx: u32 = 0;
        let mut v: size_t = 0;
        p = (*primes.offset(u as isize)).p;
        p0i = modp_ninv31(p);
        R2 = modp_R2(p, p0i);
        Rx = modp_Rx(flen as libc::c_uint, p, p0i, R2);
        modp_mkgm2(gm, igm, logn, (*primes.offset(u as isize)).g, p, p0i);
        v = 0 as size_t;
        while v < n {
            *t1.offset(v as isize) = modp_set(*k.offset(v as isize), p);
            v = v.wrapping_add(1);
            v;
        }
        modp_NTT2_ext(t1, 1 as size_t, gm, logn, p, p0i);
        v = 0 as size_t;
        y = f;
        x = fk.offset(u as isize);
        while v < n {
            *x = zint_mod_small_signed(y, flen, p, p0i, R2, Rx);
            v = v.wrapping_add(1);
            v;
            y = y.offset(fstride as isize);
            x = x.offset(tlen as isize);
        }
        modp_NTT2_ext(fk.offset(u as isize), tlen, gm, logn, p, p0i);
        v = 0 as size_t;
        x = fk.offset(u as isize);
        while v < n {
            *x = modp_montymul(modp_montymul(*t1.offset(v as isize), *x, p, p0i), R2, p, p0i);
            v = v.wrapping_add(1);
            v;
            x = x.offset(tlen as isize);
        }
        modp_iNTT2_ext(fk.offset(u as isize), tlen, igm, logn, p, p0i);
        u = u.wrapping_add(1);
        u;
    }
    zint_rebuild_CRT(fk, tlen, tlen, n, primes, 1, t1);
    u = 0 as size_t;
    x = F;
    y = fk;
    while u < n {
        zint_sub_scaled(x, Flen, y, tlen, sch, scl);
        u = u.wrapping_add(1);
        u;
        x = x.offset(Fstride as isize);
        y = y.offset(tlen as isize);
    }
}
#[inline]
unsafe extern "C" fn get_rng_u64(mut rng: *mut shake256incctx) -> u64 {
    let mut tmp: [uint8_t; 8] = [0; 8];
    shake256_inc_squeeze(
        tmp.as_mut_ptr(),
        ::core::mem::size_of::<[uint8_t; 8]>() as libc::c_ulong,
        rng,
    );
    return tmp[0 as usize] as u64
        | (tmp[1 as usize] as u64) << 8
        | (tmp[2 as usize] as u64) << 16
        | (tmp[3 as usize] as u64) << 24
        | (tmp[4 as usize] as u64) << 32
        | (tmp[5 as usize] as u64) << 40
        | (tmp[6 as usize] as u64) << 48
        | (tmp[7 as usize] as u64) << 56;
}
static mut gauss_1024_12289: [u64; 27] = [
    1283868770400643928,
    6416574995475331444,
    4078260278032692663,
    2353523259288686585,
    1227179971273316331,
    575931623374121527,
    242543240509105209,
    91437049221049666,
    30799446349977173,
    9255276791179340,
    2478152334826140,
    590642893610164,
    125206034929641,
    23590435911403,
    3948334035941,
    586753615614,
    77391054539,
    9056793210,
    940121950 as libc::c_uint as u64,
    86539696 as libc::c_uint as u64,
    7062824 as libc::c_uint as u64,
    510971 as libc::c_uint as u64,
    32764 as libc::c_uint as u64,
    1862 as libc::c_uint as u64,
    94 as libc::c_uint as u64,
    4 as libc::c_uint as u64,
    0 as libc::c_uint as u64,
];
unsafe extern "C" fn mkgauss(mut rng: *mut shake256incctx, mut logn: libc::c_uint) -> libc::c_int {
    let mut u: libc::c_uint = 0;
    let mut g: libc::c_uint = 0;
    let mut val: libc::c_int = 0;
    g = (1 as libc::c_uint) << (10 as libc::c_uint).wrapping_sub(logn);
    val = 0;
    u = 0 as libc::c_uint;
    while u < g {
        let mut r: u64 = 0;
        let mut f: u32 = 0;
        let mut v: u32 = 0;
        let mut k: u32 = 0;
        let mut neg: u32 = 0;
        r = get_rng_u64(rng);
        neg = (r >> 63) as u32;
        r &= !((1 as u64) << 63);
        f = (r.wrapping_sub(gauss_1024_12289[0 as usize]) >> 63) as u32;
        v = 0 as u32;
        r = get_rng_u64(rng);
        r &= !((1 as u64) << 63);
        k = 1 as u32;
        while (k as libc::c_ulong)
            < (::core::mem::size_of::<[u64; 27]>() as libc::c_ulong)
                .wrapping_div(::core::mem::size_of::<u64>() as libc::c_ulong)
        {
            let mut t: u32 = 0;
            t = (r.wrapping_sub(gauss_1024_12289[k as usize]) >> 63) as u32 ^ 1 as u32;
            v |= k & (t & (f ^ 1 as u32)).wrapping_neg();
            f |= t;
            k = k.wrapping_add(1);
            k;
        }
        v = (v ^ neg.wrapping_neg()).wrapping_add(neg);
        val += *(&mut v as *mut u32 as *mut int32_t);
        u = u.wrapping_add(1);
        u;
    }
    return val;
}
static mut MAX_BL_SMALL: [size_t; 11] = [
    1 as size_t,
    1 as size_t,
    2 as size_t,
    2 as size_t,
    4 as size_t,
    7 as size_t,
    14 as size_t,
    27 as size_t,
    53 as size_t,
    106 as size_t,
    209 as size_t,
];
static mut MAX_BL_LARGE: [size_t; 10] = [
    2 as size_t,
    2 as size_t,
    5 as size_t,
    7 as size_t,
    12 as size_t,
    21 as size_t,
    40 as size_t,
    78 as size_t,
    157 as size_t,
    308 as size_t,
];
static mut BITLENGTH: [C2RustUnnamed; 11] = [
    {
        let mut init = C2RustUnnamed { avg: 4, std: 0 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 11, std: 1 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 24, std: 1 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 50, std: 1 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 102, std: 1 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 202, std: 2 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 401, std: 4 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 794, std: 5 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 1577, std: 8 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 3138, std: 13 };
        init
    },
    {
        let mut init = C2RustUnnamed { avg: 6308, std: 25 };
        init
    },
];
unsafe extern "C" fn poly_small_sqnorm(mut f: *const int8_t, mut logn: libc::c_uint) -> u32 {
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
        z = *f.offset(u as isize) as int32_t;
        s = s.wrapping_add((z * z) as u32);
        ng |= s;
        u = u.wrapping_add(1);
        u;
    }
    return s | (ng >> 31).wrapping_neg();
}
unsafe extern "C" fn align_fpr(
    mut base: *mut libc::c_void,
    mut data: *mut libc::c_void,
) -> *mut fpr {
    let mut cb: *mut uint8_t = 0 as *mut uint8_t;
    let mut cd: *mut uint8_t = 0 as *mut uint8_t;
    let mut k: size_t = 0;
    let mut km: size_t = 0;
    cb = base as *mut uint8_t;
    cd = data as *mut uint8_t;
    k = cd.offset_from(cb) as libc::c_long as size_t;
    km = k.wrapping_rem(::core::mem::size_of::<fpr>() as libc::c_ulong);
    if km != 0 {
        k = (k as libc::c_ulong)
            .wrapping_add((::core::mem::size_of::<fpr>() as libc::c_ulong).wrapping_sub(km))
            as size_t as size_t;
    }
    return cb.offset(k as isize) as *mut fpr;
}
unsafe extern "C" fn align_u32(
    mut base: *mut libc::c_void,
    mut data: *mut libc::c_void,
) -> *mut u32 {
    let mut cb: *mut uint8_t = 0 as *mut uint8_t;
    let mut cd: *mut uint8_t = 0 as *mut uint8_t;
    let mut k: size_t = 0;
    let mut km: size_t = 0;
    cb = base as *mut uint8_t;
    cd = data as *mut uint8_t;
    k = cd.offset_from(cb) as libc::c_long as size_t;
    km = k.wrapping_rem(::core::mem::size_of::<u32>() as libc::c_ulong);
    if km != 0 {
        k = (k as libc::c_ulong)
            .wrapping_add((::core::mem::size_of::<u32>() as libc::c_ulong).wrapping_sub(km))
            as size_t as size_t;
    }
    return cb.offset(k as isize) as *mut u32;
}
unsafe extern "C" fn poly_small_to_fp(
    mut x: *mut fpr,
    mut f: *const int8_t,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    u = 0 as size_t;
    while u < n {
        *x.offset(u as isize) = fpr_of(*f.offset(u as isize) as i64);
        u = u.wrapping_add(1);
        u;
    }
}
unsafe extern "C" fn make_fg_step(
    mut data: *mut u32,
    mut logn: libc::c_uint,
    mut depth: libc::c_uint,
    mut in_ntt: libc::c_int,
    mut out_ntt: libc::c_int,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    let mut slen: size_t = 0;
    let mut tlen: size_t = 0;
    let mut fd: *mut u32 = 0 as *mut u32;
    let mut gd: *mut u32 = 0 as *mut u32;
    let mut fs: *mut u32 = 0 as *mut u32;
    let mut gs: *mut u32 = 0 as *mut u32;
    let mut gm: *mut u32 = 0 as *mut u32;
    let mut igm: *mut u32 = 0 as *mut u32;
    let mut t1: *mut u32 = 0 as *mut u32;
    let mut primes: *const small_prime = 0 as *const small_prime;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    slen = MAX_BL_SMALL[depth as usize];
    tlen = MAX_BL_SMALL[depth.wrapping_add(1 as libc::c_uint) as usize];
    primes = PRIMES.as_ptr();
    fd = data;
    gd = fd.offset((hn * tlen) as isize);
    fs = gd.offset((hn * tlen) as isize);
    gs = fs.offset((n * slen) as isize);
    gm = gs.offset((n * slen) as isize);
    igm = gm.offset(n as isize);
    t1 = igm.offset(n as isize);
    rust_memmove(
        fs as *mut libc::c_void,
        data as *const libc::c_void,
        (2 as size_t * n * slen).wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    u = 0 as size_t;
    while u < slen {
        let mut p: u32 = 0;
        let mut p0i: u32 = 0;
        let mut R2: u32 = 0;
        let mut v: size_t = 0;
        let mut x: *mut u32 = 0 as *mut u32;
        p = (*primes.offset(u as isize)).p;
        p0i = modp_ninv31(p);
        R2 = modp_R2(p, p0i);
        modp_mkgm2(gm, igm, logn, (*primes.offset(u as isize)).g, p, p0i);
        v = 0 as size_t;
        x = fs.offset(u as isize);
        while v < n {
            *t1.offset(v as isize) = *x;
            v = v.wrapping_add(1);
            v;
            x = x.offset(slen as isize);
        }
        if in_ntt == 0 {
            modp_NTT2_ext(t1, 1 as size_t, gm, logn, p, p0i);
        }
        v = 0 as size_t;
        x = fd.offset(u as isize);
        while v < hn {
            let mut w0: u32 = 0;
            let mut w1: u32 = 0;
            w0 = *t1.offset((v << 1).wrapping_add(0 as size_t) as isize);
            w1 = *t1.offset((v << 1).wrapping_add(1 as size_t) as isize);
            *x = modp_montymul(modp_montymul(w0, w1, p, p0i), R2, p, p0i);
            v = v.wrapping_add(1);
            v;
            x = x.offset(tlen as isize);
        }
        if in_ntt != 0 {
            modp_iNTT2_ext(fs.offset(u as isize), slen, igm, logn, p, p0i);
        }
        v = 0 as size_t;
        x = gs.offset(u as isize);
        while v < n {
            *t1.offset(v as isize) = *x;
            v = v.wrapping_add(1);
            v;
            x = x.offset(slen as isize);
        }
        if in_ntt == 0 {
            modp_NTT2_ext(t1, 1 as size_t, gm, logn, p, p0i);
        }
        v = 0 as size_t;
        x = gd.offset(u as isize);
        while v < hn {
            let mut w0_0: u32 = 0;
            let mut w1_0: u32 = 0;
            w0_0 = *t1.offset((v << 1).wrapping_add(0 as size_t) as isize);
            w1_0 = *t1.offset((v << 1).wrapping_add(1 as size_t) as isize);
            *x = modp_montymul(modp_montymul(w0_0, w1_0, p, p0i), R2, p, p0i);
            v = v.wrapping_add(1);
            v;
            x = x.offset(tlen as isize);
        }
        if in_ntt != 0 {
            modp_iNTT2_ext(gs.offset(u as isize), slen, igm, logn, p, p0i);
        }
        if out_ntt == 0 {
            modp_iNTT2_ext(
                fd.offset(u as isize),
                tlen,
                igm,
                logn.wrapping_sub(1 as libc::c_uint),
                p,
                p0i,
            );
            modp_iNTT2_ext(
                gd.offset(u as isize),
                tlen,
                igm,
                logn.wrapping_sub(1 as libc::c_uint),
                p,
                p0i,
            );
        }
        u = u.wrapping_add(1);
        u;
    }
    zint_rebuild_CRT(fs, slen, slen, n, primes, 1, gm);
    zint_rebuild_CRT(gs, slen, slen, n, primes, 1, gm);
    u = slen;
    while u < tlen {
        let mut p_0: u32 = 0;
        let mut p0i_0: u32 = 0;
        let mut R2_0: u32 = 0;
        let mut Rx: u32 = 0;
        let mut v_0: size_t = 0;
        let mut x_0: *mut u32 = 0 as *mut u32;
        p_0 = (*primes.offset(u as isize)).p;
        p0i_0 = modp_ninv31(p_0);
        R2_0 = modp_R2(p_0, p0i_0);
        Rx = modp_Rx(slen as libc::c_uint, p_0, p0i_0, R2_0);
        modp_mkgm2(gm, igm, logn, (*primes.offset(u as isize)).g, p_0, p0i_0);
        v_0 = 0 as size_t;
        x_0 = fs;
        while v_0 < n {
            *t1.offset(v_0 as isize) = zint_mod_small_signed(x_0, slen, p_0, p0i_0, R2_0, Rx);
            v_0 = v_0.wrapping_add(1);
            v_0;
            x_0 = x_0.offset(slen as isize);
        }
        modp_NTT2_ext(t1, 1 as size_t, gm, logn, p_0, p0i_0);
        v_0 = 0 as size_t;
        x_0 = fd.offset(u as isize);
        while v_0 < hn {
            let mut w0_1: u32 = 0;
            let mut w1_1: u32 = 0;
            w0_1 = *t1.offset((v_0 << 1).wrapping_add(0 as size_t) as isize);
            w1_1 = *t1.offset((v_0 << 1).wrapping_add(1 as size_t) as isize);
            *x_0 = modp_montymul(modp_montymul(w0_1, w1_1, p_0, p0i_0), R2_0, p_0, p0i_0);
            v_0 = v_0.wrapping_add(1);
            v_0;
            x_0 = x_0.offset(tlen as isize);
        }
        v_0 = 0 as size_t;
        x_0 = gs;
        while v_0 < n {
            *t1.offset(v_0 as isize) = zint_mod_small_signed(x_0, slen, p_0, p0i_0, R2_0, Rx);
            v_0 = v_0.wrapping_add(1);
            v_0;
            x_0 = x_0.offset(slen as isize);
        }
        modp_NTT2_ext(t1, 1 as size_t, gm, logn, p_0, p0i_0);
        v_0 = 0 as size_t;
        x_0 = gd.offset(u as isize);
        while v_0 < hn {
            let mut w0_2: u32 = 0;
            let mut w1_2: u32 = 0;
            w0_2 = *t1.offset((v_0 << 1).wrapping_add(0 as size_t) as isize);
            w1_2 = *t1.offset((v_0 << 1).wrapping_add(1 as size_t) as isize);
            *x_0 = modp_montymul(modp_montymul(w0_2, w1_2, p_0, p0i_0), R2_0, p_0, p0i_0);
            v_0 = v_0.wrapping_add(1);
            v_0;
            x_0 = x_0.offset(tlen as isize);
        }
        if out_ntt == 0 {
            modp_iNTT2_ext(
                fd.offset(u as isize),
                tlen,
                igm,
                logn.wrapping_sub(1 as libc::c_uint),
                p_0,
                p0i_0,
            );
            modp_iNTT2_ext(
                gd.offset(u as isize),
                tlen,
                igm,
                logn.wrapping_sub(1 as libc::c_uint),
                p_0,
                p0i_0,
            );
        }
        u = u.wrapping_add(1);
        u;
    }
}
unsafe extern "C" fn make_fg(
    mut data: *mut u32,
    mut f: *const int8_t,
    mut g: *const int8_t,
    mut logn: libc::c_uint,
    mut depth: libc::c_uint,
    mut out_ntt: libc::c_int,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut ft: *mut u32 = 0 as *mut u32;
    let mut gt: *mut u32 = 0 as *mut u32;
    let mut p0: u32 = 0;
    let mut d: libc::c_uint = 0;
    let mut primes: *const small_prime = 0 as *const small_prime;
    n = (1 as size_t) << logn;
    ft = data;
    gt = ft.offset(n as isize);
    primes = PRIMES.as_ptr();
    p0 = (*primes.offset(0 as isize)).p;
    u = 0 as size_t;
    while u < n {
        *ft.offset(u as isize) = modp_set(*f.offset(u as isize) as int32_t, p0);
        *gt.offset(u as isize) = modp_set(*g.offset(u as isize) as int32_t, p0);
        u = u.wrapping_add(1);
        u;
    }
    if depth == 0 as libc::c_uint && out_ntt != 0 {
        let mut gm: *mut u32 = 0 as *mut u32;
        let mut igm: *mut u32 = 0 as *mut u32;
        let mut p: u32 = 0;
        let mut p0i: u32 = 0;
        p = (*primes.offset(0 as isize)).p;
        p0i = modp_ninv31(p);
        gm = gt.offset(n as isize);
        igm = gm.offset(((1 as size_t) << logn) as isize);
        modp_mkgm2(gm, igm, logn, (*primes.offset(0 as isize)).g, p, p0i);
        modp_NTT2_ext(ft, 1 as size_t, gm, logn, p, p0i);
        modp_NTT2_ext(gt, 1 as size_t, gm, logn, p, p0i);
        return;
    }
    if depth == 0 as libc::c_uint {
        return;
    }
    if depth == 1 as libc::c_uint {
        make_fg_step(data, logn, 0 as libc::c_uint, 0, out_ntt);
        return;
    }
    make_fg_step(data, logn, 0 as libc::c_uint, 0, 1);
    d = 1 as libc::c_uint;
    while d.wrapping_add(1 as libc::c_uint) < depth {
        make_fg_step(data, logn.wrapping_sub(d), d, 1, 1);
        d = d.wrapping_add(1);
        d;
    }
    make_fg_step(
        data,
        logn.wrapping_sub(depth).wrapping_add(1 as libc::c_uint),
        depth.wrapping_sub(1 as libc::c_uint),
        1,
        out_ntt,
    );
}
unsafe extern "C" fn solve_NTRU_deepest(
    mut logn_top: libc::c_uint,
    mut f: *const int8_t,
    mut g: *const int8_t,
    mut tmp: *mut u32,
) -> libc::c_int {
    let mut len: size_t = 0;
    let mut Fp: *mut u32 = 0 as *mut u32;
    let mut Gp: *mut u32 = 0 as *mut u32;
    let mut fp: *mut u32 = 0 as *mut u32;
    let mut gp: *mut u32 = 0 as *mut u32;
    let mut t1: *mut u32 = 0 as *mut u32;
    let mut q: u32 = 0;
    let mut primes: *const small_prime = 0 as *const small_prime;
    len = MAX_BL_SMALL[logn_top as usize];
    primes = PRIMES.as_ptr();
    Fp = tmp;
    Gp = Fp.offset(len as isize);
    fp = Gp.offset(len as isize);
    gp = fp.offset(len as isize);
    t1 = gp.offset(len as isize);
    make_fg(fp, f, g, logn_top, logn_top, 0);
    zint_rebuild_CRT(fp, len, len, 2 as size_t, primes, 0, t1);
    if zint_bezout(Gp, Fp, fp, gp, len, t1) == 0 {
        return 0;
    }
    q = 12289 as u32;
    if zint_mul_small(Fp, len, q) != 0 as u32 || zint_mul_small(Gp, len, q) != 0 as u32 {
        return 0;
    }
    return 1;
}
unsafe extern "C" fn solve_NTRU_intermediate(
    mut logn_top: libc::c_uint,
    mut f: *const int8_t,
    mut g: *const int8_t,
    mut depth: libc::c_uint,
    mut tmp: *mut u32,
) -> libc::c_int {
    let mut logn: libc::c_uint = 0;
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut slen: size_t = 0;
    let mut dlen: size_t = 0;
    let mut llen: size_t = 0;
    let mut rlen: size_t = 0;
    let mut FGlen: size_t = 0;
    let mut u: size_t = 0;
    let mut Fd: *mut u32 = 0 as *mut u32;
    let mut Gd: *mut u32 = 0 as *mut u32;
    let mut Ft: *mut u32 = 0 as *mut u32;
    let mut Gt: *mut u32 = 0 as *mut u32;
    let mut ft: *mut u32 = 0 as *mut u32;
    let mut gt: *mut u32 = 0 as *mut u32;
    let mut t1: *mut u32 = 0 as *mut u32;
    let mut rt1: *mut fpr = 0 as *mut fpr;
    let mut rt2: *mut fpr = 0 as *mut fpr;
    let mut rt3: *mut fpr = 0 as *mut fpr;
    let mut rt4: *mut fpr = 0 as *mut fpr;
    let mut rt5: *mut fpr = 0 as *mut fpr;
    let mut scale_fg: libc::c_int = 0;
    let mut minbl_fg: libc::c_int = 0;
    let mut maxbl_fg: libc::c_int = 0;
    let mut maxbl_FG: libc::c_int = 0;
    let mut scale_k: libc::c_int = 0;
    let mut x: *mut u32 = 0 as *mut u32;
    let mut y: *mut u32 = 0 as *mut u32;
    let mut k: *mut int32_t = 0 as *mut int32_t;
    let mut primes: *const small_prime = 0 as *const small_prime;
    logn = logn_top.wrapping_sub(depth);
    n = (1 as size_t) << logn;
    hn = n >> 1;
    slen = MAX_BL_SMALL[depth as usize];
    dlen = MAX_BL_SMALL[depth.wrapping_add(1 as libc::c_uint) as usize];
    llen = MAX_BL_LARGE[depth as usize];
    primes = PRIMES.as_ptr();
    Fd = tmp;
    Gd = Fd.offset((dlen * hn) as isize);
    ft = Gd.offset((dlen * hn) as isize);
    make_fg(ft, f, g, logn_top, depth, 1);
    Ft = tmp;
    Gt = Ft.offset((n * llen) as isize);
    t1 = Gt.offset((n * llen) as isize);
    rust_memmove(
        t1 as *mut libc::c_void,
        ft as *const libc::c_void,
        (2 as size_t * n * slen).wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    ft = t1;
    gt = ft.offset((slen * n) as isize);
    t1 = gt.offset((slen * n) as isize);
    rust_memmove(
        t1 as *mut libc::c_void,
        Fd as *const libc::c_void,
        (2 as size_t * hn * dlen).wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    Fd = t1;
    Gd = Fd.offset((hn * dlen) as isize);
    u = 0 as size_t;
    while u < llen {
        let mut p: u32 = 0;
        let mut p0i: u32 = 0;
        let mut R2: u32 = 0;
        let mut Rx: u32 = 0;
        let mut v: size_t = 0;
        let mut xs: *mut u32 = 0 as *mut u32;
        let mut ys: *mut u32 = 0 as *mut u32;
        let mut xd: *mut u32 = 0 as *mut u32;
        let mut yd: *mut u32 = 0 as *mut u32;
        p = (*primes.offset(u as isize)).p;
        p0i = modp_ninv31(p);
        R2 = modp_R2(p, p0i);
        Rx = modp_Rx(dlen as libc::c_uint, p, p0i, R2);
        v = 0 as size_t;
        xs = Fd;
        ys = Gd;
        xd = Ft.offset(u as isize);
        yd = Gt.offset(u as isize);
        while v < hn {
            *xd = zint_mod_small_signed(xs, dlen, p, p0i, R2, Rx);
            *yd = zint_mod_small_signed(ys, dlen, p, p0i, R2, Rx);
            v = v.wrapping_add(1);
            v;
            xs = xs.offset(dlen as isize);
            ys = ys.offset(dlen as isize);
            xd = xd.offset(llen as isize);
            yd = yd.offset(llen as isize);
        }
        u = u.wrapping_add(1);
        u;
    }
    u = 0 as size_t;
    while u < llen {
        let mut p_0: u32 = 0;
        let mut p0i_0: u32 = 0;
        let mut R2_0: u32 = 0;
        let mut gm: *mut u32 = 0 as *mut u32;
        let mut igm: *mut u32 = 0 as *mut u32;
        let mut fx: *mut u32 = 0 as *mut u32;
        let mut gx: *mut u32 = 0 as *mut u32;
        let mut Fp: *mut u32 = 0 as *mut u32;
        let mut Gp: *mut u32 = 0 as *mut u32;
        let mut v_0: size_t = 0;
        p_0 = (*primes.offset(u as isize)).p;
        p0i_0 = modp_ninv31(p_0);
        R2_0 = modp_R2(p_0, p0i_0);
        if u == slen {
            zint_rebuild_CRT(ft, slen, slen, n, primes, 1, t1);
            zint_rebuild_CRT(gt, slen, slen, n, primes, 1, t1);
        }
        gm = t1;
        igm = gm.offset(n as isize);
        fx = igm.offset(n as isize);
        gx = fx.offset(n as isize);
        modp_mkgm2(gm, igm, logn, (*primes.offset(u as isize)).g, p_0, p0i_0);
        if u < slen {
            v_0 = 0 as size_t;
            x = ft.offset(u as isize);
            y = gt.offset(u as isize);
            while v_0 < n {
                *fx.offset(v_0 as isize) = *x;
                *gx.offset(v_0 as isize) = *y;
                v_0 = v_0.wrapping_add(1);
                v_0;
                x = x.offset(slen as isize);
                y = y.offset(slen as isize);
            }
            modp_iNTT2_ext(ft.offset(u as isize), slen, igm, logn, p_0, p0i_0);
            modp_iNTT2_ext(gt.offset(u as isize), slen, igm, logn, p_0, p0i_0);
        } else {
            let mut Rx_0: u32 = 0;
            Rx_0 = modp_Rx(slen as libc::c_uint, p_0, p0i_0, R2_0);
            v_0 = 0 as size_t;
            x = ft;
            y = gt;
            while v_0 < n {
                *fx.offset(v_0 as isize) = zint_mod_small_signed(x, slen, p_0, p0i_0, R2_0, Rx_0);
                *gx.offset(v_0 as isize) = zint_mod_small_signed(y, slen, p_0, p0i_0, R2_0, Rx_0);
                v_0 = v_0.wrapping_add(1);
                v_0;
                x = x.offset(slen as isize);
                y = y.offset(slen as isize);
            }
            modp_NTT2_ext(fx, 1 as size_t, gm, logn, p_0, p0i_0);
            modp_NTT2_ext(gx, 1 as size_t, gm, logn, p_0, p0i_0);
        }
        Fp = gx.offset(n as isize);
        Gp = Fp.offset(hn as isize);
        v_0 = 0 as size_t;
        x = Ft.offset(u as isize);
        y = Gt.offset(u as isize);
        while v_0 < hn {
            *Fp.offset(v_0 as isize) = *x;
            *Gp.offset(v_0 as isize) = *y;
            v_0 = v_0.wrapping_add(1);
            v_0;
            x = x.offset(llen as isize);
            y = y.offset(llen as isize);
        }
        modp_NTT2_ext(Fp, 1 as size_t, gm, logn.wrapping_sub(1 as libc::c_uint), p_0, p0i_0);
        modp_NTT2_ext(Gp, 1 as size_t, gm, logn.wrapping_sub(1 as libc::c_uint), p_0, p0i_0);
        v_0 = 0 as size_t;
        x = Ft.offset(u as isize);
        y = Gt.offset(u as isize);
        while v_0 < hn {
            let mut ftA: u32 = 0;
            let mut ftB: u32 = 0;
            let mut gtA: u32 = 0;
            let mut gtB: u32 = 0;
            let mut mFp: u32 = 0;
            let mut mGp: u32 = 0;
            ftA = *fx.offset((v_0 << 1).wrapping_add(0 as size_t) as isize);
            ftB = *fx.offset((v_0 << 1).wrapping_add(1 as size_t) as isize);
            gtA = *gx.offset((v_0 << 1).wrapping_add(0 as size_t) as isize);
            gtB = *gx.offset((v_0 << 1).wrapping_add(1 as size_t) as isize);
            mFp = modp_montymul(*Fp.offset(v_0 as isize), R2_0, p_0, p0i_0);
            mGp = modp_montymul(*Gp.offset(v_0 as isize), R2_0, p_0, p0i_0);
            *x.offset(0 as isize) = modp_montymul(gtB, mFp, p_0, p0i_0);
            *x.offset(llen as isize) = modp_montymul(gtA, mFp, p_0, p0i_0);
            *y.offset(0 as isize) = modp_montymul(ftB, mGp, p_0, p0i_0);
            *y.offset(llen as isize) = modp_montymul(ftA, mGp, p_0, p0i_0);
            v_0 = v_0.wrapping_add(1);
            v_0;
            x = x.offset((llen << 1) as isize);
            y = y.offset((llen << 1) as isize);
        }
        modp_iNTT2_ext(Ft.offset(u as isize), llen, igm, logn, p_0, p0i_0);
        modp_iNTT2_ext(Gt.offset(u as isize), llen, igm, logn, p_0, p0i_0);
        u = u.wrapping_add(1);
        u;
    }
    zint_rebuild_CRT(Ft, llen, llen, n, primes, 1, t1);
    zint_rebuild_CRT(Gt, llen, llen, n, primes, 1, t1);
    rt3 = align_fpr(tmp as *mut libc::c_void, t1 as *mut libc::c_void);
    rt4 = rt3.offset(n as isize);
    rt5 = rt4.offset(n as isize);
    rt1 = rt5.offset((n >> 1) as isize);
    k = align_u32(tmp as *mut libc::c_void, rt1 as *mut libc::c_void) as *mut int32_t;
    rt2 = align_fpr(tmp as *mut libc::c_void, k.offset(n as isize) as *mut libc::c_void);
    if rt2 < rt1.offset(n as isize) {
        rt2 = rt1.offset(n as isize);
    }
    t1 = (k as *mut u32).offset(n as isize);
    if slen > 10 as size_t {
        rlen = 10 as size_t;
    } else {
        rlen = slen;
    }
    poly_big_to_fp(rt3, ft.offset(slen as isize).offset(-(rlen as isize)), rlen, slen, logn);
    poly_big_to_fp(rt4, gt.offset(slen as isize).offset(-(rlen as isize)), rlen, slen, logn);
    scale_fg = 31 * slen.wrapping_sub(rlen) as libc::c_int;
    minbl_fg = BITLENGTH[depth as usize].avg - 6 * BITLENGTH[depth as usize].std;
    maxbl_fg = BITLENGTH[depth as usize].avg + 6 * BITLENGTH[depth as usize].std;
    PQCLEAN_FALCON512_CLEAN_FFT(rt3, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(rt4, logn);
    PQCLEAN_FALCON512_CLEAN_poly_invnorm2_fft(rt5, rt3, rt4, logn);
    PQCLEAN_FALCON512_CLEAN_poly_adj_fft(rt3, logn);
    PQCLEAN_FALCON512_CLEAN_poly_adj_fft(rt4, logn);
    FGlen = llen;
    maxbl_FG = 31 * llen as libc::c_int;
    scale_k = maxbl_FG - minbl_fg;
    loop {
        let mut scale_FG: libc::c_int = 0;
        let mut dc: libc::c_int = 0;
        let mut new_maxbl_FG: libc::c_int = 0;
        let mut scl: u32 = 0;
        let mut sch: u32 = 0;
        let mut pdc: fpr = 0;
        let mut pt: fpr = 0;
        if FGlen > 10 as size_t {
            rlen = 10 as size_t;
        } else {
            rlen = FGlen;
        }
        scale_FG = 31 * FGlen.wrapping_sub(rlen) as libc::c_int;
        poly_big_to_fp(rt1, Ft.offset(FGlen as isize).offset(-(rlen as isize)), rlen, llen, logn);
        poly_big_to_fp(rt2, Gt.offset(FGlen as isize).offset(-(rlen as isize)), rlen, llen, logn);
        PQCLEAN_FALCON512_CLEAN_FFT(rt1, logn);
        PQCLEAN_FALCON512_CLEAN_FFT(rt2, logn);
        PQCLEAN_FALCON512_CLEAN_poly_mul_fft(rt1, rt3, logn);
        PQCLEAN_FALCON512_CLEAN_poly_mul_fft(rt2, rt4, logn);
        PQCLEAN_FALCON512_CLEAN_poly_add(rt2, rt1, logn);
        PQCLEAN_FALCON512_CLEAN_poly_mul_autoadj_fft(rt2, rt5, logn);
        PQCLEAN_FALCON512_CLEAN_iFFT(rt2, logn);
        dc = scale_k - scale_FG + scale_fg;
        if dc < 0 {
            dc = -dc;
            pt = fpr_two;
        } else {
            pt = fpr_onehalf;
        }
        pdc = fpr_one;
        while dc != 0 {
            if dc & 1 != 0 {
                pdc = PQCLEAN_FALCON512_CLEAN_fpr_mul(pdc, pt);
            }
            dc >>= 1;
            pt = fpr_sqr(pt);
        }
        u = 0 as size_t;
        while u < n {
            let mut xv: fpr = 0;
            xv = PQCLEAN_FALCON512_CLEAN_fpr_mul(*rt2.offset(u as isize), pdc);
            if fpr_lt(fpr_mtwo31m1, xv) == 0 || fpr_lt(xv, fpr_ptwo31m1) == 0 {
                return 0;
            }
            *k.offset(u as isize) = fpr_rint(xv) as int32_t;
            u = u.wrapping_add(1);
            u;
        }
        sch = (scale_k / 31) as u32;
        scl = (scale_k % 31) as u32;
        if depth <= 4 as libc::c_uint {
            poly_sub_scaled_ntt(Ft, FGlen, llen, ft, slen, slen, k, sch, scl, logn, t1);
            poly_sub_scaled_ntt(Gt, FGlen, llen, gt, slen, slen, k, sch, scl, logn, t1);
        } else {
            poly_sub_scaled(Ft, FGlen, llen, ft, slen, slen, k, sch, scl, logn);
            poly_sub_scaled(Gt, FGlen, llen, gt, slen, slen, k, sch, scl, logn);
        }
        new_maxbl_FG = scale_k + maxbl_fg + 10;
        if new_maxbl_FG < maxbl_FG {
            maxbl_FG = new_maxbl_FG;
            if FGlen as libc::c_int * 31 >= maxbl_FG + 31 {
                FGlen = FGlen.wrapping_sub(1);
                FGlen;
            }
        }
        if scale_k <= 0 {
            break;
        }
        scale_k -= 25;
        if scale_k < 0 {
            scale_k = 0;
        }
    }
    if FGlen < slen {
        u = 0 as size_t;
        while u < n {
            let mut v_1: size_t = 0;
            let mut sw: u32 = 0;
            sw = (*Ft.offset(FGlen.wrapping_sub(1 as size_t) as isize) >> 30).wrapping_neg() >> 1;
            v_1 = FGlen;
            while v_1 < slen {
                *Ft.offset(v_1 as isize) = sw;
                v_1 = v_1.wrapping_add(1);
                v_1;
            }
            sw = (*Gt.offset(FGlen.wrapping_sub(1 as size_t) as isize) >> 30).wrapping_neg() >> 1;
            v_1 = FGlen;
            while v_1 < slen {
                *Gt.offset(v_1 as isize) = sw;
                v_1 = v_1.wrapping_add(1);
                v_1;
            }
            u = u.wrapping_add(1);
            u;
            Ft = Ft.offset(llen as isize);
            Gt = Gt.offset(llen as isize);
        }
    }
    u = 0 as size_t;
    x = tmp;
    y = tmp;
    while u < n << 1 {
        rust_memmove(
            x as *mut libc::c_void,
            y as *const libc::c_void,
            slen.wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
        );
        u = u.wrapping_add(1);
        u;
        x = x.offset(slen as isize);
        y = y.offset(llen as isize);
    }
    return 1;
}
unsafe extern "C" fn solve_NTRU_binary_depth1(
    mut logn_top: libc::c_uint,
    mut f: *const int8_t,
    mut g: *const int8_t,
    mut tmp: *mut u32,
) -> libc::c_int {
    let mut depth: libc::c_uint = 0;
    let mut logn: libc::c_uint = 0;
    let mut n_top: size_t = 0;
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut slen: size_t = 0;
    let mut dlen: size_t = 0;
    let mut llen: size_t = 0;
    let mut u: size_t = 0;
    let mut Fd: *mut u32 = 0 as *mut u32;
    let mut Gd: *mut u32 = 0 as *mut u32;
    let mut Ft: *mut u32 = 0 as *mut u32;
    let mut Gt: *mut u32 = 0 as *mut u32;
    let mut ft: *mut u32 = 0 as *mut u32;
    let mut gt: *mut u32 = 0 as *mut u32;
    let mut t1: *mut u32 = 0 as *mut u32;
    let mut rt1: *mut fpr = 0 as *mut fpr;
    let mut rt2: *mut fpr = 0 as *mut fpr;
    let mut rt3: *mut fpr = 0 as *mut fpr;
    let mut rt4: *mut fpr = 0 as *mut fpr;
    let mut rt5: *mut fpr = 0 as *mut fpr;
    let mut rt6: *mut fpr = 0 as *mut fpr;
    let mut x: *mut u32 = 0 as *mut u32;
    let mut y: *mut u32 = 0 as *mut u32;
    depth = 1 as libc::c_uint;
    n_top = (1 as size_t) << logn_top;
    logn = logn_top.wrapping_sub(depth);
    n = (1 as size_t) << logn;
    hn = n >> 1;
    slen = MAX_BL_SMALL[depth as usize];
    dlen = MAX_BL_SMALL[depth.wrapping_add(1 as libc::c_uint) as usize];
    llen = MAX_BL_LARGE[depth as usize];
    Fd = tmp;
    Gd = Fd.offset((dlen * hn) as isize);
    Ft = Gd.offset((dlen * hn) as isize);
    Gt = Ft.offset((llen * n) as isize);
    u = 0 as size_t;
    while u < llen {
        let mut p: u32 = 0;
        let mut p0i: u32 = 0;
        let mut R2: u32 = 0;
        let mut Rx: u32 = 0;
        let mut v: size_t = 0;
        let mut xs: *mut u32 = 0 as *mut u32;
        let mut ys: *mut u32 = 0 as *mut u32;
        let mut xd: *mut u32 = 0 as *mut u32;
        let mut yd: *mut u32 = 0 as *mut u32;
        p = PRIMES[u as usize].p;
        p0i = modp_ninv31(p);
        R2 = modp_R2(p, p0i);
        Rx = modp_Rx(dlen as libc::c_uint, p, p0i, R2);
        v = 0 as size_t;
        xs = Fd;
        ys = Gd;
        xd = Ft.offset(u as isize);
        yd = Gt.offset(u as isize);
        while v < hn {
            *xd = zint_mod_small_signed(xs, dlen, p, p0i, R2, Rx);
            *yd = zint_mod_small_signed(ys, dlen, p, p0i, R2, Rx);
            v = v.wrapping_add(1);
            v;
            xs = xs.offset(dlen as isize);
            ys = ys.offset(dlen as isize);
            xd = xd.offset(llen as isize);
            yd = yd.offset(llen as isize);
        }
        u = u.wrapping_add(1);
        u;
    }
    rust_memmove(
        tmp as *mut libc::c_void,
        Ft as *const libc::c_void,
        (llen * n).wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    Ft = tmp;
    rust_memmove(
        Ft.offset((llen * n) as isize) as *mut libc::c_void,
        Gt as *const libc::c_void,
        (llen * n).wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    Gt = Ft.offset((llen * n) as isize);
    ft = Gt.offset((llen * n) as isize);
    gt = ft.offset((slen * n) as isize);
    t1 = gt.offset((slen * n) as isize);
    u = 0 as size_t;
    while u < llen {
        let mut p_0: u32 = 0;
        let mut p0i_0: u32 = 0;
        let mut R2_0: u32 = 0;
        let mut gm: *mut u32 = 0 as *mut u32;
        let mut igm: *mut u32 = 0 as *mut u32;
        let mut fx: *mut u32 = 0 as *mut u32;
        let mut gx: *mut u32 = 0 as *mut u32;
        let mut Fp: *mut u32 = 0 as *mut u32;
        let mut Gp: *mut u32 = 0 as *mut u32;
        let mut e: libc::c_uint = 0;
        let mut v_0: size_t = 0;
        p_0 = PRIMES[u as usize].p;
        p0i_0 = modp_ninv31(p_0);
        R2_0 = modp_R2(p_0, p0i_0);
        gm = t1;
        igm = gm.offset(n_top as isize);
        fx = igm.offset(n as isize);
        gx = fx.offset(n_top as isize);
        modp_mkgm2(gm, igm, logn_top, PRIMES[u as usize].g, p_0, p0i_0);
        v_0 = 0 as size_t;
        while v_0 < n_top {
            *fx.offset(v_0 as isize) = modp_set(*f.offset(v_0 as isize) as int32_t, p_0);
            *gx.offset(v_0 as isize) = modp_set(*g.offset(v_0 as isize) as int32_t, p_0);
            v_0 = v_0.wrapping_add(1);
            v_0;
        }
        modp_NTT2_ext(fx, 1 as size_t, gm, logn_top, p_0, p0i_0);
        modp_NTT2_ext(gx, 1 as size_t, gm, logn_top, p_0, p0i_0);
        e = logn_top;
        while e > logn {
            modp_poly_rec_res(fx, e, p_0, p0i_0, R2_0);
            modp_poly_rec_res(gx, e, p_0, p0i_0, R2_0);
            e = e.wrapping_sub(1);
            e;
        }
        if depth > 0 as libc::c_uint {
            rust_memmove(
                gm.offset(n as isize) as *mut libc::c_void,
                igm as *const libc::c_void,
                n.wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
            );
            igm = gm.offset(n as isize);
            rust_memmove(
                igm.offset(n as isize) as *mut libc::c_void,
                fx as *const libc::c_void,
                n.wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
            );
            fx = igm.offset(n as isize);
            rust_memmove(
                fx.offset(n as isize) as *mut libc::c_void,
                gx as *const libc::c_void,
                n.wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
            );
            gx = fx.offset(n as isize);
        }
        Fp = gx.offset(n as isize);
        Gp = Fp.offset(hn as isize);
        v_0 = 0 as size_t;
        x = Ft.offset(u as isize);
        y = Gt.offset(u as isize);
        while v_0 < hn {
            *Fp.offset(v_0 as isize) = *x;
            *Gp.offset(v_0 as isize) = *y;
            v_0 = v_0.wrapping_add(1);
            v_0;
            x = x.offset(llen as isize);
            y = y.offset(llen as isize);
        }
        modp_NTT2_ext(Fp, 1 as size_t, gm, logn.wrapping_sub(1 as libc::c_uint), p_0, p0i_0);
        modp_NTT2_ext(Gp, 1 as size_t, gm, logn.wrapping_sub(1 as libc::c_uint), p_0, p0i_0);
        v_0 = 0 as size_t;
        x = Ft.offset(u as isize);
        y = Gt.offset(u as isize);
        while v_0 < hn {
            let mut ftA: u32 = 0;
            let mut ftB: u32 = 0;
            let mut gtA: u32 = 0;
            let mut gtB: u32 = 0;
            let mut mFp: u32 = 0;
            let mut mGp: u32 = 0;
            ftA = *fx.offset((v_0 << 1).wrapping_add(0 as size_t) as isize);
            ftB = *fx.offset((v_0 << 1).wrapping_add(1 as size_t) as isize);
            gtA = *gx.offset((v_0 << 1).wrapping_add(0 as size_t) as isize);
            gtB = *gx.offset((v_0 << 1).wrapping_add(1 as size_t) as isize);
            mFp = modp_montymul(*Fp.offset(v_0 as isize), R2_0, p_0, p0i_0);
            mGp = modp_montymul(*Gp.offset(v_0 as isize), R2_0, p_0, p0i_0);
            *x.offset(0 as isize) = modp_montymul(gtB, mFp, p_0, p0i_0);
            *x.offset(llen as isize) = modp_montymul(gtA, mFp, p_0, p0i_0);
            *y.offset(0 as isize) = modp_montymul(ftB, mGp, p_0, p0i_0);
            *y.offset(llen as isize) = modp_montymul(ftA, mGp, p_0, p0i_0);
            v_0 = v_0.wrapping_add(1);
            v_0;
            x = x.offset((llen << 1) as isize);
            y = y.offset((llen << 1) as isize);
        }
        modp_iNTT2_ext(Ft.offset(u as isize), llen, igm, logn, p_0, p0i_0);
        modp_iNTT2_ext(Gt.offset(u as isize), llen, igm, logn, p_0, p0i_0);
        if u < slen {
            modp_iNTT2_ext(fx, 1 as size_t, igm, logn, p_0, p0i_0);
            modp_iNTT2_ext(gx, 1 as size_t, igm, logn, p_0, p0i_0);
            v_0 = 0 as size_t;
            x = ft.offset(u as isize);
            y = gt.offset(u as isize);
            while v_0 < n {
                *x = *fx.offset(v_0 as isize);
                *y = *gx.offset(v_0 as isize);
                v_0 = v_0.wrapping_add(1);
                v_0;
                x = x.offset(slen as isize);
                y = y.offset(slen as isize);
            }
        }
        u = u.wrapping_add(1);
        u;
    }
    zint_rebuild_CRT(Ft, llen, llen, n << 1, PRIMES.as_ptr(), 1, t1);
    zint_rebuild_CRT(ft, slen, slen, n << 1, PRIMES.as_ptr(), 1, t1);
    rt1 = align_fpr(tmp as *mut libc::c_void, gt.offset((slen * n) as isize) as *mut libc::c_void);
    rt2 = rt1.offset(n as isize);
    poly_big_to_fp(rt1, Ft, llen, llen, logn);
    poly_big_to_fp(rt2, Gt, llen, llen, logn);
    rust_memmove(
        tmp as *mut libc::c_void,
        ft as *const libc::c_void,
        (2 as size_t * slen * n).wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    ft = tmp;
    gt = ft.offset((slen * n) as isize);
    rt3 = align_fpr(tmp as *mut libc::c_void, gt.offset((slen * n) as isize) as *mut libc::c_void);
    rust_memmove(
        rt3 as *mut libc::c_void,
        rt1 as *const libc::c_void,
        (2 as size_t * n).wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    rt1 = rt3;
    rt2 = rt1.offset(n as isize);
    rt3 = rt2.offset(n as isize);
    rt4 = rt3.offset(n as isize);
    poly_big_to_fp(rt3, ft, slen, slen, logn);
    poly_big_to_fp(rt4, gt, slen, slen, logn);
    rust_memmove(
        tmp as *mut libc::c_void,
        rt1 as *const libc::c_void,
        (4 as size_t * n).wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    rt1 = tmp as *mut fpr;
    rt2 = rt1.offset(n as isize);
    rt3 = rt2.offset(n as isize);
    rt4 = rt3.offset(n as isize);
    PQCLEAN_FALCON512_CLEAN_FFT(rt1, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(rt2, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(rt3, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(rt4, logn);
    rt5 = rt4.offset(n as isize);
    rt6 = rt5.offset(n as isize);
    PQCLEAN_FALCON512_CLEAN_poly_add_muladj_fft(rt5, rt1, rt2, rt3, rt4, logn);
    PQCLEAN_FALCON512_CLEAN_poly_invnorm2_fft(rt6, rt3, rt4, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mul_autoadj_fft(rt5, rt6, logn);
    PQCLEAN_FALCON512_CLEAN_iFFT(rt5, logn);
    u = 0 as size_t;
    while u < n {
        let mut z: fpr = 0;
        z = *rt5.offset(u as isize);
        if fpr_lt(z, fpr_ptwo63m1) == 0 || fpr_lt(fpr_mtwo63m1, z) == 0 {
            return 0;
        }
        *rt5.offset(u as isize) = fpr_of(fpr_rint(z));
        u = u.wrapping_add(1);
        u;
    }
    PQCLEAN_FALCON512_CLEAN_FFT(rt5, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(rt3, rt5, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(rt4, rt5, logn);
    PQCLEAN_FALCON512_CLEAN_poly_sub(rt1, rt3, logn);
    PQCLEAN_FALCON512_CLEAN_poly_sub(rt2, rt4, logn);
    PQCLEAN_FALCON512_CLEAN_iFFT(rt1, logn);
    PQCLEAN_FALCON512_CLEAN_iFFT(rt2, logn);
    Ft = tmp;
    Gt = Ft.offset(n as isize);
    rt3 = align_fpr(tmp as *mut libc::c_void, Gt.offset(n as isize) as *mut libc::c_void);
    rust_memmove(
        rt3 as *mut libc::c_void,
        rt1 as *const libc::c_void,
        (2 as size_t * n).wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    rt1 = rt3;
    rt2 = rt1.offset(n as isize);
    u = 0 as size_t;
    while u < n {
        *Ft.offset(u as isize) = fpr_rint(*rt1.offset(u as isize)) as u32;
        *Gt.offset(u as isize) = fpr_rint(*rt2.offset(u as isize)) as u32;
        u = u.wrapping_add(1);
        u;
    }
    return 1;
}
unsafe extern "C" fn solve_NTRU_binary_depth0(
    mut logn: libc::c_uint,
    mut f: *const int8_t,
    mut g: *const int8_t,
    mut tmp: *mut u32,
) -> libc::c_int {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    let mut p: u32 = 0;
    let mut p0i: u32 = 0;
    let mut R2: u32 = 0;
    let mut Fp: *mut u32 = 0 as *mut u32;
    let mut Gp: *mut u32 = 0 as *mut u32;
    let mut t1: *mut u32 = 0 as *mut u32;
    let mut t2: *mut u32 = 0 as *mut u32;
    let mut t3: *mut u32 = 0 as *mut u32;
    let mut t4: *mut u32 = 0 as *mut u32;
    let mut t5: *mut u32 = 0 as *mut u32;
    let mut gm: *mut u32 = 0 as *mut u32;
    let mut igm: *mut u32 = 0 as *mut u32;
    let mut ft: *mut u32 = 0 as *mut u32;
    let mut gt: *mut u32 = 0 as *mut u32;
    let mut rt2: *mut fpr = 0 as *mut fpr;
    let mut rt3: *mut fpr = 0 as *mut fpr;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    p = PRIMES[0 as usize].p;
    p0i = modp_ninv31(p);
    R2 = modp_R2(p, p0i);
    Fp = tmp;
    Gp = Fp.offset(hn as isize);
    ft = Gp.offset(hn as isize);
    gt = ft.offset(n as isize);
    gm = gt.offset(n as isize);
    igm = gm.offset(n as isize);
    modp_mkgm2(gm, igm, logn, PRIMES[0 as usize].g, p, p0i);
    u = 0 as size_t;
    while u < hn {
        *Fp.offset(u as isize) = modp_set(zint_one_to_plain(Fp.offset(u as isize)), p);
        *Gp.offset(u as isize) = modp_set(zint_one_to_plain(Gp.offset(u as isize)), p);
        u = u.wrapping_add(1);
        u;
    }
    modp_NTT2_ext(Fp, 1 as size_t, gm, logn.wrapping_sub(1 as libc::c_uint), p, p0i);
    modp_NTT2_ext(Gp, 1 as size_t, gm, logn.wrapping_sub(1 as libc::c_uint), p, p0i);
    u = 0 as size_t;
    while u < n {
        *ft.offset(u as isize) = modp_set(*f.offset(u as isize) as int32_t, p);
        *gt.offset(u as isize) = modp_set(*g.offset(u as isize) as int32_t, p);
        u = u.wrapping_add(1);
        u;
    }
    modp_NTT2_ext(ft, 1 as size_t, gm, logn, p, p0i);
    modp_NTT2_ext(gt, 1 as size_t, gm, logn, p, p0i);
    u = 0 as size_t;
    while u < n {
        let mut ftA: u32 = 0;
        let mut ftB: u32 = 0;
        let mut gtA: u32 = 0;
        let mut gtB: u32 = 0;
        let mut mFp: u32 = 0;
        let mut mGp: u32 = 0;
        ftA = *ft.offset(u.wrapping_add(0 as size_t) as isize);
        ftB = *ft.offset(u.wrapping_add(1 as size_t) as isize);
        gtA = *gt.offset(u.wrapping_add(0 as size_t) as isize);
        gtB = *gt.offset(u.wrapping_add(1 as size_t) as isize);
        mFp = modp_montymul(*Fp.offset((u >> 1) as isize), R2, p, p0i);
        mGp = modp_montymul(*Gp.offset((u >> 1) as isize), R2, p, p0i);
        *ft.offset(u.wrapping_add(0 as size_t) as isize) = modp_montymul(gtB, mFp, p, p0i);
        *ft.offset(u.wrapping_add(1 as size_t) as isize) = modp_montymul(gtA, mFp, p, p0i);
        *gt.offset(u.wrapping_add(0 as size_t) as isize) = modp_montymul(ftB, mGp, p, p0i);
        *gt.offset(u.wrapping_add(1 as size_t) as isize) = modp_montymul(ftA, mGp, p, p0i);
        u = u.wrapping_add(2 as size_t);
    }
    modp_iNTT2_ext(ft, 1 as size_t, igm, logn, p, p0i);
    modp_iNTT2_ext(gt, 1 as size_t, igm, logn, p, p0i);
    Gp = Fp.offset(n as isize);
    t1 = Gp.offset(n as isize);
    rust_memmove(
        Fp as *mut libc::c_void,
        ft as *const libc::c_void,
        (2 as size_t * n).wrapping_mul(::core::mem::size_of::<u32>() as libc::c_ulong),
    );
    t2 = t1.offset(n as isize);
    t3 = t2.offset(n as isize);
    t4 = t3.offset(n as isize);
    t5 = t4.offset(n as isize);
    modp_mkgm2(t1, t2, logn, PRIMES[0 as usize].g, p, p0i);
    modp_NTT2_ext(Fp, 1 as size_t, t1, logn, p, p0i);
    modp_NTT2_ext(Gp, 1 as size_t, t1, logn, p, p0i);
    let ref mut fresh4 = *t5.offset(0 as isize);
    *fresh4 = modp_set(*f.offset(0 as isize) as int32_t, p);
    *t4.offset(0 as isize) = *fresh4;
    u = 1 as size_t;
    while u < n {
        *t4.offset(u as isize) = modp_set(*f.offset(u as isize) as int32_t, p);
        *t5.offset(n.wrapping_sub(u) as isize) =
            modp_set(-(*f.offset(u as isize) as libc::c_int), p);
        u = u.wrapping_add(1);
        u;
    }
    modp_NTT2_ext(t4, 1 as size_t, t1, logn, p, p0i);
    modp_NTT2_ext(t5, 1 as size_t, t1, logn, p, p0i);
    u = 0 as size_t;
    while u < n {
        let mut w: u32 = 0;
        w = modp_montymul(*t5.offset(u as isize), R2, p, p0i);
        *t2.offset(u as isize) = modp_montymul(w, *Fp.offset(u as isize), p, p0i);
        *t3.offset(u as isize) = modp_montymul(w, *t4.offset(u as isize), p, p0i);
        u = u.wrapping_add(1);
        u;
    }
    let ref mut fresh5 = *t5.offset(0 as isize);
    *fresh5 = modp_set(*g.offset(0 as isize) as int32_t, p);
    *t4.offset(0 as isize) = *fresh5;
    u = 1 as size_t;
    while u < n {
        *t4.offset(u as isize) = modp_set(*g.offset(u as isize) as int32_t, p);
        *t5.offset(n.wrapping_sub(u) as isize) =
            modp_set(-(*g.offset(u as isize) as libc::c_int), p);
        u = u.wrapping_add(1);
        u;
    }
    modp_NTT2_ext(t4, 1 as size_t, t1, logn, p, p0i);
    modp_NTT2_ext(t5, 1 as size_t, t1, logn, p, p0i);
    u = 0 as size_t;
    while u < n {
        let mut w_0: u32 = 0;
        w_0 = modp_montymul(*t5.offset(u as isize), R2, p, p0i);
        *t2.offset(u as isize) =
            modp_add(*t2.offset(u as isize), modp_montymul(w_0, *Gp.offset(u as isize), p, p0i), p);
        *t3.offset(u as isize) =
            modp_add(*t3.offset(u as isize), modp_montymul(w_0, *t4.offset(u as isize), p, p0i), p);
        u = u.wrapping_add(1);
        u;
    }
    modp_mkgm2(t1, t4, logn, PRIMES[0 as usize].g, p, p0i);
    modp_iNTT2_ext(t2, 1 as size_t, t4, logn, p, p0i);
    modp_iNTT2_ext(t3, 1 as size_t, t4, logn, p, p0i);
    u = 0 as size_t;
    while u < n {
        *t1.offset(u as isize) = modp_norm(*t2.offset(u as isize), p) as u32;
        *t2.offset(u as isize) = modp_norm(*t3.offset(u as isize), p) as u32;
        u = u.wrapping_add(1);
        u;
    }
    rt3 = align_fpr(tmp as *mut libc::c_void, t3 as *mut libc::c_void);
    u = 0 as size_t;
    while u < n {
        *rt3.offset(u as isize) = fpr_of(*(t2 as *mut int32_t).offset(u as isize) as i64);
        u = u.wrapping_add(1);
        u;
    }
    PQCLEAN_FALCON512_CLEAN_FFT(rt3, logn);
    rt2 = align_fpr(tmp as *mut libc::c_void, t2 as *mut libc::c_void);
    rust_memmove(
        rt2 as *mut libc::c_void,
        rt3 as *const libc::c_void,
        hn.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    rt3 = rt2.offset(hn as isize);
    u = 0 as size_t;
    while u < n {
        *rt3.offset(u as isize) = fpr_of(*(t1 as *mut int32_t).offset(u as isize) as i64);
        u = u.wrapping_add(1);
        u;
    }
    PQCLEAN_FALCON512_CLEAN_FFT(rt3, logn);
    PQCLEAN_FALCON512_CLEAN_poly_div_autoadj_fft(rt3, rt2, logn);
    PQCLEAN_FALCON512_CLEAN_iFFT(rt3, logn);
    u = 0 as size_t;
    while u < n {
        *t1.offset(u as isize) = modp_set(fpr_rint(*rt3.offset(u as isize)) as int32_t, p);
        u = u.wrapping_add(1);
        u;
    }
    t2 = t1.offset(n as isize);
    t3 = t2.offset(n as isize);
    t4 = t3.offset(n as isize);
    t5 = t4.offset(n as isize);
    modp_mkgm2(t2, t3, logn, PRIMES[0 as usize].g, p, p0i);
    u = 0 as size_t;
    while u < n {
        *t4.offset(u as isize) = modp_set(*f.offset(u as isize) as int32_t, p);
        *t5.offset(u as isize) = modp_set(*g.offset(u as isize) as int32_t, p);
        u = u.wrapping_add(1);
        u;
    }
    modp_NTT2_ext(t1, 1 as size_t, t2, logn, p, p0i);
    modp_NTT2_ext(t4, 1 as size_t, t2, logn, p, p0i);
    modp_NTT2_ext(t5, 1 as size_t, t2, logn, p, p0i);
    u = 0 as size_t;
    while u < n {
        let mut kw: u32 = 0;
        kw = modp_montymul(*t1.offset(u as isize), R2, p, p0i);
        *Fp.offset(u as isize) =
            modp_sub(*Fp.offset(u as isize), modp_montymul(kw, *t4.offset(u as isize), p, p0i), p);
        *Gp.offset(u as isize) =
            modp_sub(*Gp.offset(u as isize), modp_montymul(kw, *t5.offset(u as isize), p, p0i), p);
        u = u.wrapping_add(1);
        u;
    }
    modp_iNTT2_ext(Fp, 1 as size_t, t3, logn, p, p0i);
    modp_iNTT2_ext(Gp, 1 as size_t, t3, logn, p, p0i);
    u = 0 as size_t;
    while u < n {
        *Fp.offset(u as isize) = modp_norm(*Fp.offset(u as isize), p) as u32;
        *Gp.offset(u as isize) = modp_norm(*Gp.offset(u as isize), p) as u32;
        u = u.wrapping_add(1);
        u;
    }
    return 1;
}
unsafe extern "C" fn solve_NTRU(
    mut logn: libc::c_uint,
    mut F: *mut int8_t,
    mut G: *mut int8_t,
    mut f: *const int8_t,
    mut g: *const int8_t,
    mut lim: libc::c_int,
    mut tmp: *mut u32,
) -> libc::c_int {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut ft: *mut u32 = 0 as *mut u32;
    let mut gt: *mut u32 = 0 as *mut u32;
    let mut Ft: *mut u32 = 0 as *mut u32;
    let mut Gt: *mut u32 = 0 as *mut u32;
    let mut gm: *mut u32 = 0 as *mut u32;
    let mut p: u32 = 0;
    let mut p0i: u32 = 0;
    let mut r: u32 = 0;
    let mut primes: *const small_prime = 0 as *const small_prime;
    n = (1 as size_t) << logn;
    if solve_NTRU_deepest(logn, f, g, tmp) == 0 {
        return 0;
    }
    if logn <= 2 as libc::c_uint {
        let mut depth: libc::c_uint = 0;
        depth = logn;
        loop {
            let fresh6 = depth;
            depth = depth.wrapping_sub(1);
            if !(fresh6 > 0 as libc::c_uint) {
                break;
            }
            if solve_NTRU_intermediate(logn, f, g, depth, tmp) == 0 {
                return 0;
            }
        }
    } else {
        let mut depth_0: libc::c_uint = 0;
        depth_0 = logn;
        loop {
            let fresh7 = depth_0;
            depth_0 = depth_0.wrapping_sub(1);
            if !(fresh7 > 2 as libc::c_uint) {
                break;
            }
            if solve_NTRU_intermediate(logn, f, g, depth_0, tmp) == 0 {
                return 0;
            }
        }
        if solve_NTRU_binary_depth1(logn, f, g, tmp) == 0 {
            return 0;
        }
        if solve_NTRU_binary_depth0(logn, f, g, tmp) == 0 {
            return 0;
        }
    }
    if G.is_null() {
        G = tmp.offset((2 as size_t * n) as isize) as *mut int8_t;
    }
    if poly_big_to_small(F, tmp, lim, logn) == 0
        || poly_big_to_small(G, tmp.offset(n as isize), lim, logn) == 0
    {
        return 0;
    }
    Gt = tmp;
    ft = Gt.offset(n as isize);
    gt = ft.offset(n as isize);
    Ft = gt.offset(n as isize);
    gm = Ft.offset(n as isize);
    primes = PRIMES.as_ptr();
    p = (*primes.offset(0 as isize)).p;
    p0i = modp_ninv31(p);
    modp_mkgm2(gm, tmp, logn, (*primes.offset(0 as isize)).g, p, p0i);
    u = 0 as size_t;
    while u < n {
        *Gt.offset(u as isize) = modp_set(*G.offset(u as isize) as int32_t, p);
        u = u.wrapping_add(1);
        u;
    }
    u = 0 as size_t;
    while u < n {
        *ft.offset(u as isize) = modp_set(*f.offset(u as isize) as int32_t, p);
        *gt.offset(u as isize) = modp_set(*g.offset(u as isize) as int32_t, p);
        *Ft.offset(u as isize) = modp_set(*F.offset(u as isize) as int32_t, p);
        u = u.wrapping_add(1);
        u;
    }
    modp_NTT2_ext(ft, 1 as size_t, gm, logn, p, p0i);
    modp_NTT2_ext(gt, 1 as size_t, gm, logn, p, p0i);
    modp_NTT2_ext(Ft, 1 as size_t, gm, logn, p, p0i);
    modp_NTT2_ext(Gt, 1 as size_t, gm, logn, p, p0i);
    r = modp_montymul(12289 as u32, 1 as u32, p, p0i);
    u = 0 as size_t;
    while u < n {
        let mut z: u32 = 0;
        z = modp_sub(
            modp_montymul(*ft.offset(u as isize), *Gt.offset(u as isize), p, p0i),
            modp_montymul(*gt.offset(u as isize), *Ft.offset(u as isize), p, p0i),
            p,
        );
        if z != r {
            return 0;
        }
        u = u.wrapping_add(1);
        u;
    }
    return 1;
}
unsafe extern "C" fn poly_small_mkgauss(
    mut rng: *mut shake256incctx,
    mut f: *mut int8_t,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut mod2: libc::c_uint = 0;
    n = (1 as size_t) << logn;
    mod2 = 0 as libc::c_uint;
    u = 0 as size_t;
    while u < n {
        let mut s: libc::c_int = 0;
        loop {
            s = mkgauss(rng, logn);
            if s < -(127) || s > 127 {
                continue;
            }
            if u == n.wrapping_sub(1 as size_t) {
                if !(mod2 ^ (s & 1) as libc::c_uint == 0 as libc::c_uint) {
                    break;
                }
            } else {
                mod2 ^= (s & 1) as libc::c_uint;
                break;
            }
        }
        *f.offset(u as isize) = s as int8_t;
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_keygen(
    mut rng: *mut shake256incctx,
    mut f: *mut int8_t,
    mut g: *mut int8_t,
    mut F: *mut int8_t,
    mut G: *mut int8_t,
    mut h: *mut uint16_t,
    mut logn: libc::c_uint,
    mut tmp: *mut uint8_t,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut h2: *mut uint16_t = 0 as *mut uint16_t;
    let mut tmp2: *mut uint16_t = 0 as *mut uint16_t;
    let mut rc: *mut shake256incctx = 0 as *mut shake256incctx;
    n = (1 as size_t) << logn;
    rc = rng;
    loop {
        let mut rt1: *mut fpr = 0 as *mut fpr;
        let mut rt2: *mut fpr = 0 as *mut fpr;
        let mut rt3: *mut fpr = 0 as *mut fpr;
        let mut bnorm: fpr = 0;
        let mut normf: u32 = 0;
        let mut normg: u32 = 0;
        let mut norm: u32 = 0;
        let mut lim: libc::c_int = 0;
        poly_small_mkgauss(rc, f, logn);
        poly_small_mkgauss(rc, g, logn);
        lim = (1)
            << *PQCLEAN_FALCON512_CLEAN_max_fg_bits.as_ptr().offset(logn as isize) as libc::c_int
                - 1;
        u = 0 as size_t;
        while u < n {
            if *f.offset(u as isize) as libc::c_int >= lim
                || *f.offset(u as isize) as libc::c_int <= -lim
                || *g.offset(u as isize) as libc::c_int >= lim
                || *g.offset(u as isize) as libc::c_int <= -lim
            {
                lim = -(1);
                break;
            } else {
                u = u.wrapping_add(1);
                u;
            }
        }
        if lim < 0 {
            continue;
        }
        normf = poly_small_sqnorm(f, logn);
        normg = poly_small_sqnorm(g, logn);
        norm = normf.wrapping_add(normg) | ((normf | normg) >> 31).wrapping_neg();
        if norm >= 16823 as u32 {
            continue;
        }
        rt1 = tmp as *mut fpr;
        rt2 = rt1.offset(n as isize);
        rt3 = rt2.offset(n as isize);
        poly_small_to_fp(rt1, f, logn);
        poly_small_to_fp(rt2, g, logn);
        PQCLEAN_FALCON512_CLEAN_FFT(rt1, logn);
        PQCLEAN_FALCON512_CLEAN_FFT(rt2, logn);
        PQCLEAN_FALCON512_CLEAN_poly_invnorm2_fft(rt3, rt1, rt2, logn);
        PQCLEAN_FALCON512_CLEAN_poly_adj_fft(rt1, logn);
        PQCLEAN_FALCON512_CLEAN_poly_adj_fft(rt2, logn);
        PQCLEAN_FALCON512_CLEAN_poly_mulconst(rt1, fpr_q, logn);
        PQCLEAN_FALCON512_CLEAN_poly_mulconst(rt2, fpr_q, logn);
        PQCLEAN_FALCON512_CLEAN_poly_mul_autoadj_fft(rt1, rt3, logn);
        PQCLEAN_FALCON512_CLEAN_poly_mul_autoadj_fft(rt2, rt3, logn);
        PQCLEAN_FALCON512_CLEAN_iFFT(rt1, logn);
        PQCLEAN_FALCON512_CLEAN_iFFT(rt2, logn);
        bnorm = fpr_zero;
        u = 0 as size_t;
        while u < n {
            bnorm = PQCLEAN_FALCON512_CLEAN_fpr_add(bnorm, fpr_sqr(*rt1.offset(u as isize)));
            bnorm = PQCLEAN_FALCON512_CLEAN_fpr_add(bnorm, fpr_sqr(*rt2.offset(u as isize)));
            u = u.wrapping_add(1);
            u;
        }
        if fpr_lt(bnorm, fpr_bnorm_max) == 0 {
            continue;
        }
        if h.is_null() {
            h2 = tmp as *mut uint16_t;
            tmp2 = h2.offset(n as isize);
        } else {
            h2 = h;
            tmp2 = tmp as *mut uint16_t;
        }
        if PQCLEAN_FALCON512_CLEAN_compute_public(h2, f, g, logn, tmp2 as *mut uint8_t) == 0 {
            continue;
        }
        lim = ((1)
            << *PQCLEAN_FALCON512_CLEAN_max_FG_bits.as_ptr().offset(logn as isize) as libc::c_int
                - 1)
            - 1;
        if !(solve_NTRU(logn, F, G, f, g, lim, tmp as *mut u32) == 0) {
            break;
        }
    }
}
