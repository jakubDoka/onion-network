use {
    crate::galois,
    std::{iter, mem, usize},
};

pub fn standardize_matrix(mx: &mut [u8], r: usize, c: usize) {
    assert_eq!(mx.len(), r * c);

    for i in 0..r {
        let Some((p_row, p_val)) = (i..r).map(|j| (j, mx[j * c + i])).find(|&(_, x)| x != 0) else {
            continue;
        };

        if p_row != i {
            matrix_swap_rows(mx, c, i, p_row);
        }

        let inv = galois::div(1, p_val).expect("we checked");
        galois::mul_slice_in_place(inv, &mut mx[i * c..i * c + c]);

        for j in i + 1..r {
            matrix_add_mul_rows(mx, c, i, j, mx[j * c + i]);
        }
    }

    for i in (0..r).rev() {
        for j in (0..i).rev() {
            matrix_add_mul_rows(mx, c, i, j, mx[j * c + i]);
        }
    }
}

pub fn invert_matrix_with(mx: &mut [u8], k: usize, out: &mut [u8]) {
    assert_eq!(mx.len(), k * k);
    assert_eq!(out.len(), k * k);

    for i in 0..k {
        let Some((p_row, p_val)) = (i..k).map(|j| (j, mx[j * k + i])).find(|&(_, x)| x != 0) else {
            continue;
        };

        if p_row != i {
            matrix_swap_rows(mx, k, i, p_row);
            matrix_swap_rows(out, k, i, p_row);
        }

        let inv = galois::div(1, p_val).expect("we checked");
        galois::mul_slice_in_place(inv, &mut mx[i * k..i * k + k]);
        galois::mul_slice_in_place(inv, &mut out[i * k..i * k + k]);

        debug_assert!(mx[i * k + i] == 1, "{mx:?}");

        for j in i + 1..k {
            let fac = mx[j * k + i];
            matrix_add_mul_rows(mx, k, i, j, fac);
            matrix_add_mul_rows(out, k, i, j, fac);
        }
    }

    for i in (0..k).rev() {
        for j in (0..i).rev() {
            let fac = mx[j * k + i];
            matrix_add_mul_rows(mx, k, i, j, fac);
            matrix_add_mul_rows(out, k, i, j, fac);
        }
    }

    debug_assert!((0..k).all(|i| mx[i * k + i] == 1), "{mx:?}");
}

pub fn parity_matrix(mx: &[u8], r: usize, c: usize, out: &mut [u8]) -> (usize, usize) {
    assert_eq!(mx.len(), r * c);
    assert_eq!(out.len(), (c - r) * c);

    for i in 0..c - r {
        out[i * c + i + r] = 1;
    }

    for i in 0..c - r {
        for j in 0..r {
            out[i * c + j] = mx[j * c + i + r];
        }
    }

    ((c - r), c)
}

pub fn invert_matrix(mx: &mut [u8], k: usize) -> Option<()> {
    assert_eq!(mx.len(), k * k);

    let mut unused_rows = vec![true; k];
    let mut swaps = vec![];
    for i in 0..k {
        let pivot = unused_rows
            .iter_mut()
            .enumerate()
            .position(|(j, unused)| mx[j * k + i] != 0 && mem::take(unused))?;

        if pivot != i {
            let pair @ [a, b] = [pivot.min(i), pivot.max(i)];
            matrix_swap_rows(mx, k, a, b);
            swaps.push(pair.into());
        }

        let (above_pivot, rest) = mx.split_at_mut(i * k);
        let (pivot_row, below_pivot) = rest.split_at_mut(k);

        let c = pivot_row[i];

        if c != 1 {
            let c = galois::div(1, c)?;
            pivot_row[i] = 1;
            galois::mul_slice_in_place(c, pivot_row);
        }

        if pivot_row[..i].iter().chain(pivot_row[i + 1..].iter()).all(|&x| x == 0) {
            continue;
        }

        // we avoid chain since that for some reason makes things slower

        for row in above_pivot.chunks_exact_mut(k) {
            let c = std::mem::take(&mut row[i]);
            galois::mul_slice_xor(c, pivot_row, row);
        }

        for row in below_pivot.chunks_exact_mut(k) {
            let c = std::mem::take(&mut row[i]);
            galois::mul_slice_xor(c, pivot_row, row);
        }
    }

    for (a, b) in swaps.into_iter().rev() {
        matrix_swap_rows(mx, k, a, b);
    }

    Some(())
}

#[inline]
pub fn matrix_swap_rows(mx: &mut [u8], c: usize, a: usize, b: usize) {
    assert!(a < b);
    let (left, right) = mx.split_at_mut(b * c);
    left[a * c..a * c + c].swap_with_slice(&mut right[..c]);
}

#[inline]
pub fn matrix_add_mul_rows(mx: &mut [u8], c: usize, a: usize, b: usize, fac: u8) {
    let (source, dest) = if a < b {
        let (left, right) = mx.split_at_mut(b * c);
        (&left[a * c..a * c + c], &mut right[..c])
    } else {
        let (left, right) = mx.split_at_mut(a * c);
        (&right[..c], &mut left[b * c..b * c + c])
    };
    galois::mul_slice_xor(fac, source, dest);
}

pub fn create_inverted_vdm(vdm: &mut [u8], k: usize) {
    assert!(vdm.len() >= k * k);

    if k == 1 {
        vdm[0] = 1;
        return;
    }

    let mut b = vec![0; k];
    let mut c = vec![0; k];

    c[k - 1] = 0;
    for i in 1..k {
        let mul_p_i = &galois::MUL_TABLE[galois::EXP_TABLE[i] as usize];
        for j in (k - 1 - (i - 1))..(k - 1) {
            c[j] ^= mul_p_i[c[j + 1] as usize];
        }
        c[k - 1] ^= galois::EXP_TABLE[i];
    }

    for row in 0..k {
        let index = if row != 0 { galois::EXP_TABLE[row] as usize } else { 0 };
        let mul_p_row = &galois::MUL_TABLE[index];

        let mut t = 1;
        b[k - 1] = 1;
        for i in (0..(k - 1)).rev() {
            b[i] = c[i + 1] ^ mul_p_row[b[i + 1] as usize];
            t = b[i] ^ mul_p_row[t as usize];
        }

        let mul_t_inv = &galois::MUL_TABLE[galois::INV_TABLE[t as usize] as usize];
        for col in 0..k {
            vdm[col * k + row] = mul_t_inv[b[col] as usize];
        }
    }
}

#[derive(Debug, Clone)]
pub struct Poly {
    data: [u8; 256],
    len: u16,
}

impl Poly {
    pub fn from_iter(iter: impl IntoIterator<Item = u8>) -> Self {
        let mut data = [0; 256];
        let mut iter = iter.into_iter();

        let len = data.iter_mut().zip(iter.by_ref()).map(|(a, b)| *a = b).count();

        assert!(iter.next().is_none(), "too many elements");

        Self { data, len: len.try_into().unwrap() }
    }

    pub fn zero(size: usize) -> Self {
        Self { data: [0; 256], len: size.try_into().unwrap() }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len as usize]
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data[..self.len as usize]
    }

    pub fn is_zero(&self) -> bool {
        self.as_slice().iter().all(|&x| x == 0)
    }

    pub const fn deg(&self) -> Option<usize> {
        (self.len as usize).checked_sub(1)
    }

    pub fn scale(mut self, factor: u8) -> Self {
        galois::mul_slice_in_place(factor, self.as_mut_slice());
        self
    }

    pub fn add(mut self, b: &Self) -> Self {
        self.as_mut_slice()
            .iter_mut()
            .zip(b.as_slice())
            .for_each(|(a, b)| *a = galois::add(*a, *b));
        self
    }

    pub fn sanitize(&mut self) {
        let trailing_zeros = self.as_slice().iter().rev().take_while(|&&x| x == 0).count();
        self.len = (self.len as usize - trailing_zeros).max(1).try_into().unwrap();
    }

    #[must_use]
    pub fn push(&mut self, x: u8) -> Option<()> {
        *self.data.get_mut(self.len as usize)? = x;
        self.len += 1;
        Some(())
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }

        let out = self.data[self.len as usize];
        self.len -= 1;
        Some(out)
    }

    pub fn div(mut self, mut b: Self) -> Option<(Self, Self)> {
        self.sanitize();
        b.sanitize();

        if b.as_slice().is_empty() {
            return None;
        }

        if self.as_slice().is_empty() {
            return Some((Self::zero(1), Self::zero(1)));
        }

        let mut q = Self::zero(1);
        let mut p = self;

        while b.deg() <= p.deg() {
            let leading_p = p.as_slice().last().copied().unwrap_or(0);
            let leading_b = b.as_slice().last().copied().unwrap();

            let coef = galois::div(leading_p, leading_b)?;
            q.push(coef)?;

            let scaled = b.clone().scale(coef);
            let padded = Self::from_iter(
                iter::repeat(0)
                    .take(p.deg()? - scaled.deg()?)
                    .chain(scaled.as_slice().iter().copied()),
            );

            p = p.add(&padded);
            let pop = p.pop();
            debug_assert!(pop == Some(0));
        }

        q.as_mut_slice().reverse();

        p.sanitize();
        q.sanitize();

        Some((q, p))
    }

    pub fn eval(&self, x: u8) -> u8 {
        self.as_slice()
            .iter()
            .enumerate()
            .map(|(i, coef)| galois::mul(*coef, galois::exp(x, i)))
            .fold(0, galois::add)
    }
}
