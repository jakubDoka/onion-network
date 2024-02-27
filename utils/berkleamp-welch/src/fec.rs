use {
    crate::{galois, math},
    std::{iter, mem, usize},
};

pub struct Fec {
    required: usize,
    total: usize,
    enc_matrix: Vec<u8>,
    vand_matrix: Vec<u8>,
}

impl Fec {
    /// # Panics
    /// if '0 < required < total <= 256' is not true
    #[must_use]
    pub fn new(required: usize, total: usize) -> Self {
        assert!(required > 0);
        assert!(total > required);
        assert!(required <= 256);
        assert!(total <= 256);

        let mut enc_matrix = vec![0; required * total];
        let mut inverted_vdm = vec![0; required * total];
        math::create_inverted_vdm(&mut inverted_vdm, required);

        for (i, e) in inverted_vdm.iter_mut().enumerate().skip(required * required) {
            *e = galois::EXP_TABLE[((i / required) * (i % required)) % 255];
        }

        for i in 0..required {
            enc_matrix[i * (required + 1)] = 1;
        }

        for row in (required * required..required * total).step_by(required) {
            for col in 0..required {
                let mut acc = 0;
                for i in 0..required {
                    acc ^= galois::mul(inverted_vdm[row + i], inverted_vdm[i * required + col]);
                }
                enc_matrix[row + col] = acc;
            }
        }

        let mut vand_matrix = vec![0; required * total];
        vand_matrix[0] = 1;
        let mut g = 1;
        for row in vand_matrix.chunks_exact_mut(total) {
            let mut a = 1;
            for col in row.iter_mut().skip(1) {
                *col = a;
                a = galois::mul(g, a);
            }
            g = galois::mul(2, g);
        }

        Self { required, total, enc_matrix, vand_matrix }
    }

    #[must_use]
    pub const fn required(&self) -> usize {
        self.required
    }

    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }

    pub fn encode(&self, data: &[u8], parity: &mut [u8]) -> Result<(), EncodeError> {
        let block_size = data.len() / self.required;

        if block_size * self.required != data.len() {
            return Err(EncodeError::InvalidDataLength);
        }

        if block_size * (self.total - self.required) != parity.len() {
            return Err(EncodeError::InvalidParityLength);
        }

        parity.fill(0);

        for (i, par) in parity.chunks_exact_mut(block_size).enumerate() {
            for (j, dat) in data.chunks_exact(block_size).enumerate() {
                galois::mul_slice_xor(
                    self.enc_matrix[(i + self.required) * self.required + j],
                    dat,
                    par,
                );
            }
        }

        Ok(())
    }

    pub fn rebuild<'a, 'b>(
        &self,
        mut shares: &'a mut [Share<'b>],
        temp_buffer: &mut Vec<u8>,
    ) -> Result<&'a mut [Share<'b>], RebuildError> {
        shares.sort_unstable_by_key(|s| s.number);
        shares = shares.take_mut(..self.required).ok_or(RebuildError::NotEnoughShares)?;

        let block_size = shares[0].data.len();
        if shares.iter().any(|s| s.data.len() != block_size) {
            return Err(RebuildError::NotEqualLengths);
        }

        temp_buffer.clear();
        temp_buffer.resize(self.required * self.required + block_size, 0);
        let (m_dec, buf) = temp_buffer.split_at_mut(self.required * self.required);

        for i in 0..self.required {
            m_dec[i * (self.required + 1)] = 1;
        }

        let data_count = shares.iter().take_while(|s| s.number < self.required).count();
        let (data, parity) = shares.split_at_mut(data_count);
        let first_id = data.first().map_or(0, |s| s.number);
        let last_id = data.last().map_or(0, |s| s.number + 1);

        for (i, p) in data
            .array_windows()
            .flat_map(|[a, b]| a.number + 1..b.number)
            .chain(last_id..self.required)
            .chain(0..first_id)
            .zip(parity)
        {
            let src = p.number * self.required..p.number * self.required + self.required;
            let dst = i * self.required..i * self.required + self.required;
            m_dec[dst].copy_from_slice(&self.enc_matrix[src]);
            p.number = usize::MAX - i;
        }

        shares.sort_unstable_by_key(|s| {
            if s.number > self.required {
                usize::MAX - s.number
            } else {
                s.number
            }
        });

        math::invert_matrix(m_dec, self.required).ok_or(RebuildError::InvertMatrix)?;

        for (i, row) in m_dec.chunks_exact(self.required).enumerate() {
            if shares[i].number < self.required {
                continue;
            }

            buf.fill(0);
            for (&cof, share) in row.iter().zip(&*shares) {
                galois::mul_slice_xor(cof, share.data, buf);
            }

            shares[i].data.copy_from_slice(buf);
        }

        Ok(shares)
    }

    pub fn decode<'a, 'b>(
        &self,
        mut shares: &'a mut [Share<'b>],
        report_invalid: bool,
        temp_buffer: &mut Vec<u8>,
    ) -> Result<&'a mut [Share<'b>], DecodeError> {
        let allocs = Resources::new(temp_buffer, self.required, self.total, shares)
            .map_err(DecodeError::InitResources)?;
        shares = self.correct(shares, report_invalid, allocs).map_err(DecodeError::Correct)?;
        self.rebuild(shares, temp_buffer).map_err(DecodeError::Rebuild)
    }

    pub fn correct<'a, 'b>(
        &self,
        shares: &'a mut [Share<'b>],
        report_invalid: bool,
        mut allocs: Resources,
    ) -> Result<&'a mut [Share<'b>], CorrectError> {
        let (r, c) = self.syndrome_matrix(&mut *shares, &mut allocs);
        let synd = mem::take(&mut allocs.parity);

        let buf = mem::take(&mut allocs.buf);
        let correction = mem::take(&mut allocs.corrections);
        for i in 0..r {
            buf.fill(0);

            for j in 0..c {
                galois::mul_slice_xor(synd[i * c + j], shares[j].data, buf);
            }

            for (j, _) in buf.iter().enumerate().filter(|(_, &b)| b != 0) {
                self.berklekamp_welch(&mut *shares, j, correction, &mut allocs)
                    .map_err(CorrectError::BerklekampWelch)?;

                for share in shares.iter_mut() {
                    if share.number < self.total {
                        // considered invalid
                        share.number = usize::MAX - share.number;
                    }
                    share.data[j] = correction[usize::MAX - share.number];
                }
            }
        }

        shares.sort_unstable_by_key(|s| s.number);

        let valid_count = shares.iter().take_while(|s| s.number < self.total).count();

        for share in &mut shares[valid_count..] {
            share.number = usize::MAX - share.number;
        }

        if report_invalid && valid_count != shares.len() {
            return Err(CorrectError::InvalidShares(
                shares[valid_count..].iter().map(|s| s.number).collect(),
            ));
        }

        Ok(shares)
    }

    pub fn berklekamp_welch(
        &self,
        shares: &mut [Share],
        j: usize,
        correction: &mut [u8],
        allocs: &mut Resources,
    ) -> Result<(), BerklekampWelchError> {
        let e = allocs.e;
        let q = allocs.q;

        let interp_base = 2;

        let eval_point = |num| {
            if num == 0 {
                0
            } else {
                galois::exp(interp_base, num - 1)
            }
        };

        let dim = q + e;

        let s = &mut *allocs.s;
        let f = &mut *allocs.f;
        for ((f, share), row) in f.iter_mut().zip(shares).zip(s.chunks_exact_mut(dim)) {
            let x_i = eval_point(share.number);
            let r_i = share.data[j];

            *f = galois::mul(galois::exp(x_i, e), r_i);

            for (i, x) in row[..q].iter_mut().enumerate() {
                *x = galois::exp(x_i, i);
            }
            for (k, x) in row[q..].iter_mut().enumerate() {
                *x = galois::mul(galois::exp(x_i, k), r_i);
            }
        }

        let a = &mut *allocs.a;
        a.fill(0);
        a.iter_mut().step_by(dim + 1).for_each(|x| *x = 1);

        math::invert_matrix_with(s, dim, a);

        let u = &mut *allocs.u;
        for (ri, u) in a.chunks_exact(dim).zip(&mut *u) {
            *u = ri.iter().zip(&*f).map(|(&a, &b)| galois::mul(a, b)).fold(0, galois::add);
        }

        let q_poly = math::Poly::from_iter(u[..q].iter().copied());
        let e_poly = math::Poly::from_iter(u[q..].iter().copied().chain(iter::once(1)));

        let (p_poly, rem) = q_poly.div(e_poly).ok_or(BerklekampWelchError::DivPoly)?;

        if !rem.is_zero() {
            return Err(BerklekampWelchError::CannotRecover);
        }

        for (i, out) in correction.iter_mut().enumerate() {
            let pt = if i != 0 { galois::exp(interp_base, i - 1) } else { 0 };
            *out = p_poly.eval(pt);
        }

        Ok(())
    }

    fn syndrome_matrix(&self, shares: &mut [Share], resources: &mut Resources) -> (usize, usize) {
        let keepers = &mut *resources.presence_set;
        shares.iter_mut().for_each(|s| keepers[s.number] = true);

        let out = &mut *resources.syndrome;
        for row in 0..self.required {
            let mut skipped = 0;
            for col in 0..self.total {
                if !keepers[col] {
                    skipped += 1;
                    continue;
                }

                out[row * shares.len() + col - skipped] = self.vand_matrix[row * self.total + col];
            }
        }

        math::standardize_matrix(out, self.required, shares.len());

        math::parity_matrix(out, self.required, shares.len(), resources.parity)
    }
}

pub struct Resources<'a> {
    presence_set: &'a mut [bool],
    syndrome: &'a mut [u8],
    parity: &'a mut [u8],

    buf: &'a mut [u8],
    corrections: &'a mut [u8],

    e: usize,
    q: usize,
    s: &'a mut [u8],
    a: &'a mut [u8],
    f: &'a mut [u8],
    u: &'a mut [u8],
}

impl<'a> Resources<'a> {
    pub fn new(
        buffer: &'a mut Vec<u8>,
        required: usize,
        totoal: usize,
        shares: &mut [Share],
    ) -> Result<Self, ResourcesError> {
        shares.sort_unstable_by_key(|s| s.number);
        if shares.array_windows().any(|[a, b]| a.number == b.number) {
            return Err(ResourcesError::DuplicateShares);
        }

        if shares.len() < required {
            return Err(ResourcesError::NotEnoughShares);
        }

        let share_len = shares[0].data.len();
        if shares.iter().any(|s| s.data.len() != share_len) {
            return Err(ResourcesError::NotEqualLengths);
        }

        let presence_set_len = totoal;
        let syndrome_len = required * shares.len();
        let parity_len = (shares.len() - required) * shares.len();

        let buf_len = share_len;
        let corrections_len = totoal;

        let e = (shares.len() - required) / 2;

        if e == 0 {
            return Err(ResourcesError::NotEnoughShares);
        }

        let q = e + required;
        let dim = q + e;

        let s_len = dim * dim;
        let a_len = dim * dim;
        let f_len = dim;
        let u_len = dim;

        buffer.clear();
        buffer.resize(
            presence_set_len
                + syndrome_len
                + parity_len
                + buf_len
                + corrections_len
                + s_len
                + a_len
                + f_len
                + u_len,
            0,
        );

        let mut rest = &mut buffer[..];
        let mut take = |len| rest.take_mut(..len).expect("buffer too small, why?");

        Ok(Self {
            // SAFETY: the buffer is zeroed out
            presence_set: unsafe { &mut *(take(presence_set_len) as *mut [u8] as *mut [bool]) },
            syndrome: take(syndrome_len),
            parity: take(parity_len),
            buf: take(buf_len),
            corrections: take(corrections_len),
            e,
            q,
            s: take(s_len),
            a: take(a_len),
            f: take(f_len),
            u: take(u_len),
        })
    }
}

macro_rules! impl_error {
    ($(
        enum $name:ident {$(
            #[error($( $fmt:tt )*)]
            $variant:ident $(($inner:ty))?,
        )*}
    )*) => {$(
        #[derive(Debug)]
        pub enum $name {$(
            $variant $(($inner))?,
        )*}

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {$(
                    impl_error!(@pat $variant $(b $inner)?) =>
                        impl_error!(@expr f ($($fmt)*) $(b $inner)?),
                )*}
            }
        }

        impl std::error::Error for $name {}
    )*};

    (@pat $variant:ident $binding:ident $ty:ty) => { Self::$variant($binding) };
    (@pat $variant:ident) => { Self::$variant };
    (@expr $f:ident ($($fmt:tt)*) $binding:ident $inner:ty) => { write!($f, $($fmt)*, $binding) };
    (@expr $f:ident ($($fmt:tt)*)) => { write!($f, $($fmt)*) };
}

impl_error! {
    enum ResourcesError {
        #[error("duplicate shares")]
        DuplicateShares,
        #[error("not enough shares")]
        NotEnoughShares,
        #[error("not equal lengths")]
        NotEqualLengths,
    }

    enum EncodeError {
        #[error("invalid parity length")]
        InvalidParityLength,
        #[error("invalid data length")]
        InvalidDataLength,
    }

    enum RebuildError {
        #[error("not enough shares")]
        NotEnoughShares,
        #[error("not equal lengths")]
        NotEqualLengths,
        #[error("invert matrix")]
        InvertMatrix,
    }

    enum DecodeError {
        #[error("init resources: {0}")]
        InitResources(ResourcesError),
        #[error("correct: {0}")]
        Correct(CorrectError),
        #[error("rebuild: {0}")]
        Rebuild(RebuildError),
    }

    enum CorrectError {
        #[error("invalid shares: {0:?}")]
        InvalidShares(Vec<usize>),
        #[error("syndrome matrix: {0}")]
        SyndromeMatrix(SyndromeMatrixError),
        #[error("berklekamp welch: {0}")]
        BerklekampWelch(BerklekampWelchError),
    }

    enum SyndromeMatrixError {
        #[error("failed to standardize")]
        FailedToStandardize,
    }

    enum BerklekampWelchError {
        #[error("not enough shares")]
        NotEnoughShares,
        #[error("invert matrix")]
        InvertMatrix,
        #[error("div poly")]
        DivPoly,
        #[error("cannot recover")]
        CannotRecover,
    }
}

#[derive(Debug)]
pub struct Share<'a> {
    number: usize,
    data: &'a mut [u8],
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn encode_rebuild() {
        let mut data = [1, 2, 3, 4, 5, 6, 7, 8];
        let expected = data;
        let mut parity = [0; 8];

        let fec = Fec::new(2, 4);

        fec.encode(&data, &mut parity).unwrap();

        let (left, _) = parity.split_at_mut(4);
        let mut shares =
            [Share { number: 1, data: &mut data[4..] }, Share { number: 2, data: left }];

        let shares = fec.rebuild(&mut shares, &mut vec![]).unwrap();

        assert_eq!(shares[0].data, &expected[..4]);
        assert_eq!(shares[1].data, &expected[4..]);
    }

    #[test]
    fn encode_damage_correct() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut parity = [0; 8];

        let fec = Fec::new(2, 4);

        fec.encode(&data, &mut parity).unwrap();

        let (left_parity, right_parity) = parity.split_at_mut(4);
        let mut corrupted = data;
        corrupted[0] = 0;
        let (left, right) = corrupted.split_at_mut(4);
        let mut shares = [
            Share { number: 0, data: left },
            Share { number: 1, data: right },
            Share { number: 2, data: left_parity },
            Share { number: 3, data: right_parity },
        ];

        let shares = fec.decode(&mut shares, false, &mut vec![]).unwrap();

        assert_eq!(shares[0].data, &data[..4]);
        assert_eq!(shares[1].data, &data[4..]);
    }

    #[test]
    #[ignore]
    fn bench_reed_solomon() {
        let f = Fec::new(32, 64);

        let mut data = [0; 32 * 1024];
        data.iter_mut().enumerate().for_each(|(i, x)| *x = (i & 0xff).try_into().unwrap());
        let mut parity = [0; 32 * 1024];

        f.encode(&data, &mut parity).unwrap();

        let mut parity_cp = parity;
        let mut shards = parity_cp
            .chunks_exact_mut(1024)
            .enumerate()
            .map(|(i, x)| Share { number: i + 32, data: x })
            .collect::<Vec<_>>();

        let now = std::time::Instant::now();
        let iters = 100_000;
        let mut temp = vec![];
        for _ in 0..iters {
            f.rebuild(&mut shards, &mut temp).unwrap();
            for (shard, chunk) in shards.iter_mut().zip(parity.chunks_exact_mut(1024)) {
                shard.data.copy_from_slice(chunk);
                shard.number = usize::MAX - shard.number;
            }
        }
        println!("{:?}", now.elapsed().checked_div(iters).unwrap());
    }
}
