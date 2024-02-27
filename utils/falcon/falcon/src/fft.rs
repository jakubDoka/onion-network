use crate::libc;
extern "C" {
    fn PQCLEAN_FALCON512_CLEAN_fpr_add(x: fpr, y: fpr) -> fpr;
    fn PQCLEAN_FALCON512_CLEAN_fpr_mul(x: fpr, y: fpr) -> fpr;
    fn PQCLEAN_FALCON512_CLEAN_fpr_div(x: fpr, y: fpr) -> fpr;
    static PQCLEAN_FALCON512_CLEAN_fpr_gm_tab: [fpr; 0];
    static PQCLEAN_FALCON512_CLEAN_fpr_p2_tab: [fpr; 0];
}
pub type __u32 = libc::c_uint;
pub type __u64 = libc::c_ulong;

pub type size_t = libc::c_ulong;
pub type fpr = u64;
static mut fpr_zero: fpr = 0 as fpr;
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
unsafe extern "C" fn fpr_inv(mut x: fpr) -> fpr {
    return PQCLEAN_FALCON512_CLEAN_fpr_div(4607182418800017408, x);
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_FFT(mut f: *mut fpr, mut logn: libc::c_uint) {
    let mut u: libc::c_uint = 0;
    let mut t: size_t = 0;
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut m: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    t = hn;
    u = 1 as libc::c_uint;
    m = 2 as size_t;
    while u < logn {
        let mut ht: size_t = 0;
        let mut hm: size_t = 0;
        let mut i1: size_t = 0;
        let mut j1: size_t = 0;
        ht = t >> 1;
        hm = m >> 1;
        i1 = 0 as size_t;
        j1 = 0 as size_t;
        while i1 < hm {
            let mut j: size_t = 0;
            let mut j2: size_t = 0;
            j2 = j1.wrapping_add(ht);
            let mut s_re: fpr = 0;
            let mut s_im: fpr = 0;
            s_re = *PQCLEAN_FALCON512_CLEAN_fpr_gm_tab
                .as_ptr()
                .offset((m.wrapping_add(i1) << 1).wrapping_add(0 as size_t) as isize);
            s_im = *PQCLEAN_FALCON512_CLEAN_fpr_gm_tab
                .as_ptr()
                .offset((m.wrapping_add(i1) << 1).wrapping_add(1 as size_t) as isize);
            j = j1;
            while j < j2 {
                let mut x_re: fpr = 0;
                let mut x_im: fpr = 0;
                let mut y_re: fpr = 0;
                let mut y_im: fpr = 0;
                x_re = *f.offset(j as isize);
                x_im = *f.offset(j.wrapping_add(hn) as isize);
                y_re = *f.offset(j.wrapping_add(ht) as isize);
                y_im = *f.offset(j.wrapping_add(ht).wrapping_add(hn) as isize);
                let mut fpct_a_re: fpr = 0;
                let mut fpct_a_im: fpr = 0;
                let mut fpct_b_re: fpr = 0;
                let mut fpct_b_im: fpr = 0;
                let mut fpct_d_re: fpr = 0;
                let mut fpct_d_im: fpr = 0;
                fpct_a_re = y_re;
                fpct_a_im = y_im;
                fpct_b_re = s_re;
                fpct_b_im = s_im;
                fpct_d_re = fpr_sub(
                    PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
                    PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
                );
                fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
                    PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
                    PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
                );
                y_re = fpct_d_re;
                y_im = fpct_d_im;
                let mut fpct_re: fpr = 0;
                let mut fpct_im: fpr = 0;
                fpct_re = PQCLEAN_FALCON512_CLEAN_fpr_add(x_re, y_re);
                fpct_im = PQCLEAN_FALCON512_CLEAN_fpr_add(x_im, y_im);
                *f.offset(j as isize) = fpct_re;
                *f.offset(j.wrapping_add(hn) as isize) = fpct_im;
                let mut fpct_re_0: fpr = 0;
                let mut fpct_im_0: fpr = 0;
                fpct_re_0 = fpr_sub(x_re, y_re);
                fpct_im_0 = fpr_sub(x_im, y_im);
                *f.offset(j.wrapping_add(ht) as isize) = fpct_re_0;
                *f.offset(j.wrapping_add(ht).wrapping_add(hn) as isize) = fpct_im_0;
                j = j.wrapping_add(1);
                j;
            }
            i1 = i1.wrapping_add(1);
            i1;
            j1 = j1.wrapping_add(t);
        }
        t = ht;
        u = u.wrapping_add(1);
        u;
        m <<= 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_iFFT(mut f: *mut fpr, mut logn: libc::c_uint) {
    let mut u: size_t = 0;
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut t: size_t = 0;
    let mut m: size_t = 0;
    n = (1 as size_t) << logn;
    t = 1 as size_t;
    m = n;
    hn = n >> 1;
    u = logn as size_t;
    while u > 1 as size_t {
        let mut hm: size_t = 0;
        let mut dt: size_t = 0;
        let mut i1: size_t = 0;
        let mut j1: size_t = 0;
        hm = m >> 1;
        dt = t << 1;
        i1 = 0 as size_t;
        j1 = 0 as size_t;
        while j1 < hn {
            let mut j: size_t = 0;
            let mut j2: size_t = 0;
            j2 = j1.wrapping_add(t);
            let mut s_re: fpr = 0;
            let mut s_im: fpr = 0;
            s_re = *PQCLEAN_FALCON512_CLEAN_fpr_gm_tab
                .as_ptr()
                .offset((hm.wrapping_add(i1) << 1).wrapping_add(0 as size_t) as isize);
            s_im = fpr_neg(
                *PQCLEAN_FALCON512_CLEAN_fpr_gm_tab
                    .as_ptr()
                    .offset((hm.wrapping_add(i1) << 1).wrapping_add(1 as size_t) as isize),
            );
            j = j1;
            while j < j2 {
                let mut x_re: fpr = 0;
                let mut x_im: fpr = 0;
                let mut y_re: fpr = 0;
                let mut y_im: fpr = 0;
                x_re = *f.offset(j as isize);
                x_im = *f.offset(j.wrapping_add(hn) as isize);
                y_re = *f.offset(j.wrapping_add(t) as isize);
                y_im = *f.offset(j.wrapping_add(t).wrapping_add(hn) as isize);
                let mut fpct_re: fpr = 0;
                let mut fpct_im: fpr = 0;
                fpct_re = PQCLEAN_FALCON512_CLEAN_fpr_add(x_re, y_re);
                fpct_im = PQCLEAN_FALCON512_CLEAN_fpr_add(x_im, y_im);
                *f.offset(j as isize) = fpct_re;
                *f.offset(j.wrapping_add(hn) as isize) = fpct_im;
                let mut fpct_re_0: fpr = 0;
                let mut fpct_im_0: fpr = 0;
                fpct_re_0 = fpr_sub(x_re, y_re);
                fpct_im_0 = fpr_sub(x_im, y_im);
                x_re = fpct_re_0;
                x_im = fpct_im_0;
                let mut fpct_a_re: fpr = 0;
                let mut fpct_a_im: fpr = 0;
                let mut fpct_b_re: fpr = 0;
                let mut fpct_b_im: fpr = 0;
                let mut fpct_d_re: fpr = 0;
                let mut fpct_d_im: fpr = 0;
                fpct_a_re = x_re;
                fpct_a_im = x_im;
                fpct_b_re = s_re;
                fpct_b_im = s_im;
                fpct_d_re = fpr_sub(
                    PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
                    PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
                );
                fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
                    PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
                    PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
                );
                *f.offset(j.wrapping_add(t) as isize) = fpct_d_re;
                *f.offset(j.wrapping_add(t).wrapping_add(hn) as isize) = fpct_d_im;
                j = j.wrapping_add(1);
                j;
            }
            i1 = i1.wrapping_add(1);
            i1;
            j1 = j1.wrapping_add(dt);
        }
        t = dt;
        m = hm;
        u = u.wrapping_sub(1);
        u;
    }
    if logn > 0 as libc::c_uint {
        let mut ni: fpr = 0;
        ni = *PQCLEAN_FALCON512_CLEAN_fpr_p2_tab.as_ptr().offset(logn as isize);
        u = 0 as size_t;
        while u < n {
            *f.offset(u as isize) = PQCLEAN_FALCON512_CLEAN_fpr_mul(*f.offset(u as isize), ni);
            u = u.wrapping_add(1);
            u;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_add(
    mut a: *mut fpr,
    mut b: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    u = 0 as size_t;
    while u < n {
        *a.offset(u as isize) =
            PQCLEAN_FALCON512_CLEAN_fpr_add(*a.offset(u as isize), *b.offset(u as isize));
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_sub(
    mut a: *mut fpr,
    mut b: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    u = 0 as size_t;
    while u < n {
        *a.offset(u as isize) = fpr_sub(*a.offset(u as isize), *b.offset(u as isize));
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_neg(mut a: *mut fpr, mut logn: libc::c_uint) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    u = 0 as size_t;
    while u < n {
        *a.offset(u as isize) = fpr_neg(*a.offset(u as isize));
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_adj_fft(
    mut a: *mut fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    u = n >> 1;
    while u < n {
        *a.offset(u as isize) = fpr_neg(*a.offset(u as isize));
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_mul_fft(
    mut a: *mut fpr,
    mut b: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        let mut a_re: fpr = 0;
        let mut a_im: fpr = 0;
        let mut b_re: fpr = 0;
        let mut b_im: fpr = 0;
        a_re = *a.offset(u as isize);
        a_im = *a.offset(u.wrapping_add(hn) as isize);
        b_re = *b.offset(u as isize);
        b_im = *b.offset(u.wrapping_add(hn) as isize);
        let mut fpct_a_re: fpr = 0;
        let mut fpct_a_im: fpr = 0;
        let mut fpct_b_re: fpr = 0;
        let mut fpct_b_im: fpr = 0;
        let mut fpct_d_re: fpr = 0;
        let mut fpct_d_im: fpr = 0;
        fpct_a_re = a_re;
        fpct_a_im = a_im;
        fpct_b_re = b_re;
        fpct_b_im = b_im;
        fpct_d_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
        );
        fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
        );
        *a.offset(u as isize) = fpct_d_re;
        *a.offset(u.wrapping_add(hn) as isize) = fpct_d_im;
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_muladj_fft(
    mut a: *mut fpr,
    mut b: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        let mut a_re: fpr = 0;
        let mut a_im: fpr = 0;
        let mut b_re: fpr = 0;
        let mut b_im: fpr = 0;
        a_re = *a.offset(u as isize);
        a_im = *a.offset(u.wrapping_add(hn) as isize);
        b_re = *b.offset(u as isize);
        b_im = fpr_neg(*b.offset(u.wrapping_add(hn) as isize));
        let mut fpct_a_re: fpr = 0;
        let mut fpct_a_im: fpr = 0;
        let mut fpct_b_re: fpr = 0;
        let mut fpct_b_im: fpr = 0;
        let mut fpct_d_re: fpr = 0;
        let mut fpct_d_im: fpr = 0;
        fpct_a_re = a_re;
        fpct_a_im = a_im;
        fpct_b_re = b_re;
        fpct_b_im = b_im;
        fpct_d_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
        );
        fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
        );
        *a.offset(u as isize) = fpct_d_re;
        *a.offset(u.wrapping_add(hn) as isize) = fpct_d_im;
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_mulselfadj_fft(
    mut a: *mut fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        let mut a_re: fpr = 0;
        let mut a_im: fpr = 0;
        a_re = *a.offset(u as isize);
        a_im = *a.offset(u.wrapping_add(hn) as isize);
        *a.offset(u as isize) = PQCLEAN_FALCON512_CLEAN_fpr_add(fpr_sqr(a_re), fpr_sqr(a_im));
        *a.offset(u.wrapping_add(hn) as isize) = fpr_zero;
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_mulconst(
    mut a: *mut fpr,
    mut x: fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    u = 0 as size_t;
    while u < n {
        *a.offset(u as isize) = PQCLEAN_FALCON512_CLEAN_fpr_mul(*a.offset(u as isize), x);
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_div_fft(
    mut a: *mut fpr,
    mut b: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        let mut a_re: fpr = 0;
        let mut a_im: fpr = 0;
        let mut b_re: fpr = 0;
        let mut b_im: fpr = 0;
        a_re = *a.offset(u as isize);
        a_im = *a.offset(u.wrapping_add(hn) as isize);
        b_re = *b.offset(u as isize);
        b_im = *b.offset(u.wrapping_add(hn) as isize);
        let mut fpct_a_re: fpr = 0;
        let mut fpct_a_im: fpr = 0;
        let mut fpct_b_re: fpr = 0;
        let mut fpct_b_im: fpr = 0;
        let mut fpct_d_re: fpr = 0;
        let mut fpct_d_im: fpr = 0;
        let mut fpct_m: fpr = 0;
        fpct_a_re = a_re;
        fpct_a_im = a_im;
        fpct_b_re = b_re;
        fpct_b_im = b_im;
        fpct_m = PQCLEAN_FALCON512_CLEAN_fpr_add(fpr_sqr(fpct_b_re), fpr_sqr(fpct_b_im));
        fpct_m = fpr_inv(fpct_m);
        fpct_b_re = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_b_re, fpct_m);
        fpct_b_im = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_neg(fpct_b_im), fpct_m);
        fpct_d_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
        );
        fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
        );
        *a.offset(u as isize) = fpct_d_re;
        *a.offset(u.wrapping_add(hn) as isize) = fpct_d_im;
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_invnorm2_fft(
    mut d: *mut fpr,
    mut a: *const fpr,
    mut b: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        let mut a_re: fpr = 0;
        let mut a_im: fpr = 0;
        let mut b_re: fpr = 0;
        let mut b_im: fpr = 0;
        a_re = *a.offset(u as isize);
        a_im = *a.offset(u.wrapping_add(hn) as isize);
        b_re = *b.offset(u as isize);
        b_im = *b.offset(u.wrapping_add(hn) as isize);
        *d.offset(u as isize) = fpr_inv(PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_add(fpr_sqr(a_re), fpr_sqr(a_im)),
            PQCLEAN_FALCON512_CLEAN_fpr_add(fpr_sqr(b_re), fpr_sqr(b_im)),
        ));
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_add_muladj_fft(
    mut d: *mut fpr,
    mut F: *const fpr,
    mut G: *const fpr,
    mut f: *const fpr,
    mut g: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        let mut F_re: fpr = 0;
        let mut F_im: fpr = 0;
        let mut G_re: fpr = 0;
        let mut G_im: fpr = 0;
        let mut f_re: fpr = 0;
        let mut f_im: fpr = 0;
        let mut g_re: fpr = 0;
        let mut g_im: fpr = 0;
        let mut a_re: fpr = 0;
        let mut a_im: fpr = 0;
        let mut b_re: fpr = 0;
        let mut b_im: fpr = 0;
        F_re = *F.offset(u as isize);
        F_im = *F.offset(u.wrapping_add(hn) as isize);
        G_re = *G.offset(u as isize);
        G_im = *G.offset(u.wrapping_add(hn) as isize);
        f_re = *f.offset(u as isize);
        f_im = *f.offset(u.wrapping_add(hn) as isize);
        g_re = *g.offset(u as isize);
        g_im = *g.offset(u.wrapping_add(hn) as isize);
        let mut fpct_a_re: fpr = 0;
        let mut fpct_a_im: fpr = 0;
        let mut fpct_b_re: fpr = 0;
        let mut fpct_b_im: fpr = 0;
        let mut fpct_d_re: fpr = 0;
        let mut fpct_d_im: fpr = 0;
        fpct_a_re = F_re;
        fpct_a_im = F_im;
        fpct_b_re = f_re;
        fpct_b_im = fpr_neg(f_im);
        fpct_d_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
        );
        fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
        );
        a_re = fpct_d_re;
        a_im = fpct_d_im;
        let mut fpct_a_re_0: fpr = 0;
        let mut fpct_a_im_0: fpr = 0;
        let mut fpct_b_re_0: fpr = 0;
        let mut fpct_b_im_0: fpr = 0;
        let mut fpct_d_re_0: fpr = 0;
        let mut fpct_d_im_0: fpr = 0;
        fpct_a_re_0 = G_re;
        fpct_a_im_0 = G_im;
        fpct_b_re_0 = g_re;
        fpct_b_im_0 = fpr_neg(g_im);
        fpct_d_re_0 = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re_0, fpct_b_re_0),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im_0, fpct_b_im_0),
        );
        fpct_d_im_0 = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re_0, fpct_b_im_0),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im_0, fpct_b_re_0),
        );
        b_re = fpct_d_re_0;
        b_im = fpct_d_im_0;
        *d.offset(u as isize) = PQCLEAN_FALCON512_CLEAN_fpr_add(a_re, b_re);
        *d.offset(u.wrapping_add(hn) as isize) = PQCLEAN_FALCON512_CLEAN_fpr_add(a_im, b_im);
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_mul_autoadj_fft(
    mut a: *mut fpr,
    mut b: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        *a.offset(u as isize) =
            PQCLEAN_FALCON512_CLEAN_fpr_mul(*a.offset(u as isize), *b.offset(u as isize));
        *a.offset(u.wrapping_add(hn) as isize) = PQCLEAN_FALCON512_CLEAN_fpr_mul(
            *a.offset(u.wrapping_add(hn) as isize),
            *b.offset(u as isize),
        );
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_div_autoadj_fft(
    mut a: *mut fpr,
    mut b: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        let mut ib: fpr = 0;
        ib = fpr_inv(*b.offset(u as isize));
        *a.offset(u as isize) = PQCLEAN_FALCON512_CLEAN_fpr_mul(*a.offset(u as isize), ib);
        *a.offset(u.wrapping_add(hn) as isize) =
            PQCLEAN_FALCON512_CLEAN_fpr_mul(*a.offset(u.wrapping_add(hn) as isize), ib);
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_LDL_fft(
    mut g00: *const fpr,
    mut g01: *mut fpr,
    mut g11: *mut fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        let mut g00_re: fpr = 0;
        let mut g00_im: fpr = 0;
        let mut g01_re: fpr = 0;
        let mut g01_im: fpr = 0;
        let mut g11_re: fpr = 0;
        let mut g11_im: fpr = 0;
        let mut mu_re: fpr = 0;
        let mut mu_im: fpr = 0;
        g00_re = *g00.offset(u as isize);
        g00_im = *g00.offset(u.wrapping_add(hn) as isize);
        g01_re = *g01.offset(u as isize);
        g01_im = *g01.offset(u.wrapping_add(hn) as isize);
        g11_re = *g11.offset(u as isize);
        g11_im = *g11.offset(u.wrapping_add(hn) as isize);
        let mut fpct_a_re: fpr = 0;
        let mut fpct_a_im: fpr = 0;
        let mut fpct_b_re: fpr = 0;
        let mut fpct_b_im: fpr = 0;
        let mut fpct_d_re: fpr = 0;
        let mut fpct_d_im: fpr = 0;
        let mut fpct_m: fpr = 0;
        fpct_a_re = g01_re;
        fpct_a_im = g01_im;
        fpct_b_re = g00_re;
        fpct_b_im = g00_im;
        fpct_m = PQCLEAN_FALCON512_CLEAN_fpr_add(fpr_sqr(fpct_b_re), fpr_sqr(fpct_b_im));
        fpct_m = fpr_inv(fpct_m);
        fpct_b_re = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_b_re, fpct_m);
        fpct_b_im = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_neg(fpct_b_im), fpct_m);
        fpct_d_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
        );
        fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
        );
        mu_re = fpct_d_re;
        mu_im = fpct_d_im;
        let mut fpct_a_re_0: fpr = 0;
        let mut fpct_a_im_0: fpr = 0;
        let mut fpct_b_re_0: fpr = 0;
        let mut fpct_b_im_0: fpr = 0;
        let mut fpct_d_re_0: fpr = 0;
        let mut fpct_d_im_0: fpr = 0;
        fpct_a_re_0 = mu_re;
        fpct_a_im_0 = mu_im;
        fpct_b_re_0 = g01_re;
        fpct_b_im_0 = fpr_neg(g01_im);
        fpct_d_re_0 = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re_0, fpct_b_re_0),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im_0, fpct_b_im_0),
        );
        fpct_d_im_0 = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re_0, fpct_b_im_0),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im_0, fpct_b_re_0),
        );
        g01_re = fpct_d_re_0;
        g01_im = fpct_d_im_0;
        let mut fpct_re: fpr = 0;
        let mut fpct_im: fpr = 0;
        fpct_re = fpr_sub(g11_re, g01_re);
        fpct_im = fpr_sub(g11_im, g01_im);
        *g11.offset(u as isize) = fpct_re;
        *g11.offset(u.wrapping_add(hn) as isize) = fpct_im;
        *g01.offset(u as isize) = mu_re;
        *g01.offset(u.wrapping_add(hn) as isize) = fpr_neg(mu_im);
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_LDLmv_fft(
    mut d11: *mut fpr,
    mut l10: *mut fpr,
    mut g00: *const fpr,
    mut g01: *const fpr,
    mut g11: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    u = 0 as size_t;
    while u < hn {
        let mut g00_re: fpr = 0;
        let mut g00_im: fpr = 0;
        let mut g01_re: fpr = 0;
        let mut g01_im: fpr = 0;
        let mut g11_re: fpr = 0;
        let mut g11_im: fpr = 0;
        let mut mu_re: fpr = 0;
        let mut mu_im: fpr = 0;
        g00_re = *g00.offset(u as isize);
        g00_im = *g00.offset(u.wrapping_add(hn) as isize);
        g01_re = *g01.offset(u as isize);
        g01_im = *g01.offset(u.wrapping_add(hn) as isize);
        g11_re = *g11.offset(u as isize);
        g11_im = *g11.offset(u.wrapping_add(hn) as isize);
        let mut fpct_a_re: fpr = 0;
        let mut fpct_a_im: fpr = 0;
        let mut fpct_b_re: fpr = 0;
        let mut fpct_b_im: fpr = 0;
        let mut fpct_d_re: fpr = 0;
        let mut fpct_d_im: fpr = 0;
        let mut fpct_m: fpr = 0;
        fpct_a_re = g01_re;
        fpct_a_im = g01_im;
        fpct_b_re = g00_re;
        fpct_b_im = g00_im;
        fpct_m = PQCLEAN_FALCON512_CLEAN_fpr_add(fpr_sqr(fpct_b_re), fpr_sqr(fpct_b_im));
        fpct_m = fpr_inv(fpct_m);
        fpct_b_re = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_b_re, fpct_m);
        fpct_b_im = PQCLEAN_FALCON512_CLEAN_fpr_mul(fpr_neg(fpct_b_im), fpct_m);
        fpct_d_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
        );
        fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
        );
        mu_re = fpct_d_re;
        mu_im = fpct_d_im;
        let mut fpct_a_re_0: fpr = 0;
        let mut fpct_a_im_0: fpr = 0;
        let mut fpct_b_re_0: fpr = 0;
        let mut fpct_b_im_0: fpr = 0;
        let mut fpct_d_re_0: fpr = 0;
        let mut fpct_d_im_0: fpr = 0;
        fpct_a_re_0 = mu_re;
        fpct_a_im_0 = mu_im;
        fpct_b_re_0 = g01_re;
        fpct_b_im_0 = fpr_neg(g01_im);
        fpct_d_re_0 = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re_0, fpct_b_re_0),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im_0, fpct_b_im_0),
        );
        fpct_d_im_0 = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re_0, fpct_b_im_0),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im_0, fpct_b_re_0),
        );
        g01_re = fpct_d_re_0;
        g01_im = fpct_d_im_0;
        let mut fpct_re: fpr = 0;
        let mut fpct_im: fpr = 0;
        fpct_re = fpr_sub(g11_re, g01_re);
        fpct_im = fpr_sub(g11_im, g01_im);
        *d11.offset(u as isize) = fpct_re;
        *d11.offset(u.wrapping_add(hn) as isize) = fpct_im;
        *l10.offset(u as isize) = mu_re;
        *l10.offset(u.wrapping_add(hn) as isize) = fpr_neg(mu_im);
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_split_fft(
    mut f0: *mut fpr,
    mut f1: *mut fpr,
    mut f: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut qn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    qn = hn >> 1;
    *f0.offset(0 as isize) = *f.offset(0 as isize);
    *f1.offset(0 as isize) = *f.offset(hn as isize);
    u = 0 as size_t;
    while u < qn {
        let mut a_re: fpr = 0;
        let mut a_im: fpr = 0;
        let mut b_re: fpr = 0;
        let mut b_im: fpr = 0;
        let mut t_re: fpr = 0;
        let mut t_im: fpr = 0;
        a_re = *f.offset((u << 1).wrapping_add(0 as size_t) as isize);
        a_im = *f.offset((u << 1).wrapping_add(0 as size_t).wrapping_add(hn) as isize);
        b_re = *f.offset((u << 1).wrapping_add(1 as size_t) as isize);
        b_im = *f.offset((u << 1).wrapping_add(1 as size_t).wrapping_add(hn) as isize);
        let mut fpct_re: fpr = 0;
        let mut fpct_im: fpr = 0;
        fpct_re = PQCLEAN_FALCON512_CLEAN_fpr_add(a_re, b_re);
        fpct_im = PQCLEAN_FALCON512_CLEAN_fpr_add(a_im, b_im);
        t_re = fpct_re;
        t_im = fpct_im;
        *f0.offset(u as isize) = fpr_half(t_re);
        *f0.offset(u.wrapping_add(qn) as isize) = fpr_half(t_im);
        let mut fpct_re_0: fpr = 0;
        let mut fpct_im_0: fpr = 0;
        fpct_re_0 = fpr_sub(a_re, b_re);
        fpct_im_0 = fpr_sub(a_im, b_im);
        t_re = fpct_re_0;
        t_im = fpct_im_0;
        let mut fpct_a_re: fpr = 0;
        let mut fpct_a_im: fpr = 0;
        let mut fpct_b_re: fpr = 0;
        let mut fpct_b_im: fpr = 0;
        let mut fpct_d_re: fpr = 0;
        let mut fpct_d_im: fpr = 0;
        fpct_a_re = t_re;
        fpct_a_im = t_im;
        fpct_b_re = *PQCLEAN_FALCON512_CLEAN_fpr_gm_tab
            .as_ptr()
            .offset((u.wrapping_add(hn) << 1).wrapping_add(0 as size_t) as isize);
        fpct_b_im = fpr_neg(
            *PQCLEAN_FALCON512_CLEAN_fpr_gm_tab
                .as_ptr()
                .offset((u.wrapping_add(hn) << 1).wrapping_add(1 as size_t) as isize),
        );
        fpct_d_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
        );
        fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
        );
        t_re = fpct_d_re;
        t_im = fpct_d_im;
        *f1.offset(u as isize) = fpr_half(t_re);
        *f1.offset(u.wrapping_add(qn) as isize) = fpr_half(t_im);
        u = u.wrapping_add(1);
        u;
    }
}
#[no_mangle]
pub unsafe extern "C" fn PQCLEAN_FALCON512_CLEAN_poly_merge_fft(
    mut f: *mut fpr,
    mut f0: *const fpr,
    mut f1: *const fpr,
    mut logn: libc::c_uint,
) {
    let mut n: size_t = 0;
    let mut hn: size_t = 0;
    let mut qn: size_t = 0;
    let mut u: size_t = 0;
    n = (1 as size_t) << logn;
    hn = n >> 1;
    qn = hn >> 1;
    *f.offset(0 as isize) = *f0.offset(0 as isize);
    *f.offset(hn as isize) = *f1.offset(0 as isize);
    u = 0 as size_t;
    while u < qn {
        let mut a_re: fpr = 0;
        let mut a_im: fpr = 0;
        let mut b_re: fpr = 0;
        let mut b_im: fpr = 0;
        let mut t_re: fpr = 0;
        let mut t_im: fpr = 0;
        a_re = *f0.offset(u as isize);
        a_im = *f0.offset(u.wrapping_add(qn) as isize);
        let mut fpct_a_re: fpr = 0;
        let mut fpct_a_im: fpr = 0;
        let mut fpct_b_re: fpr = 0;
        let mut fpct_b_im: fpr = 0;
        let mut fpct_d_re: fpr = 0;
        let mut fpct_d_im: fpr = 0;
        fpct_a_re = *f1.offset(u as isize);
        fpct_a_im = *f1.offset(u.wrapping_add(qn) as isize);
        fpct_b_re = *PQCLEAN_FALCON512_CLEAN_fpr_gm_tab
            .as_ptr()
            .offset((u.wrapping_add(hn) << 1).wrapping_add(0 as size_t) as isize);
        fpct_b_im = *PQCLEAN_FALCON512_CLEAN_fpr_gm_tab
            .as_ptr()
            .offset((u.wrapping_add(hn) << 1).wrapping_add(1 as size_t) as isize);
        fpct_d_re = fpr_sub(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_re),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_im),
        );
        fpct_d_im = PQCLEAN_FALCON512_CLEAN_fpr_add(
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_re, fpct_b_im),
            PQCLEAN_FALCON512_CLEAN_fpr_mul(fpct_a_im, fpct_b_re),
        );
        b_re = fpct_d_re;
        b_im = fpct_d_im;
        let mut fpct_re: fpr = 0;
        let mut fpct_im: fpr = 0;
        fpct_re = PQCLEAN_FALCON512_CLEAN_fpr_add(a_re, b_re);
        fpct_im = PQCLEAN_FALCON512_CLEAN_fpr_add(a_im, b_im);
        t_re = fpct_re;
        t_im = fpct_im;
        *f.offset((u << 1).wrapping_add(0 as size_t) as isize) = t_re;
        *f.offset((u << 1).wrapping_add(0 as size_t).wrapping_add(hn) as isize) = t_im;
        let mut fpct_re_0: fpr = 0;
        let mut fpct_im_0: fpr = 0;
        fpct_re_0 = fpr_sub(a_re, b_re);
        fpct_im_0 = fpr_sub(a_im, b_im);
        t_re = fpct_re_0;
        t_im = fpct_im_0;
        *f.offset((u << 1).wrapping_add(1 as size_t) as isize) = t_re;
        *f.offset((u << 1).wrapping_add(1 as size_t).wrapping_add(hn) as isize) = t_im;
        u = u.wrapping_add(1);
        u;
    }
}
