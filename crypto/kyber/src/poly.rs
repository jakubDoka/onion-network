use {
    crate::params::{ETA1, ETA2, INDCPA_MSGBYTES, N, POLYBYTES, POLYCOMPRESSEDBYTES, Q, SYMBYTES},
    core::array,
};

pub type Poly = [i16; N];

pub fn compress(input: &Poly) -> [u8; POLYCOMPRESSEDBYTES] {
    let mut output = [0u8; POLYCOMPRESSEDBYTES];
    for (out_chunk, in_chunk) in output.array_chunks_mut::<4>().zip(input.array_chunks::<8>()) {
        in_chunk
            .map(|mut u| {
                u += (u >> 0xf) & Q as i16;
                (((((u as u16) << 4) + Q as u16 / 2) / Q as u16) & 0xf) as u8
            })
            .array_chunks::<2>()
            .map(|[a, b]| a | (b << 4))
            .zip(out_chunk)
            .for_each(|(u, o)| *o = u);
    }
    output
}

pub fn decompress(input: &[u8; POLYCOMPRESSEDBYTES]) -> Poly {
    let mut output = [0i16; N];
    for (out, input) in output.array_chunks_mut().zip(input) {
        *out = [input & 0xf, input >> 4]
            .map(u16::from)
            .map(|u| (u * Q as u16 + 8) >> 4)
            .map(|u| u as i16);
    }
    output
}

pub fn to_bytes(input: &Poly) -> [u8; POLYBYTES] {
    let mut output = [0u8; POLYBYTES];
    for (out_chunk, in_chunk) in output.array_chunks_mut::<3>().zip(input.array_chunks::<2>()) {
        let [t0, t1] = in_chunk.map(|u| u + ((u >> 0xf) & Q as i16));
        *out_chunk = [t0, (t0 >> 8) | (t1 << 4), t1 >> 4].map(|u| u as u8);
    }
    output
}

pub fn from_bytes(input: &[u8; POLYBYTES]) -> Poly {
    let mut output = [0i16; N];
    for (out, input) in output.array_chunks_mut::<2>().zip(input.array_chunks::<3>()) {
        let [t0, t1, t2] = input.map(u16::from);
        *out = [t0 | ((t1 & 0xf) << 8), (t1 >> 4) | (t2 << 4)].map(|u| (u & 0xfff) as i16);
    }
    output
}

pub fn from_msg(input: &[u8; INDCPA_MSGBYTES]) -> Poly {
    let mut output = [0i16; N];
    for (out, &input) in output.array_chunks_mut::<8>().zip(input) {
        *out = array::from_fn(|i| -i16::from((input >> i) & 1) & ((Q + 1) / 2) as i16);
    }
    output
}

pub fn to_msg(input: &Poly) -> [u8; INDCPA_MSGBYTES] {
    let mut output = [0u8; INDCPA_MSGBYTES];
    for (out, input) in input.array_chunks::<8>().zip(&mut output) {
        *input = out.iter().enumerate().fold(0, |acc, (i, &(mut t))| {
            t += (t >> 15) & Q as i16;
            t = (((t << 1) + Q as i16 / 2) / Q as i16) & 1;
            acc | ((t as u8) << i)
        });
    }
    output
}

pub fn getnoise_eta1(seed: &[u8; SYMBYTES], nonce: u8) -> Poly {
    let mut buf = [0u8; ETA1 * N / 4];
    crate::symetric::prf(&mut buf, seed, nonce);
    crate::cbd::eta1(&buf)
}

pub fn getnoise_eta2(seed: &[u8; SYMBYTES], nonce: u8) -> Poly {
    let mut buf = [0u8; ETA2 * N / 4];
    crate::symetric::prf(&mut buf, seed, nonce);
    crate::cbd::eta2(&buf)
}

pub fn ntt(inout: &mut Poly) {
    crate::ntt::ntt(inout);
    reduce(inout);
}

pub fn invntt_tomont(inout: &mut Poly) {
    crate::ntt::invntt(inout);
}

pub fn basemul_montgomery(a: &Poly, b: &Poly) -> Poly {
    let mut output = [0i16; N];
    for ([((ra, aa), ba), ((rb, ab), bb)], &zeta) in output
        .array_chunks_mut::<2>()
        .zip(a.array_chunks::<2>())
        .zip(b.array_chunks::<2>())
        .array_chunks()
        .zip(&crate::ntt::ZETAS[64..])
    {
        crate::ntt::basemul(ra, aa, ba, zeta);
        crate::ntt::basemul(rb, ab, bb, -zeta);
    }
    output
}

pub fn tomont(inout: &mut Poly) {
    let f = (1u64 << 32) % Q as u64;
    for r in inout.iter_mut() {
        *r = crate::mondgomery::reduce(i32::from(*r) * f as i32);
    }
}

pub fn reduce(inout: &mut Poly) {
    for r in inout.iter_mut() {
        *r = crate::barrett::reduce(*r);
    }
}

pub fn add(a: Poly, b: Poly) -> Poly {
    array::from_fn(|i| a[i] + b[i])
}

pub fn sub(a: Poly, b: Poly) -> Poly {
    array::from_fn(|i| a[i] - b[i])
}
