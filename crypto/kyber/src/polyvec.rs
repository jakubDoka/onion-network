use {
    crate::{
        params::{K, N, POLYBYTES, POLYVECBYTES, POLYVECCOMPRESSEDBYTES, Q},
        poly::Poly,
    },
    core::array,
};

pub type Polyvec = [Poly; K];

pub fn compress(input: &Polyvec) -> [u8; POLYVECCOMPRESSEDBYTES] {
    let mut output = [0u8; POLYVECCOMPRESSEDBYTES];
    let input_ier = input.iter().flat_map(|p| p.array_chunks::<4>());
    for (out_chunk, in_chunk) in output.array_chunks_mut::<5>().zip(input_ier) {
        let [t0, t1, t2, t3] = in_chunk
            .map(|u| u + ((u >> 0xf) & Q as i16))
            .map(|u| ((((u as u32) << 10) + Q as u32 / 2) / Q as u32) & 0x3ff);

        *out_chunk =
            [t0, (t0 >> 8) | (t1 << 2), (t1 >> 6) | (t2 << 4), (t2 >> 4) | (t3 << 6), t3 >> 2]
                .map(|u| u as u8);
    }
    output
}

pub fn decompress(input: &[u8; POLYVECCOMPRESSEDBYTES]) -> Polyvec {
    let mut output = [[0i16; N]; K];
    let output_ier = output.iter_mut().flat_map(|p| p.array_chunks_mut::<4>());
    for (out_chunk, in_chunk) in output_ier.zip(input.array_chunks::<5>()) {
        *out_chunk = array::from_fn(|i| {
            (u16::from(in_chunk[i]) >> (2 * i)) | (u16::from(in_chunk[i + 1]) << (8 - 2 * i))
        })
        .map(|u| (u32::from(u & 0x3ff) * Q as u32 + 512) >> 10)
        .map(|u| u as i16);
    }
    output
}

pub fn to_bytes(input: &Polyvec) -> [u8; POLYVECBYTES] {
    let mut output = [0u8; POLYVECBYTES];
    for (out, input) in output.array_chunks_mut::<POLYBYTES>().zip(input) {
        *out = crate::poly::to_bytes(input);
    }
    output
}

pub fn from_bytes(input: &[u8; POLYVECBYTES]) -> Polyvec {
    let mut output = [[0i16; N]; K];
    for (out, input) in output.iter_mut().zip(input.array_chunks::<POLYBYTES>()) {
        *out = crate::poly::from_bytes(input);
    }
    output
}

pub fn ntt(inout: &mut Polyvec) {
    for r in inout.iter_mut() {
        crate::poly::ntt(r);
    }
}

pub fn invntt_tomont(inout: &mut Polyvec) {
    for r in inout.iter_mut() {
        crate::poly::invntt_tomont(r);
    }
}

pub fn basemul_acc_montgomery(a: &Polyvec, b: &Polyvec) -> Poly {
    //unsigned int i;
    //poly t;

    //PQCLEAN_KYBER768_CLEAN_poly_basemul_montgomery(r, &a->vec[0], &b->vec[0]);
    //for (i = 1; i < KYBER_K; i++) {
    //    PQCLEAN_KYBER768_CLEAN_poly_basemul_montgomery(&t, &a->vec[i], &b->vec[i]);
    //    PQCLEAN_KYBER768_CLEAN_poly_add(r, r, &t);
    //}

    //PQCLEAN_KYBER768_CLEAN_poly_reduce(r);

    let mut res = a
        .iter()
        .zip(b)
        .map(|(a, b)| crate::poly::basemul_montgomery(a, b))
        .reduce(crate::poly::add)
        .expect("ist an array mate");
    crate::poly::reduce(&mut res);
    res
}

pub fn reduce(inout: &mut Polyvec) {
    for r in inout.iter_mut() {
        crate::poly::reduce(r);
    }
}

pub fn add(a: Polyvec, b: Polyvec) -> Polyvec {
    array::from_fn(|i| crate::poly::add(a[i], b[i]))
}
