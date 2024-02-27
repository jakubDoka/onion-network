include!(concat!(env!("OUT_DIR"), "/table.rs"));

#[inline]
pub fn add(a: u8, b: u8) -> u8 {
    a ^ b
}

#[inline]
pub fn mul(a: u8, b: u8) -> u8 {
    MUL_TABLE[a as usize][b as usize]
}

#[inline]
pub fn div(a: u8, b: u8) -> Option<u8> {
    if b == 0 {
        return None;
    }

    if a == 0 {
        return Some(0);
    }

    if a == 1 {
        return Some(INV_TABLE[b as usize]);
    }

    let log_a = LOG_TABLE[a as usize];
    let log_b = LOG_TABLE[b as usize];
    let mut log_result = log_a as isize - log_b as isize;
    if log_result < 0 {
        log_result += 255;
    }
    Some(EXP_TABLE[log_result as usize])
}

#[inline]
pub fn exp(a: u8, n: usize) -> u8 {
    if n == 0 {
        1
    } else if a == 0 {
        0
    } else {
        let log_a = LOG_TABLE[a as usize];
        let mut log_result = log_a as usize * n;
        while 255 <= log_result {
            log_result -= 255;
        }
        EXP_TABLE[log_result]
    }
}

#[allow(unused)]
const PURE_RUST_UNROLL: usize = 4;

#[cfg(not(any(
    not(feature = "simd"),
    target_feature = "avx2",
    target_feature = "ssse3",
    target_feature = "simd128"
)))]
compile_error!(
    "`simd` feature is useless without `avx2`, `ssse3` or `simd128` target feature enabled"
);

#[cfg(target_feature = "avx2")]
const LANES: usize = 32;
#[cfg(any(
    all(target_feature = "ssse3", not(target_feature = "avx2")),
    target_feature = "simd128"
))]
const LANES: usize = 16;

#[cfg(any(target_feature = "avx2", target_feature = "ssse3", target_feature = "simd128"))]
pub fn mul_slice_xor(c: u8, input: &[u8], out: &mut [u8]) {
    use core::simd::Simd;

    assert_eq!(input.len(), out.len());

    let low = [MUL_TABLE_LOW[c as usize]; LANES / 16];
    let high = [MUL_TABLE_HIGH[c as usize]; LANES / 16];
    let low = Simd::<_, LANES>::from_slice(low.flatten());
    let high = Simd::<_, LANES>::from_slice(high.flatten());
    let mask = Simd::<_, LANES>::splat(0xf);

    let mut input = input.array_chunks::<LANES>();
    let mut out = out.array_chunks_mut::<LANES>();

    for (i, o) in input.by_ref().zip(out.by_ref()) {
        let is = Simd::from_array(*i);
        let os = Simd::from_array(*o);
        let res = os ^ low.swizzle_dyn(is & mask) ^ high.swizzle_dyn((is >> 4) & mask);
        *o = Simd::to_array(res);
    }

    mul_slice_xor_low(c, input.remainder(), out.into_remainder());
}

#[cfg(any(target_feature = "avx2", target_feature = "ssse3", target_feature = "simd128"))]
pub fn mul_slice_in_place(c: u8, out: &mut [u8]) {
    use core::simd::Simd;

    let low = [MUL_TABLE_LOW[c as usize]; LANES / 16];
    let high = [MUL_TABLE_HIGH[c as usize]; LANES / 16];
    let low = Simd::<_, LANES>::from_slice(low.flatten());
    let high = Simd::<_, LANES>::from_slice(high.flatten());
    let mask = Simd::<_, LANES>::splat(0xf);

    let mut out = out.array_chunks_mut::<LANES>();

    for o in out.by_ref() {
        let os = Simd::from_array(*o);
        let res = os ^ low.swizzle_dyn(os & mask) ^ high.swizzle_dyn((os >> 4) & mask);
        *o = Simd::to_array(res);
    }

    mul_slice_in_place_low(c, out.into_remainder());
}

#[cfg(not(any(target_feature = "avx2", target_feature = "ssse3", target_feature = "simd128")))]
pub fn mul_slice_xor(c: u8, input: &[u8], out: &mut [u8]) {
    mul_slice_xor_low(c, input, out);
}

fn mul_slice_xor_low(c: u8, input: &[u8], out: &mut [u8]) {
    assert_eq!(input.len(), out.len());

    let mt = &MUL_TABLE[c as usize];
    for (i, o) in input.iter().zip(out) {
        *o ^= mt[*i as usize];
    }
}

#[cfg(not(any(target_feature = "avx2", target_feature = "ssse3", target_feature = "simd128")))]
pub fn mul_slice_in_place(c: u8, out: &mut [u8]) {
    mul_slice_in_place_low(c, out);
}

fn mul_slice_in_place_low(c: u8, out: &mut [u8]) {
    let mt = &MUL_TABLE[c as usize];
    for o in out {
        *o = mt[*o as usize];
    }
}
