use {
    component_utils::Codec,
    std::{
        ops::Range,
        ptr::Unique,
        simd::{cmp::SimdPartialOrd, Mask, Simd, SimdElement},
    },
};

pub trait SortedElement:
    std::ops::Add<Output = Self>
    + std::ops::Sub<Output = Self>
    + PartialOrd
    + Ord
    + Copy
    + SimdElement
    + std::ops::AddAssign
    + std::ops::SubAssign
    + ConstsValue
    + TryFrom<usize>
    + TryInto<usize>
{
}

impl<T> SortedElement for T where
    T: std::ops::Add<Output = Self>
        + std::ops::Sub<Output = Self>
        + PartialOrd
        + Ord
        + Copy
        + SimdElement
        + std::ops::AddAssign
        + std::ops::SubAssign
        + ConstsValue
        + TryFrom<usize>
        + TryInto<usize>
{
}

pub trait ConstsValue {
    const MAX: Self;
    const MIN: Self;
    const ONE: Self;
}

macro_rules! impl_max_value {
    ($($t:ty),*) => {$(
        impl ConstsValue for $t {
            const MAX: Self = <$t>::MAX;
            const MIN: Self = <$t>::MIN;
            const ONE: Self = 1;
        }
    )*};
}

impl_max_value!(u8, u16, u32, u64, u128, usize);

#[derive(Codec)]
pub struct SortedCompactVec<T: SortedElement> {
    data: SimdStorage<T>,
}

impl<T: SortedElement> SortedCompactVec<T> {
    pub fn new() -> Self {
        let mut data = SimdStorage::default();
        data.insert(0, T::MIN..T::MAX);
        Self { data }
    }

    pub fn allocated() -> Self {
        Self { data: SimdStorage::default() }
    }

    pub fn lowest_active(&self) -> Option<T> {
        // this may panic but at that point we are fucked anyway
        self.data.first().map(|r| r.end())
    }

    #[must_use]
    pub fn push(&mut self, value: T) -> bool {
        self.push_range(value..value + T::ONE)
    }

    pub fn push_range(&mut self, range: Range<T>) -> bool {
        let Err(i) = self.data.slices().0.binary_search(&range.start) else {
            return false;
        };

        let inc_prev = self
            .data
            .get(i.wrapping_sub(1))
            .is_some_and(|other| range.start == other.end() + T::ONE);
        let dec_next = self.data.get(i).is_some_and(|other| range.end == *other.start + T::ONE);
        match (inc_prev, dec_next) {
            (true, true) => {
                let other = self.data.remove(i).unwrap();
                *self.data.get_mut(i - 1).unwrap().len += other.end - other.start;
            }
            (true, false) => *self.data.get_mut(i - 1).unwrap().len += range.end - range.start,
            (false, true) => {
                *self.data.get_mut(i).unwrap().start = range.start;
                *self.data.get_mut(i).unwrap().len += range.end - range.start;
            }
            (false, false) => {
                self.data.insert(i, range);
            }
        }

        true
    }

    pub fn pop(&mut self) -> Option<T> {
        let last = self.data.last_mut()?;
        if *last.len == T::MIN {
            self.data.pop().map(|r| r.start)
        } else {
            *last.len -= T::ONE;
            Some(last.end() + T::ONE)
        }
    }

    pub fn pop_n(&mut self, n: T) -> Option<Range<T>> {
        let last = self.data.last_mut()?;

        if *last.len < n {
            self.data.pop()
        } else {
            *last.len -= n;
            Some(last.end()..last.end() + n)
        }
    }

    pub fn pop_exact(&mut self, n: T) -> Option<Range<T>>
    where
        Simd<T, LANES>: SimdPartialOrd<Mask = Mask<T::Mask, LANES>>,
    {
        let [.., (lens_simd, lens)] = self.data.simd_slices();
        let query = Simd::<T, LANES>::splat(n);
        let i = lens_simd
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, &len)| {
                let bit_mask = len.simd_ge(query).to_bitmask();
                if bit_mask == 0 {
                    return None;
                }

                let index = bit_mask.trailing_zeros() as usize;
                Some(i * LANES + index)
            })
            .or_else(|| {
                lens.iter().enumerate().rev().find_map(|(i, &len)| {
                    if len >= n {
                        Some(i + lens_simd.len() * LANES)
                    } else {
                        None
                    }
                })
            })?;

        let changed = self.data.get_mut(i).unwrap();
        if *changed.len == n {
            self.data.remove(i)
        } else {
            *changed.len -= n;
            Some(changed.end()..changed.end() + n)
        }
    }

    pub fn pop_exact_slow(&mut self, n: T) -> Option<Range<T>> {
        let (.., lens) = self.data.slices();
        let i = lens.iter().enumerate().rev().find_map(
            |(i, &len)| {
                if len >= n {
                    Some(i)
                } else {
                    None
                }
            },
        )?;

        let changed = self.data.get_mut(i).unwrap();
        if *changed.len == n {
            self.data.remove(i)
        } else {
            *changed.len -= n;
            Some(changed.end()..changed.end() + n)
        }
    }
}

impl<T: SortedElement> Default for SortedCompactVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub const LANES: usize = 64;

struct SimdStorage<T: SimdElement> {
    dara: Unique<Simd<T, LANES>>,
    cap: usize, // in Tx64s / 2
    len: usize, // in Ts / 2
}

impl<'a, T: SimdElement + Codec<'a>> Codec<'a> for SimdStorage<T> {
    fn encode(&self, buffer: &mut impl component_utils::Buffer) -> Option<()> {
        let (left, right) = self.slices();
        self.len.encode(buffer)?;
        for e in left.iter().chain(right.iter()) {
            e.encode(buffer)?;
        }
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let len = usize::decode(buffer)?;
        let mut data = Self::default();
        data.ensure_capacity(len);
        let [left, right] = data.left_right_t();

        for i in 0..len {
            unsafe { *left.add(i) = T::decode(buffer)? };
        }
        for i in 0..len {
            unsafe { *right.add(i) = T::decode(buffer)? };
        }

        data.len = len;
        Some(data)
    }
}

impl<T: SimdElement> SimdStorage<T> {
    fn insert(&mut self, index: usize, elem: Range<T>)
    where
        T: std::ops::Sub<Output = T>,
    {
        self.ensure_capacity(1);

        let [left, right] = self.left_right_t();
        unsafe {
            std::ptr::copy(left.add(index), left.add(index + 1), self.len - index);
            std::ptr::copy(right.add(index), right.add(index + 1), self.len - index);
            *left.add(index) = elem.start;
            *right.add(index) = elem.end - elem.start;
        }
        self.len += 1;
    }

    fn remove(&mut self, index: usize) -> Option<Range<T>> {
        if index >= self.len {
            return None;
        }

        let [left, right] = self.left_right_t();
        let elem = unsafe { *left.add(index)..*right.add(index) };
        unsafe {
            std::ptr::copy(left.add(index + 1), left.add(index), self.len - index - 1);
            std::ptr::copy(right.add(index + 1), right.add(index), self.len - index - 1);
        }
        self.len -= 1;
        Some(elem)
    }

    fn pop(&mut self) -> Option<Range<T>> {
        self.remove(self.len.checked_sub(1)?)
    }

    fn slices(&self) -> (&[T], &[T]) {
        let [left, right] = self.left_right_t();
        unsafe {
            (
                std::slice::from_raw_parts(left, self.len),
                std::slice::from_raw_parts(right, self.len),
            )
        }
    }

    fn simd_slices(&self) -> [(&[Simd<T, LANES>], &[T]); 2] {
        let [left, right] = self.left_right();
        let (simd_len, len) = self.project_len();
        unsafe {
            [
                (
                    std::slice::from_raw_parts(left, simd_len),
                    std::slice::from_raw_parts(left.add(simd_len) as _, len),
                ),
                (
                    std::slice::from_raw_parts(right, simd_len),
                    std::slice::from_raw_parts(right.add(simd_len) as _, len),
                ),
            ]
        }
    }

    fn get(&self, index: usize) -> Option<RangeRef<T>> {
        let [left, right] = self.left_right_t();
        if index >= self.len {
            return None;
        }

        Some(unsafe { RangeRef { start: &*left.add(index), len: &*right.add(index) } })
    }

    fn get_mut(&mut self, index: usize) -> Option<RangeMut<T>> {
        let [left, right] = self.left_right_t();
        if index >= self.len {
            return None;
        }

        Some(unsafe { RangeMut { start: &mut *left.add(index), len: &mut *right.add(index) } })
    }

    fn first(&self) -> Option<RangeRef<T>> {
        self.get(0)
    }

    fn last_mut(&mut self) -> Option<RangeMut<T>> {
        self.get_mut(self.len - 1)
    }

    fn left_right(&self) -> [*mut Simd<T, LANES>; 2] {
        let ptr = self.dara.as_ptr();
        unsafe { [ptr, ptr.add(self.cap)] }
    }

    fn left_right_t(&self) -> [*mut T; 2] {
        let ptr = self.dara.as_ptr() as *mut T;
        unsafe { [ptr, ptr.add(self.cap * LANES)] }
    }

    fn project_len(&self) -> (usize, usize) {
        (self.len / LANES, self.len % LANES)
    }

    fn ensure_capacity(&mut self, len: usize) {
        if self.len + len > self.cap * LANES {
            self.expand(((self.len + len + LANES - 1) / LANES).next_power_of_two());
        }
    }

    #[cold]
    fn expand(&mut self, new_cap: usize) {
        let new_layout = Self::compute_layout(new_cap);
        if self.cap == 0 {
            self.dara = unsafe { Unique::new_unchecked(std::alloc::alloc(new_layout) as _) };
        } else {
            let old_layout = Self::compute_layout(self.cap);
            self.dara = unsafe {
                Unique::new_unchecked(std::alloc::realloc(
                    self.dara.as_ptr() as _,
                    old_layout,
                    new_layout.size(),
                ) as _)
            };

            let prev_right = unsafe { self.dara.as_ptr().add(self.cap) as *mut T };
            let new_right = unsafe { self.dara.as_ptr().add(new_cap) as *mut T };
            unsafe {
                std::ptr::copy(prev_right, new_right, self.len);
            }
        }
        self.cap = new_cap;
    }

    fn compute_layout(cap: usize) -> std::alloc::Layout {
        unsafe {
            std::alloc::Layout::from_size_align_unchecked(
                cap * std::mem::size_of::<Simd<T, LANES>>() * 2,
                std::mem::align_of::<Simd<T, LANES>>(),
            )
        }
    }
}

impl<T: SimdElement> Default for SimdStorage<T> {
    fn default() -> Self {
        Self { dara: Unique::dangling(), cap: 0, len: 0 }
    }
}

impl<T: SimdElement> Drop for SimdStorage<T> {
    fn drop(&mut self) {
        if self.cap == 0 {
            return;
        }

        unsafe {
            std::alloc::dealloc(self.dara.as_ptr() as *mut u8, Self::compute_layout(self.cap))
        }
    }
}

pub struct RangeMut<'a, T> {
    start: &'a mut T,
    len: &'a mut T,
}

impl<T> RangeMut<'_, T> {
    fn end(&self) -> T
    where
        T: std::ops::Add<Output = T> + Copy,
    {
        *self.start + *self.len
    }
}

pub struct RangeRef<'a, T> {
    start: &'a T,
    len: &'a T,
}

impl<T> RangeRef<'_, T> {
    fn end(&self) -> T
    where
        T: std::ops::Add<Output = T> + Copy,
    {
        *self.start + *self.len
    }
}

#[cfg(test)]
mod test {
    use std::time::Instant;

    #[test]
    fn test_sorted_vec_expansion() {
        let mut vec = super::SortedCompactVec::allocated();
        let max_free: usize = 100000;
        for i in 0..max_free {
            if i % 2 != 0 {
                assert!(vec.push_range(i * 3..i * 3 + 3));
            }
        }

        let now = Instant::now();
        let iters = 10000;
        for _ in 0..iters {
            assert_eq!(vec.pop_exact(max_free + 2), None);
        }
        println!("pop_exact: {:?}", now.elapsed().checked_div(iters).unwrap());

        let now = Instant::now();
        for _ in 0..iters {
            assert_eq!(vec.pop_exact_slow(max_free + 2), None);
        }
        println!("pop_exact_slow: {:?}", now.elapsed().checked_div(iters).unwrap());
    }
}
