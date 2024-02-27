use crate::libc;
extern "C" {
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
    fn PQCLEAN_FALCON512_CLEAN_poly_merge_fft(
        f: *mut fpr,
        f0: *const fpr,
        f1: *const fpr,
        logn: libc::c_uint,
    );
    fn PQCLEAN_FALCON512_CLEAN_poly_split_fft(
        f0: *mut fpr,
        f1: *mut fpr,
        f: *const fpr,
        logn: libc::c_uint,
    );
    fn PQCLEAN_FALCON512_CLEAN_poly_LDLmv_fft(
        d11: *mut fpr,
        l10: *mut fpr,
        g00: *const fpr,
        g01: *const fpr,
        g11: *const fpr,
        logn: libc::c_uint,
    );
    fn PQCLEAN_FALCON512_CLEAN_poly_LDL_fft(
        g00: *const fpr,
        g01: *mut fpr,
        g11: *mut fpr,
        logn: libc::c_uint,
    );
    fn PQCLEAN_FALCON512_CLEAN_poly_mulconst(a: *mut fpr, x: fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(a: *mut fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_muladj_fft(a: *mut fpr, b: *const fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_is_short_half(
        sqn: u32,
        s2: *const int16_t,
        logn: libc::c_uint,
    ) -> libc::c_int;
    fn PQCLEAN_FALCON512_CLEAN_fpr_scaled(i: i64, sc: libc::c_int) -> fpr;
    fn PQCLEAN_FALCON512_CLEAN_fpr_add(x: fpr, y: fpr) -> fpr;
    fn PQCLEAN_FALCON512_CLEAN_fpr_mul(x: fpr, y: fpr) -> fpr;
    fn PQCLEAN_FALCON512_CLEAN_fpr_sqrt(x: fpr) -> fpr;
    fn PQCLEAN_FALCON512_CLEAN_fpr_expm_p63(x: fpr, ccs: fpr) -> u64;
    fn PQCLEAN_FALCON512_CLEAN_prng_init(p: *mut prng, src: *mut shake256incctx);
    fn PQCLEAN_FALCON512_CLEAN_prng_refill(p: *mut prng);
    fn PQCLEAN_FALCON512_CLEAN_FFT(f: *mut fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_iFFT(f: *mut fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_add(a: *mut fpr, b: *const fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_sub(a: *mut fpr, b: *const fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_neg(a: *mut fpr, logn: libc::c_uint);
    fn PQCLEAN_FALCON512_CLEAN_poly_mul_fft(a: *mut fpr, b: *const fpr, logn: libc::c_uint);
}
pub type __int8_t = libc::c_schar;
pub type __uint8_t = libc::c_uchar;
pub type __int16_t = libc::c_short;
pub type __uint16_t = libc::c_ushort;
pub type __int32_t = libc::c_int;
pub type __u32 = libc::c_uint;
pub type __i64 = libc::c_long;
pub type __u64 = libc::c_ulong;
pub type int8_t = __int8_t;
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
pub type fpr = u64;
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
pub type samplerZ = Option<unsafe extern "C" fn(*mut libc::c_void, fpr, fpr) -> libc::c_int>;
#[derive()]
#[repr(C)]
pub struct sampler_context {
    pub p: prng,
    pub sigma_min: fpr,
}
#[inline]
unsafe extern "C" fn fpr_ursh(mut x: u64, mut n: libc::c_int) -> u64 {
    x ^= (x ^ x >> 32) & ((n >> 5) as u64).wrapping_neg();
    return x >> (n & 31);
}
#[inline]
unsafe extern "C" fn fpr_irsh(mut x: i64, mut n: libc::c_int) -> i64 {
    x ^= (x ^ x >> 32) & -((n >> 5) as i64);
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
static mut fpr_inverse_of_q: fpr = 4545632735260551042 as fpr;
static mut fpr_inv_2sqrsigma0: fpr = 4594603506513722306 as fpr;
static mut fpr_inv_sigma: [fpr; 11] = [
    0 as fpr,
    4574611497772390042 as fpr,
    4574501679055810265 as fpr,
    4574396282908341804 as fpr,
    4574245855758572086 as fpr,
    4574103865040221165 as fpr,
    4573969550563515544 as fpr,
    4573842244705920822 as fpr,
    4573721358406441454 as fpr,
    4573606369665796042 as fpr,
    4573496814039276259 as fpr,
];
static mut fpr_sigma_min: [fpr; 11] = [
    0 as fpr,
    4607707126469777035 as fpr,
    4607777455861499430 as fpr,
    4607846828256951418 as fpr,
    4607949175006100261 as fpr,
    4608049571757433526 as fpr,
    4608148125896792003 as fpr,
    4608244935301382692 as fpr,
    4608340089478362016 as fpr,
    4608433670533905013 as fpr,
    4608525754002622308 as fpr,
];
static mut fpr_log2: fpr = 4604418534313441775 as fpr;
static mut fpr_inv_log2: fpr = 4609176140021203710 as fpr;
static mut fpr_invsqrt2: fpr = 4604544271217802189 as fpr;
static mut fpr_invsqrt8: fpr = 4600040671590431693 as fpr;
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
unsafe extern "C" fn fpr_floor(mut x: fpr) -> i64 {
    let mut t: u64 = 0;
    let mut xi: i64 = 0;
    let mut e: libc::c_int = 0;
    let mut cc: libc::c_int = 0;
    e = (x >> 52) as libc::c_int & 0x7ff;
    t = x >> 63;
    xi = ((x << 10 | (1 as u64) << 62) & ((1 as u64) << 63).wrapping_sub(1 as u64)) as i64;
    xi = (xi ^ -(t as i64)) + t as i64;
    cc = 1085 - e;
    xi = fpr_irsh(xi, cc & 63);
    xi ^= (xi ^ -(t as i64)) & -(((63 - cc) as u32 >> 31) as i64);
    return xi;
}
#[inline]
unsafe extern "C" fn fpr_trunc(mut x: fpr) -> i64 {
    let mut t: u64 = 0;
    let mut xu: u64 = 0;
    let mut e: libc::c_int = 0;
    let mut cc: libc::c_int = 0;
    e = (x >> 52) as libc::c_int & 0x7ff;
    xu = (x << 10 | (1 as u64) << 62) & ((1 as u64) << 63).wrapping_sub(1 as u64);
    cc = 1085 - e;
    xu = fpr_ursh(xu, cc & 63);
    xu &= (((cc - 64) as u32 >> 31) as u64).wrapping_neg();
    t = x >> 63;
    xu = (xu ^ t.wrapping_neg()).wrapping_add(t);
    return *(&mut xu as *mut u64 as *mut i64);
}
#[inline]
unsafe extern "C" fn fpr_sub(mut x: fpr, mut y: fpr) -> fpr {
    y ^= (1 as u64) << 63;
    return PQCLEAN_FALCON512_CLEAN_fpr_add(x, y);
}
#[inline]
unsafe extern "C" fn fpr_neg(mut x: fpr) -> fpr {
    x ^= (1 as u64) << 63;
    return x;
}
#[inline]
unsafe extern "C" fn fpr_half(mut x: fpr) -> fpr {
    let mut t: u32 = 0;
    x = (x as u64).wrapping_sub((1 as u64) << 52) as fpr as fpr;
    t = ((x >> 52) as u32 & 0x7ff as u32).wrapping_add(1 as u32) >> 11;
    x &= (t as u64).wrapping_sub(1 as u64);
    return x;
}
#[inline]
unsafe extern "C" fn fpr_sqr(mut x: fpr) -> fpr {
    return PQCLEAN_FALCON512_CLEAN_fpr_mul(x, x);
}
#[inline]
unsafe extern "C" fn prng_get_u64(mut p: *mut prng) -> u64 {
    let mut u: size_t = 0;
    u = (*p).ptr;
    if u >= (::core::mem::size_of::<[uint8_t; 512]>() as libc::c_ulong)
        .wrapping_sub(9 as libc::c_ulong)
    {
        PQCLEAN_FALCON512_CLEAN_prng_refill(p);
        u = 0 as size_t;
    }
    (*p).ptr = u.wrapping_add(8 as size_t);
    return (*p).buf.d[u.wrapping_add(0 as size_t) as usize] as u64
        | ((*p).buf.d[u.wrapping_add(1 as size_t) as usize] as u64) << 8
        | ((*p).buf.d[u.wrapping_add(2 as size_t) as usize] as u64) << 16
        | ((*p).buf.d[u.wrapping_add(3 as size_t) as usize] as u64) << 24
        | ((*p).buf.d[u.wrapping_add(4 as size_t) as usize] as u64) << 32
        | ((*p).buf.d[u.wrapping_add(5 as size_t) as usize] as u64) << 40
        | ((*p).buf.d[u.wrapping_add(6 as size_t) as usize] as u64) << 48
        | ((*p).buf.d[u.wrapping_add(7 as size_t) as usize] as u64) << 56;
}
#[inline]
unsafe extern "C" fn prng_get_u8(mut p: *mut prng) -> libc::c_uint {
    let mut v: libc::c_uint = 0;
    let fresh0 = (*p).ptr;
    (*p).ptr = ((*p).ptr).wrapping_add(1);
    v = (*p).buf.d[fresh0 as usize] as libc::c_uint;
    if (*p).ptr == ::core::mem::size_of::<[uint8_t; 512]>() as libc::c_ulong {
        PQCLEAN_FALCON512_CLEAN_prng_refill(p);
    }
    return v;
}
#[inline]
unsafe extern "C" fn ffLDL_treesize(mut logn: libc::c_uint) -> libc::c_uint {
    return logn.wrapping_add(1 as libc::c_uint) << logn;
}
unsafe extern "C" fn ffLDL_fft_inner(
    mut tree: *mut fpr,
    mut g0: *mut fpr,
    mut g1: *mut fpr,
    mut logn: libc::c_uint,
    mut tmp: *mut fpr,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    n = (1 as size_t) << logn;
    if n == 1 as size_t {
        *tree.offset(0 as isize) = *g0.offset(0 as isize);
        return;
    }
    hn = n >> 1;
    PQCLEAN_FALCON512_CLEAN_poly_LDLmv_fft(tmp, tree, g0, g1, g0, logn);
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(g1, g1.offset(hn as isize), g0, logn);
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(g0, g0.offset(hn as isize), tmp, logn);
    ffLDL_fft_inner(
        tree.offset(n as isize),
        g1,
        g1.offset(hn as isize),
        logn.wrapping_sub(1 as libc::c_uint),
        tmp,
    );
    ffLDL_fft_inner(
        tree.offset(n as isize)
            .offset(ffLDL_treesize(logn.wrapping_sub(1 as libc::c_uint)) as isize),
        g0,
        g0.offset(hn as isize),
        logn.wrapping_sub(1 as libc::c_uint),
        tmp,
    );
}
unsafe extern "C" fn ffLDL_fft(
    mut tree: *mut fpr,
    mut g00: *const fpr,
    mut g01: *const fpr,
    mut g11: *const fpr,
    mut logn: libc::c_uint,
    mut tmp: *mut fpr,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut d00: *mut fpr = 0 as *mut fpr;
    let mut d11: *mut fpr = 0 as *mut fpr;
    n = (1 as size_t) << logn;
    if n == 1 as size_t {
        *tree.offset(0 as isize) = *g00.offset(0 as isize);
        return;
    }
    hn = n >> 1;
    d00 = tmp;
    d11 = tmp.offset(n as isize);
    tmp = tmp.offset((n << 1) as isize);
    rust_memcpy(
        d00 as *mut libc::c_void,
        g00 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_LDLmv_fft(d11, tree, g00, g01, g11, logn);
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(tmp, tmp.offset(hn as isize), d00, logn);
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(d00, d00.offset(hn as isize), d11, logn);
    rust_memcpy(
        d11 as *mut libc::c_void,
        tmp as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    ffLDL_fft_inner(
        tree.offset(n as isize),
        d11,
        d11.offset(hn as isize),
        logn.wrapping_sub(1 as libc::c_uint),
        tmp,
    );
    ffLDL_fft_inner(
        tree.offset(n as isize)
            .offset(ffLDL_treesize(logn.wrapping_sub(1 as libc::c_uint)) as isize),
        d00,
        d00.offset(hn as isize),
        logn.wrapping_sub(1 as libc::c_uint),
        tmp,
    );
}
unsafe extern "C" fn ffLDL_binary_normalize(
    mut tree: *mut fpr,
    mut orig_logn: libc::c_uint,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    n = (1 as size_t) << logn;
    if n == 1 as size_t {
        *tree.offset(0 as isize) = PQCLEAN_FALCON512_CLEAN_fpr_mul(
            PQCLEAN_FALCON512_CLEAN_fpr_sqrt(*tree.offset(0 as isize)),
            fpr_inv_sigma[orig_logn as usize],
        );
    } else {
        ffLDL_binary_normalize(
            tree.offset(n as isize),
            orig_logn,
            logn.wrapping_sub(1 as libc::c_uint),
        );
        ffLDL_binary_normalize(
            tree.offset(n as isize)
                .offset(ffLDL_treesize(logn.wrapping_sub(1 as libc::c_uint)) as isize),
            orig_logn,
            logn.wrapping_sub(1 as libc::c_uint),
        );
    };
}
unsafe extern "C" fn smallints_to_fpr(
    mut r: *mut fpr,
    mut t: *const int8_t,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    u = 0 as size_t;
    while u < n {
        *r.offset(u as isize) = fpr_of(*t.offset(u as isize) as i64);
        u = u.wrapping_add(1);
        u;
    }
}
#[inline]
unsafe extern "C" fn skoff_b00(mut logn: libc::c_uint) -> size_t {
    return 0 as size_t;
}
#[inline]
unsafe extern "C" fn skoff_b01(mut logn: libc::c_uint) -> size_t {
    return (1 as size_t) << logn;
}
#[inline]
unsafe extern "C" fn skoff_b10(mut logn: libc::c_uint) -> size_t {
    return 2 as size_t * ((1 as size_t) << logn);
}
#[inline]
unsafe extern "C" fn skoff_b11(mut logn: libc::c_uint) -> size_t {
    return 3 as size_t * ((1 as size_t) << logn);
}
#[inline]
unsafe extern "C" fn skoff_tree(mut logn: libc::c_uint) -> size_t {
    return 4 as size_t * ((1 as size_t) << logn);
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_expand_privkey(
    mut expanded_key: *mut fpr,
    mut f: *const int8_t,
    mut g: *const int8_t,
    mut F: *const int8_t,
    mut G: *const int8_t,
    mut logn: libc::c_uint,
    mut tmp: *mut uint8_t,
) {
    let mut n: size_t = 0;
    let mut rf: *mut fpr = 0 as *mut fpr;
    let mut rg: *mut fpr = 0 as *mut fpr;
    let mut rF: *mut fpr = 0 as *mut fpr;
    let mut rG: *mut fpr = 0 as *mut fpr;
    let mut b00: *mut fpr = 0 as *mut fpr;
    let mut b01: *mut fpr = 0 as *mut fpr;
    let mut b10: *mut fpr = 0 as *mut fpr;
    let mut b11: *mut fpr = 0 as *mut fpr;
    let mut g00: *mut fpr = 0 as *mut fpr;
    let mut g01: *mut fpr = 0 as *mut fpr;
    let mut g11: *mut fpr = 0 as *mut fpr;
    let mut gxx: *mut fpr = 0 as *mut fpr;
    let mut tree: *mut fpr = 0 as *mut fpr;
    n = (1 as size_t) << logn;
    b00 = expanded_key.offset(skoff_b00(logn) as isize);
    b01 = expanded_key.offset(skoff_b01(logn) as isize);
    b10 = expanded_key.offset(skoff_b10(logn) as isize);
    b11 = expanded_key.offset(skoff_b11(logn) as isize);
    tree = expanded_key.offset(skoff_tree(logn) as isize);
    rf = b01;
    rg = b00;
    rF = b11;
    rG = b10;
    smallints_to_fpr(rf, f, logn);
    smallints_to_fpr(rg, g, logn);
    smallints_to_fpr(rF, F, logn);
    smallints_to_fpr(rG, G, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(rf, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(rg, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(rF, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(rG, logn);
    PQCLEAN_FALCON512_CLEAN_poly_neg(rf, logn);
    PQCLEAN_FALCON512_CLEAN_poly_neg(rF, logn);
    g00 = tmp as *mut fpr;
    g01 = g00.offset(n as isize);
    g11 = g01.offset(n as isize);
    gxx = g11.offset(n as isize);
    rust_memcpy(
        g00 as *mut libc::c_void,
        b00 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(g00, logn);
    rust_memcpy(
        gxx as *mut libc::c_void,
        b01 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(gxx, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(g00, gxx, logn);
    rust_memcpy(
        g01 as *mut libc::c_void,
        b00 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_muladj_fft(g01, b10, logn);
    rust_memcpy(
        gxx as *mut libc::c_void,
        b01 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_muladj_fft(gxx, b11, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(g01, gxx, logn);
    rust_memcpy(
        g11 as *mut libc::c_void,
        b10 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(g11, logn);
    rust_memcpy(
        gxx as *mut libc::c_void,
        b11 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(gxx, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(g11, gxx, logn);
    ffLDL_fft(tree, g00, g01, g11, logn, gxx);
    ffLDL_binary_normalize(tree, logn, logn);
}
unsafe extern "C" fn ffSampling_fft_dyntree(
    mut samp: samplerZ,
    mut samp_ctx: *mut libc::c_void,
    mut t0: *mut fpr,
    mut t1: *mut fpr,
    mut g00: *mut fpr,
    mut g01: *mut fpr,
    mut g11: *mut fpr,
    mut orig_logn: libc::c_uint,
    mut logn: libc::c_uint,
    mut tmp: *mut fpr,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut z0: *mut fpr = 0 as *mut fpr;
    let mut z1: *mut fpr = 0 as *mut fpr;
    if logn == 0 as libc::c_uint {
        let mut leaf: fpr = 0;
        leaf = *g00.offset(0 as isize);
        leaf = PQCLEAN_FALCON512_CLEAN_fpr_mul(
            PQCLEAN_FALCON512_CLEAN_fpr_sqrt(leaf),
            fpr_inv_sigma[orig_logn as usize],
        );
        *t0.offset(0 as isize) = fpr_of(samp.expect("non-null function pointer")(
            samp_ctx,
            *t0.offset(0 as isize),
            leaf,
        ) as i64);
        *t1.offset(0 as isize) = fpr_of(samp.expect("non-null function pointer")(
            samp_ctx,
            *t1.offset(0 as isize),
            leaf,
        ) as i64);
        return;
    }
    n = (1 as size_t) << logn;
    hn = n >> 1;
    PQCLEAN_FALCON512_CLEAN_poly_LDL_fft(g00, g01, g11, logn);
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(tmp, tmp.offset(hn as isize), g00, logn);
    rust_memcpy(
        g00 as *mut libc::c_void,
        tmp as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(tmp, tmp.offset(hn as isize), g11, logn);
    rust_memcpy(
        g11 as *mut libc::c_void,
        tmp as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    rust_memcpy(
        tmp as *mut libc::c_void,
        g01 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    rust_memcpy(
        g01 as *mut libc::c_void,
        g00 as *const libc::c_void,
        hn.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    rust_memcpy(
        g01.offset(hn as isize) as *mut libc::c_void,
        g11 as *const libc::c_void,
        hn.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    z1 = tmp.offset(n as isize);
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(z1, z1.offset(hn as isize), t1, logn);
    ffSampling_fft_dyntree(
        samp,
        samp_ctx,
        z1,
        z1.offset(hn as isize),
        g11,
        g11.offset(hn as isize),
        g01.offset(hn as isize),
        orig_logn,
        logn.wrapping_sub(1 as libc::c_uint),
        z1.offset(n as isize),
    );
    PQCLEAN_FALCON512_CLEAN_poly_merge_fft(
        tmp.offset((n << 1) as isize),
        z1,
        z1.offset(hn as isize),
        logn,
    );
    rust_memcpy(
        z1 as *mut libc::c_void,
        t1 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_sub(z1, tmp.offset((n << 1) as isize), logn);
    rust_memcpy(
        t1 as *mut libc::c_void,
        tmp.offset((n << 1) as isize) as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(tmp, z1, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(t0, tmp, logn);
    z0 = tmp;
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(z0, z0.offset(hn as isize), t0, logn);
    ffSampling_fft_dyntree(
        samp,
        samp_ctx,
        z0,
        z0.offset(hn as isize),
        g00,
        g00.offset(hn as isize),
        g01,
        orig_logn,
        logn.wrapping_sub(1 as libc::c_uint),
        z0.offset(n as isize),
    );
    PQCLEAN_FALCON512_CLEAN_poly_merge_fft(t0, z0, z0.offset(hn as isize), logn);
}
unsafe extern "C" fn ffSampling_fft(
    mut samp: samplerZ,
    mut samp_ctx: *mut libc::c_void,
    mut z0: *mut fpr,
    mut z1: *mut fpr,
    mut tree: *const fpr,
    mut t0: *const fpr,
    mut t1: *const fpr,
    mut logn: libc::c_uint,
    mut tmp: *mut fpr,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut tree0: *const fpr = 0 as *const fpr;
    let mut tree1: *const fpr = 0 as *const fpr;
    if logn == 2 as libc::c_uint {
        let mut x0: fpr = 0;
        let mut x1: fpr = 0;
        let mut y0: fpr = 0;
        let mut y1: fpr = 0;
        let mut w0: fpr = 0;
        let mut w1: fpr = 0;
        let mut w2: fpr = 0;
        let mut w3: fpr = 0;
        let mut sigma: fpr = 0;
        let mut a_re: fpr = 0;
        let mut a_im: fpr = 0;
        let mut b_re: fpr = 0;
        let mut b_im: fpr = 0;
        let mut c_re: fpr = 0;
        let mut c_im: fpr = 0;
        tree0 = tree.offset(4 as isize);
        tree1 = tree.offset(8 as isize);
        a_re = *t1.offset(0 as isize);
        a_im = *t1.offset(2 as isize);
        b_re = *t1.offset(1 as isize);
        b_im = *t1.offset(3 as isize);
        c_re = PQCLEAN_FALCON512_CLEAN_fpr_add(a_re, b_re);
        c_im = PQCLEAN_FALCON512_CLEAN_fpr_add(a_im, b_im);
        w0 = fpr_half(c_re);
        w1 = fpr_half(c_im);
        c_re = fpr_sub(a_re, b_re);
        c_im = fpr_sub(a_im, b_im);
        w2 = PQCLEAN_FALCON512_CLEAN_fpr_mul(
            PQCLEAN_FALCON512_CLEAN_fpr_add(c_re, c_im),
            fpr_invsqrt8,
        );
        w3 = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_sub(c_im, c_re), fpr_invsqrt8);
        x0 = w2;
        x1 = w3;
        sigma = *tree1.offset(3 as isize);
        w2 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x0, sigma) as i64);
        w3 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x1, sigma) as i64);
        a_re = fpr_sub(x0, w2);
        a_im = fpr_sub(x1, w3);
        b_re = *tree1.offset(0 as isize);
        b_im = *tree1.offset(1 as isize);
        c_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re, b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im, b_im),
        );
        c_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re, b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im, b_re),
        );
        x0 = PQCLEAN_FALCON512_CLEAN_fpr_add(c_re, w0);
        x1 = PQCLEAN_FALCON512_CLEAN_fpr_add(c_im, w1);
        sigma = *tree1.offset(2 as isize);
        w0 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x0, sigma) as i64);
        w1 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x1, sigma) as i64);
        a_re = w0;
        a_im = w1;
        b_re = w2;
        b_im = w3;
        c_re = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_sub(b_re, b_im), fpr_invsqrt2);
        c_im = PQCLEAN_FALCON512_CLEAN_fpr_mul(
            PQCLEAN_FALCON512_CLEAN_fpr_add(b_re, b_im),
            fpr_invsqrt2,
        );
        w0 = PQCLEAN_FALCON512_CLEAN_fpr_add(a_re, c_re);
        *z1.offset(0 as isize) = w0;
        w2 = PQCLEAN_FALCON512_CLEAN_fpr_add(a_im, c_im);
        *z1.offset(2 as isize) = w2;
        w1 = fpr_sub(a_re, c_re);
        *z1.offset(1 as isize) = w1;
        w3 = fpr_sub(a_im, c_im);
        *z1.offset(3 as isize) = w3;
        w0 = fpr_sub(*t1.offset(0 as isize), w0);
        w1 = fpr_sub(*t1.offset(1 as isize), w1);
        w2 = fpr_sub(*t1.offset(2 as isize), w2);
        w3 = fpr_sub(*t1.offset(3 as isize), w3);
        a_re = w0;
        a_im = w2;
        b_re = *tree.offset(0 as isize);
        b_im = *tree.offset(2 as isize);
        w0 = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re, b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im, b_im),
        );
        w2 = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re, b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im, b_re),
        );
        a_re = w1;
        a_im = w3;
        b_re = *tree.offset(1 as isize);
        b_im = *tree.offset(3 as isize);
        w1 = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re, b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im, b_im),
        );
        w3 = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re, b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im, b_re),
        );
        w0 = PQCLEAN_FALCON512_CLEAN_fpr_add(w0, *t0.offset(0 as isize));
        w1 = PQCLEAN_FALCON512_CLEAN_fpr_add(w1, *t0.offset(1 as isize));
        w2 = PQCLEAN_FALCON512_CLEAN_fpr_add(w2, *t0.offset(2 as isize));
        w3 = PQCLEAN_FALCON512_CLEAN_fpr_add(w3, *t0.offset(3 as isize));
        a_re = w0;
        a_im = w2;
        b_re = w1;
        b_im = w3;
        c_re = PQCLEAN_FALCON512_CLEAN_fpr_add(a_re, b_re);
        c_im = PQCLEAN_FALCON512_CLEAN_fpr_add(a_im, b_im);
        w0 = fpr_half(c_re);
        w1 = fpr_half(c_im);
        c_re = fpr_sub(a_re, b_re);
        c_im = fpr_sub(a_im, b_im);
        w2 = PQCLEAN_FALCON512_CLEAN_fpr_mul(
            PQCLEAN_FALCON512_CLEAN_fpr_add(c_re, c_im),
            fpr_invsqrt8,
        );
        w3 = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_sub(c_im, c_re), fpr_invsqrt8);
        x0 = w2;
        x1 = w3;
        sigma = *tree0.offset(3 as isize);
        y0 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x0, sigma) as i64);
        w2 = y0;
        y1 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x1, sigma) as i64);
        w3 = y1;
        a_re = fpr_sub(x0, y0);
        a_im = fpr_sub(x1, y1);
        b_re = *tree0.offset(0 as isize);
        b_im = *tree0.offset(1 as isize);
        c_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re, b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im, b_im),
        );
        c_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re, b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im, b_re),
        );
        x0 = PQCLEAN_FALCON512_CLEAN_fpr_add(c_re, w0);
        x1 = PQCLEAN_FALCON512_CLEAN_fpr_add(c_im, w1);
        sigma = *tree0.offset(2 as isize);
        w0 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x0, sigma) as i64);
        w1 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x1, sigma) as i64);
        a_re = w0;
        a_im = w1;
        b_re = w2;
        b_im = w3;
        c_re = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_sub(b_re, b_im), fpr_invsqrt2);
        c_im = PQCLEAN_FALCON512_CLEAN_fpr_mul(
            PQCLEAN_FALCON512_CLEAN_fpr_add(b_re, b_im),
            fpr_invsqrt2,
        );
        *z0.offset(0 as isize) = PQCLEAN_FALCON512_CLEAN_fpr_add(a_re, c_re);
        *z0.offset(2 as isize) = PQCLEAN_FALCON512_CLEAN_fpr_add(a_im, c_im);
        *z0.offset(1 as isize) = fpr_sub(a_re, c_re);
        *z0.offset(3 as isize) = fpr_sub(a_im, c_im);
        return;
    }
    if logn == 1 as libc::c_uint {
        let mut x0_0: fpr = 0;
        let mut x1_0: fpr = 0;
        let mut y0_0: fpr = 0;
        let mut y1_0: fpr = 0;
        let mut sigma_0: fpr = 0;
        let mut a_re_0: fpr = 0;
        let mut a_im_0: fpr = 0;
        let mut b_re_0: fpr = 0;
        let mut b_im_0: fpr = 0;
        let mut c_re_0: fpr = 0;
        let mut c_im_0: fpr = 0;
        x0_0 = *t1.offset(0 as isize);
        x1_0 = *t1.offset(1 as isize);
        sigma_0 = *tree.offset(3 as isize);
        y0_0 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x0_0, sigma_0) as i64);
        *z1.offset(0 as isize) = y0_0;
        y1_0 = fpr_of(samp.expect("non-null function pointer")(samp_ctx, x1_0, sigma_0) as i64);
        *z1.offset(1 as isize) = y1_0;
        a_re_0 = fpr_sub(x0_0, y0_0);
        a_im_0 = fpr_sub(x1_0, y1_0);
        b_re_0 = *tree.offset(0 as isize);
        b_im_0 = *tree.offset(1 as isize);
        c_re_0 = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re_0, b_re_0),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im_0, b_im_0),
        );
        c_im_0 = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_re_0, b_im_0),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(a_im_0, b_re_0),
        );
        x0_0 = PQCLEAN_FALCON512_CLEAN_fpr_add(c_re_0, *t0.offset(0 as isize));
        x1_0 = PQCLEAN_FALCON512_CLEAN_fpr_add(c_im_0, *t0.offset(1 as isize));
        sigma_0 = *tree.offset(2 as isize);
        *z0.offset(0 as isize) =
            fpr_of(samp.expect("non-null function pointer")(samp_ctx, x0_0, sigma_0) as i64);
        *z0.offset(1 as isize) =
            fpr_of(samp.expect("non-null function pointer")(samp_ctx, x1_0, sigma_0) as i64);
        return;
    }
    n = (1 as size_t) << logn;
    hn = n >> 1;
    tree0 = tree.offset(n as isize);
    tree1 = tree
        .offset(n as isize)
        .offset(ffLDL_treesize(logn.wrapping_sub(1 as libc::c_uint)) as isize);
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(z1, z1.offset(hn as isize), t1, logn);
    ffSampling_fft(
        samp,
        samp_ctx,
        tmp,
        tmp.offset(hn as isize),
        tree1,
        z1,
        z1.offset(hn as isize),
        logn.wrapping_sub(1 as libc::c_uint),
        tmp.offset(n as isize),
    );
    PQCLEAN_FALCON512_CLEAN_poly_merge_fft(z1, tmp, tmp.offset(hn as isize), logn);
    rust_memcpy(
        tmp as *mut libc::c_void,
        t1 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_sub(tmp, z1, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(tmp, tree, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(tmp, t0, logn);
    PQCLEAN_FALCON512_CLEAN_poly_split_fft(z0, z0.offset(hn as isize), tmp, logn);
    ffSampling_fft(
        samp,
        samp_ctx,
        tmp,
        tmp.offset(hn as isize),
        tree0,
        z0,
        z0.offset(hn as isize),
        logn.wrapping_sub(1 as libc::c_uint),
        tmp.offset(n as isize),
    );
    PQCLEAN_FALCON512_CLEAN_poly_merge_fft(z0, tmp, tmp.offset(hn as isize), logn);
}
unsafe extern "C" fn do_sign_tree(
    mut samp: samplerZ,
    mut samp_ctx: *mut libc::c_void,
    mut s2: *mut int16_t,
    mut expanded_key: *const fpr,
    mut hm: *const uint16_t,
    mut logn: libc::c_uint,
    mut tmp: *mut fpr,
) -> libc::c_int {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut t0: *mut fpr = 0 as *mut fpr;
    let mut t1: *mut fpr = 0 as *mut fpr;
    let mut tx: *mut fpr = 0 as *mut fpr;
    let mut ty: *mut fpr = 0 as *mut fpr;
    let mut b00: *const fpr = 0 as *const fpr;
    let mut b01: *const fpr = 0 as *const fpr;
    let mut b10: *const fpr = 0 as *const fpr;
    let mut b11: *const fpr = 0 as *const fpr;
    let mut tree: *const fpr = 0 as *const fpr;
    let mut ni: fpr = 0;
    let mut sqn: u32 = 0;
    let mut ng: u32 = 0;
    let mut s1tmp: *mut int16_t = 0 as *mut int16_t;
    let mut s2tmp: *mut int16_t = 0 as *mut int16_t;
    n = (1 as size_t) << logn;
    t0 = tmp;
    t1 = t0.offset(n as isize);
    b00 = expanded_key.offset(skoff_b00(logn) as isize);
    b01 = expanded_key.offset(skoff_b01(logn) as isize);
    b10 = expanded_key.offset(skoff_b10(logn) as isize);
    b11 = expanded_key.offset(skoff_b11(logn) as isize);
    tree = expanded_key.offset(skoff_tree(logn) as isize);
    u = 0 as size_t;
    while u < n {
        *t0.offset(u as isize) = fpr_of(*hm.offset(u as isize) as i64);
        u = u.wrapping_add(1);
        u;
    }
    PQCLEAN_FALCON512_CLEAN_FFT(t0, logn);
    ni = fpr_inverse_of_q;
    rust_memcpy(
        t1 as *mut libc::c_void,
        t0 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(t1, b01, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mulconst(t1, fpr_neg(ni), logn);
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(t0, b11, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mulconst(t0, ni, logn);
    tx = t1.offset(n as isize);
    ty = tx.offset(n as isize);
    ffSampling_fft(samp, samp_ctx, tx, ty, tree, t0, t1, logn, ty.offset(n as isize));
    rust_memcpy(
        t0 as *mut libc::c_void,
        tx as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    rust_memcpy(
        t1 as *mut libc::c_void,
        ty as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(tx, b00, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(ty, b10, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(tx, ty, logn);
    rust_memcpy(
        ty as *mut libc::c_void,
        t0 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(ty, b01, logn);
    rust_memcpy(
        t0 as *mut libc::c_void,
        tx as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(t1, b11, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(t1, ty, logn);
    PQCLEAN_FALCON512_CLEAN_iFFT(t0, logn);
    PQCLEAN_FALCON512_CLEAN_iFFT(t1, logn);
    s1tmp = tx as *mut int16_t;
    sqn = 0 as u32;
    ng = 0 as u32;
    u = 0 as size_t;
    while u < n {
        let mut z: int32_t = 0;
        z = *hm.offset(u as isize) as int32_t - fpr_rint(*t0.offset(u as isize)) as int32_t;
        sqn = sqn.wrapping_add((z * z) as u32);
        ng |= sqn;
        *s1tmp.offset(u as isize) = z as int16_t;
        u = u.wrapping_add(1);
        u;
    }
    sqn |= (ng >> 31).wrapping_neg();
    s2tmp = tmp as *mut int16_t;
    u = 0 as size_t;
    while u < n {
        *s2tmp.offset(u as isize) = -fpr_rint(*t1.offset(u as isize)) as int16_t;
        u = u.wrapping_add(1);
        u;
    }
    if PQCLEAN_FALCON512_CLEAN_is_short_half(sqn, s2tmp, logn) != 0 {
        rust_memcpy(
            s2 as *mut libc::c_void,
            s2tmp as *const libc::c_void,
            n.wrapping_mul(::core::mem::size_of::<int16_t>() as libc::c_ulong),
        );
        rust_memcpy(
            tmp as *mut libc::c_void,
            s1tmp as *const libc::c_void,
            n.wrapping_mul(::core::mem::size_of::<int16_t>() as libc::c_ulong),
        );
        return 1;
    }
    return 0;
}
unsafe extern "C" fn do_sign_dyn(
    mut samp: samplerZ,
    mut samp_ctx: *mut libc::c_void,
    mut s2: *mut int16_t,
    mut f: *const int8_t,
    mut g: *const int8_t,
    mut F: *const int8_t,
    mut G: *const int8_t,
    mut hm: *const uint16_t,
    mut logn: libc::c_uint,
    mut tmp: *mut fpr,
) -> libc::c_int {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    let mut t0: *mut fpr = 0 as *mut fpr;
    let mut t1: *mut fpr = 0 as *mut fpr;
    let mut tx: *mut fpr = 0 as *mut fpr;
    let mut ty: *mut fpr = 0 as *mut fpr;
    let mut b00: *mut fpr = 0 as *mut fpr;
    let mut b01: *mut fpr = 0 as *mut fpr;
    let mut b10: *mut fpr = 0 as *mut fpr;
    let mut b11: *mut fpr = 0 as *mut fpr;
    let mut g00: *mut fpr = 0 as *mut fpr;
    let mut g01: *mut fpr = 0 as *mut fpr;
    let mut g11: *mut fpr = 0 as *mut fpr;
    let mut ni: fpr = 0;
    let mut sqn: u32 = 0;
    let mut ng: u32 = 0;
    let mut s1tmp: *mut int16_t = 0 as *mut int16_t;
    let mut s2tmp: *mut int16_t = 0 as *mut int16_t;
    n = (1 as size_t) << logn;
    b00 = tmp;
    b01 = b00.offset(n as isize);
    b10 = b01.offset(n as isize);
    b11 = b10.offset(n as isize);
    smallints_to_fpr(b01, f, logn);
    smallints_to_fpr(b00, g, logn);
    smallints_to_fpr(b11, F, logn);
    smallints_to_fpr(b10, G, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(b01, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(b00, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(b11, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(b10, logn);
    PQCLEAN_FALCON512_CLEAN_poly_neg(b01, logn);
    PQCLEAN_FALCON512_CLEAN_poly_neg(b11, logn);
    t0 = b11.offset(n as isize);
    t1 = t0.offset(n as isize);
    rust_memcpy(
        t0 as *mut libc::c_void,
        b01 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(t0, logn);
    rust_memcpy(
        t1 as *mut libc::c_void,
        b00 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_muladj_fft(t1, b10, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(b00, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(b00, t0, logn);
    rust_memcpy(
        t0 as *mut libc::c_void,
        b01 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_muladj_fft(b01, b11, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(b01, t1, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(b10, logn);
    rust_memcpy(
        t1 as *mut libc::c_void,
        b11 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(t1, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(b10, t1, logn);
    g00 = b00;
    g01 = b01;
    g11 = b10;
    b01 = t0;
    t0 = b01.offset(n as isize);
    t1 = t0.offset(n as isize);
    u = 0 as size_t;
    while u < n {
        *t0.offset(u as isize) = fpr_of(*hm.offset(u as isize) as i64);
        u = u.wrapping_add(1);
        u;
    }
    PQCLEAN_FALCON512_CLEAN_FFT(t0, logn);
    ni = fpr_inverse_of_q;
    rust_memcpy(
        t1 as *mut libc::c_void,
        t0 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(t1, b01, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mulconst(t1, fpr_neg(ni), logn);
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(t0, b11, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mulconst(t0, ni, logn);
    rust_memcpy(
        b11 as *mut libc::c_void,
        t0 as *const libc::c_void,
        (n * 2 as size_t).wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    t0 = g11.offset(n as isize);
    t1 = t0.offset(n as isize);
    ffSampling_fft_dyntree(
        samp,
        samp_ctx,
        t0,
        t1,
        g00,
        g01,
        g11,
        logn,
        logn,
        t1.offset(n as isize),
    );
    b00 = tmp;
    b01 = b00.offset(n as isize);
    b10 = b01.offset(n as isize);
    b11 = b10.offset(n as isize);
    rust_memmove(
        b11.offset(n as isize) as *mut libc::c_void,
        t0 as *const libc::c_void,
        (n * 2 as size_t).wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    t0 = b11.offset(n as isize);
    t1 = t0.offset(n as isize);
    smallints_to_fpr(b01, f, logn);
    smallints_to_fpr(b00, g, logn);
    smallints_to_fpr(b11, F, logn);
    smallints_to_fpr(b10, G, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(b01, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(b00, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(b11, logn);
    PQCLEAN_FALCON512_CLEAN_FFT(b10, logn);
    PQCLEAN_FALCON512_CLEAN_poly_neg(b01, logn);
    PQCLEAN_FALCON512_CLEAN_poly_neg(b11, logn);
    tx = t1.offset(n as isize);
    ty = tx.offset(n as isize);
    rust_memcpy(
        tx as *mut libc::c_void,
        t0 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    rust_memcpy(
        ty as *mut libc::c_void,
        t1 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(tx, b00, logn);
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(ty, b10, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(tx, ty, logn);
    rust_memcpy(
        ty as *mut libc::c_void,
        t0 as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(ty, b01, logn);
    rust_memcpy(
        t0 as *mut libc::c_void,
        tx as *const libc::c_void,
        n.wrapping_mul(::core::mem::size_of::<fpr>() as libc::c_ulong),
    );
    PQCLEAN_FALCON512_CLEAN_poly_mul_fft(t1, b11, logn);
    PQCLEAN_FALCON512_CLEAN_poly_add(t1, ty, logn);
    PQCLEAN_FALCON512_CLEAN_iFFT(t0, logn);
    PQCLEAN_FALCON512_CLEAN_iFFT(t1, logn);
    s1tmp = tx as *mut int16_t;
    sqn = 0 as u32;
    ng = 0 as u32;
    u = 0 as size_t;
    while u < n {
        let mut z: int32_t = 0;
        z = *hm.offset(u as isize) as int32_t - fpr_rint(*t0.offset(u as isize)) as int32_t;
        sqn = sqn.wrapping_add((z * z) as u32);
        ng |= sqn;
        *s1tmp.offset(u as isize) = z as int16_t;
        u = u.wrapping_add(1);
        u;
    }
    sqn |= (ng >> 31).wrapping_neg();
    s2tmp = tmp as *mut int16_t;
    u = 0 as size_t;
    while u < n {
        *s2tmp.offset(u as isize) = -fpr_rint(*t1.offset(u as isize)) as int16_t;
        u = u.wrapping_add(1);
        u;
    }
    if PQCLEAN_FALCON512_CLEAN_is_short_half(sqn, s2tmp, logn) != 0 {
        rust_memcpy(
            s2 as *mut libc::c_void,
            s2tmp as *const libc::c_void,
            n.wrapping_mul(::core::mem::size_of::<int16_t>() as libc::c_ulong),
        );
        rust_memcpy(
            tmp as *mut libc::c_void,
            s1tmp as *const libc::c_void,
            n.wrapping_mul(::core::mem::size_of::<int16_t>() as libc::c_ulong),
        );
        return 1;
    }
    return 0;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_gaussian0_sampler(
    mut p: *mut prng,
) -> libc::c_int {
    static mut dist: [u32; 54] = [
        10745844 as libc::c_uint,
        3068844 as libc::c_uint,
        3741698 as libc::c_uint,
        5559083 as libc::c_uint,
        1580863 as libc::c_uint,
        8248194 as libc::c_uint,
        2260429 as libc::c_uint,
        13669192 as libc::c_uint,
        2736639 as libc::c_uint,
        708981 as libc::c_uint,
        4421575 as libc::c_uint,
        10046180 as libc::c_uint,
        169348 as libc::c_uint,
        7122675 as libc::c_uint,
        4136815 as libc::c_uint,
        30538 as libc::c_uint,
        13063405 as libc::c_uint,
        7650655 as libc::c_uint,
        4132 as libc::c_uint,
        14505003 as libc::c_uint,
        7826148 as libc::c_uint,
        417 as libc::c_uint,
        16768101 as libc::c_uint,
        11363290 as libc::c_uint,
        31 as libc::c_uint,
        8444042 as libc::c_uint,
        8086568 as libc::c_uint,
        1 as libc::c_uint,
        12844466 as libc::c_uint,
        265321 as libc::c_uint,
        0 as libc::c_uint,
        1232676 as libc::c_uint,
        13644283 as libc::c_uint,
        0 as libc::c_uint,
        38047 as libc::c_uint,
        9111839 as libc::c_uint,
        0 as libc::c_uint,
        870 as libc::c_uint,
        6138264 as libc::c_uint,
        0 as libc::c_uint,
        14 as libc::c_uint,
        12545723 as libc::c_uint,
        0 as libc::c_uint,
        0 as libc::c_uint,
        3104126 as libc::c_uint,
        0 as libc::c_uint,
        0 as libc::c_uint,
        28824 as libc::c_uint,
        0 as libc::c_uint,
        0 as libc::c_uint,
        198 as libc::c_uint,
        0 as libc::c_uint,
        0 as libc::c_uint,
        1 as libc::c_uint,
    ];
    let mut v0: u32 = 0;
    let mut v1: u32 = 0;
    let mut v2: u32 = 0;
    let mut hi: u32 = 0;
    let mut lo: u64 = 0;
    let mut u: size_t = 0;
    let mut z: libc::c_int = 0;
    lo = prng_get_u64(p);
    hi = prng_get_u8(p);
    v0 = lo as u32 & 0xffffff as u32;
    v1 = (lo >> 24) as u32 & 0xffffff as u32;
    v2 = (lo >> 48) as u32 | hi << 16;
    z = 0;
    u = 0 as size_t;
    while u
        < (::core::mem::size_of::<[u32; 54]>() as libc::c_ulong)
            .wrapping_div(::core::mem::size_of::<u32>() as libc::c_ulong)
    {
        let mut w0: u32 = 0;
        let mut w1: u32 = 0;
        let mut w2: u32 = 0;
        let mut cc: u32 = 0;
        w0 = dist[u.wrapping_add(2 as size_t) as usize];
        w1 = dist[u.wrapping_add(1 as size_t) as usize];
        w2 = dist[u.wrapping_add(0 as size_t) as usize];
        cc = v0.wrapping_sub(w0) >> 31;
        cc = v1.wrapping_sub(w1).wrapping_sub(cc) >> 31;
        cc = v2.wrapping_sub(w2).wrapping_sub(cc) >> 31;
        z += cc as libc::c_int;
        u = u.wrapping_add(3 as size_t);
    }
    return z;
}
unsafe extern "C" fn BerExp(mut p: *mut prng, mut x: fpr, mut ccs: fpr) -> libc::c_int {
    let mut s: libc::c_int = 0;
    let mut i: libc::c_int = 0;
    let mut r: fpr = 0;
    let mut sw: u32 = 0;
    let mut w: u32 = 0;
    let mut z: u64 = 0;
    s = fpr_trunc(PQCLEAN_FALCON512_CLEAN_fpr_mul(x, fpr_inv_log2)) as libc::c_int;
    r = fpr_sub(x, PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_of(s as i64), fpr_log2));
    sw = s as u32;
    sw ^= (sw ^ 63 as u32) & ((63 as u32).wrapping_sub(sw) >> 31).wrapping_neg();
    s = sw as libc::c_int;
    z = (PQCLEAN_FALCON512_CLEAN_fpr_expm_p63(r, ccs) << 1).wrapping_sub(1 as u64) >> s;
    i = 64;
    loop {
        i -= 8;
        w = (prng_get_u8(p)).wrapping_sub((z >> i) as u32 & 0xff as u32);
        if !(w == 0 && i > 0) {
            break;
        }
    }
    return (w >> 31) as libc::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_sampler(
    mut ctx: *mut libc::c_void,
    mut mu: fpr,
    mut isigma: fpr,
) -> libc::c_int {
    let mut spc: *mut sampler_context = 0 as *mut sampler_context;
    let mut s: libc::c_int = 0;
    let mut r: fpr = 0;
    let mut dss: fpr = 0;
    let mut ccs: fpr = 0;
    spc = ctx as *mut sampler_context;
    s = fpr_floor(mu) as libc::c_int;
    r = fpr_sub(mu, fpr_of(s as i64));
    dss = fpr_half(fpr_sqr(isigma));
    ccs = PQCLEAN_FALCON512_CLEAN_fpr_mul(isigma, (*spc).sigma_min);
    loop {
        let mut z0: libc::c_int = 0;
        let mut z: libc::c_int = 0;
        let mut b: libc::c_int = 0;
        let mut x: fpr = 0;
        z0 = PQCLEAN_FALCON512_CLEAN_gaussian0_sampler(&mut (*spc).p);
        b = prng_get_u8(&mut (*spc).p) as libc::c_int & 1;
        z = b + ((b << 1) - 1) * z0;
        x = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_sqr(fpr_sub(fpr_of(z as i64), r)), dss);
        x = fpr_sub(
            x,
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_of((z0 * z0) as i64), fpr_inv_2sqrsigma0),
        );
        if BerExp(&mut (*spc).p, x, ccs) != 0 {
            return s + z;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_sign_tree(
    mut sig: *mut int16_t,
    mut rng: *mut shake256incctx,
    mut expanded_key: *const fpr,
    mut hm: *const uint16_t,
    mut logn: libc::c_uint,
    mut tmp: *mut uint8_t,
) {
    let mut ftmp: *mut fpr = 0 as *mut fpr;
    ftmp = tmp as *mut fpr;
    loop {
        let mut spc: sampler_context = sampler_context {
            p: prng {
                buf: C2RustUnnamed_0 { d: [0; 512] },
                ptr: 0,
                state: C2RustUnnamed { d: [0; 256] },
                type_0: 0,
            },
            sigma_min: 0,
        };
        let mut samp: samplerZ = None;
        let mut samp_ctx: *mut libc::c_void = 0 as *mut libc::c_void;
        spc.sigma_min = fpr_sigma_min[logn as usize];
        PQCLEAN_FALCON512_CLEAN_prng_init(&mut spc.p, rng);
        samp = Some(
            PQCLEAN_FALCON512_CLEAN_sampler
                as unsafe extern "C" fn(*mut libc::c_void, fpr, fpr) -> libc::c_int,
        );
        samp_ctx = &mut spc as *mut sampler_context as *mut libc::c_void;
        if do_sign_tree(samp, samp_ctx, sig, expanded_key, hm, logn, ftmp) != 0 {
            break;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_sign_dyn(
    mut sig: *mut int16_t,
    mut rng: *mut shake256incctx,
    mut f: *const int8_t,
    mut g: *const int8_t,
    mut F: *const int8_t,
    mut G: *const int8_t,
    mut hm: *const uint16_t,
    mut logn: libc::c_uint,
    mut tmp: *mut uint8_t,
) {
    let mut ftmp: *mut fpr = 0 as *mut fpr;
    ftmp = tmp as *mut fpr;
    loop {
        let mut spc: sampler_context = sampler_context {
            p: prng {
                buf: C2RustUnnamed_0 { d: [0; 512] },
                ptr: 0,
                state: C2RustUnnamed { d: [0; 256] },
                type_0: 0,
            },
            sigma_min: 0,
        };
        let mut samp: samplerZ = None;
        let mut samp_ctx: *mut libc::c_void = 0 as *mut libc::c_void;
        spc.sigma_min = fpr_sigma_min[logn as usize];
        PQCLEAN_FALCON512_CLEAN_prng_init(&mut spc.p, rng);
        samp = Some(
            PQCLEAN_FALCON512_CLEAN_sampler
                as unsafe extern "C" fn(*mut libc::c_void, fpr, fpr) -> libc::c_int,
        );
        samp_ctx = &mut spc as *mut sampler_context as *mut libc::c_void;
        if do_sign_dyn(samp, samp_ctx, sig, f, g, F, G, hm, logn, ftmp) != 0 {
            break;
        }
    }
}
