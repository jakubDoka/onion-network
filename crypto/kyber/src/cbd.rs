use {
    crate::{
        params::{ETA1, ETA2, N},
        poly::Poly,
    },
    core::array,
};

pub fn cbd2(buf: &[u8; 2 * N / 4]) -> Poly {
    let mut output = [0i16; N];
    for (t, coffs) in
        buf.array_chunks().copied().map(u32::from_le_bytes).zip(output.array_chunks_mut::<8>())
    {
        let d = (t & 0x5555_5555) + ((t >> 1) & 0x5555_5555);
        *coffs = array::from_fn(|i| {
            let a = (d >> (4 * i)) & 0x3;
            let b = (d >> (4 * i + 2)) & 0x3;
            a as i16 - b as i16
        });
    }
    output
}

pub fn eta1(buf: &[u8; ETA1 * N / 4]) -> Poly {
    cbd2(buf)
}

pub fn eta2(buf: &[u8; ETA2 * N / 4]) -> Poly {
    cbd2(buf)
}
