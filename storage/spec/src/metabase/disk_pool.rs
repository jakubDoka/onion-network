use {
    crate::sorted_compact_vec::{ConstsValue, SortedCompactVec, SortedElement, LANES},
    Codec,
    std::{
        io, iter,
        ops::{Deref, DerefMut, Range},
        os::unix::fs::FileExt,
        path::Path,
        simd::{
            cmp::{SimdOrd, SimdPartialEq},
            Mask, Simd, SimdElement,
        },
        sync::RwLock,
        usize,
    },
};

const CONTEXT_FILE_NAME: &str = "context.dp";
const GROUP_FILE_NAME_PREFIX: &str = "group";

const FILE_COUNT: usize = 1024;

/// # Safety
/// The type needs to be safely transmutabe to array of bytes.
/// The zeroed state of type should signify its deleted.
pub unsafe trait Record: Sized {
    const EXT: &'static str;

    type Elem: SimdElement + ConstsValue + Eq;

    fn contains_deleted(slice: &[Self]) -> bool
    where
        Simd<Self::Elem, LANES>:
            SimdPartialEq<Mask = Mask<<Self::Elem as SimdElement>::Mask, LANES>>,
    {
        let bytes = Self::to_elements(slice);
        let (prefix, simd, suffix) = bytes.as_simd::<64>();

        let mut element_progress = std::mem::size_of::<Self>();

        for chunk in
            prefix.chunks_exact(element_progress).chain(suffix.chunks_exact(element_progress))
        {
            if chunk.iter().all(|&b| b == Self::Elem::MIN) {
                return true;
            }
        }

        let mut iter = simd.iter();
        let query = Simd::splat(Self::Elem::MIN);
        while let Some(chunk) = iter.next() {
            let mut bits = chunk.simd_ne(query).to_bitmask();

            if 64 < std::mem::size_of::<Self>() {
                if bits.trailing_zeros() as usize >= element_progress {
                    return true;
                }

                if bits == 0 {
                    element_progress -= 64;
                    continue;
                }

                let advance = element_progress / 64;
                let Some(elem) = iter.nth(advance) else {
                    return false;
                };

                let bits = elem.simd_ne(query).to_bitmask();
                element_progress = std::mem::size_of::<Self>() - bits.leading_zeros() as usize;
            } else {
                let mut remining_bits = 64;

                if element_progress != std::mem::size_of::<Self>() {
                    if bits & ((1 << element_progress) - 1) == 0 {
                        return true;
                    }

                    bits >>= element_progress;
                    remining_bits -= element_progress;
                }

                for _ in 0..(remining_bits / std::mem::size_of::<Self>()) {
                    if bits.trailing_zeros() as usize >= element_progress {
                        return true;
                    }

                    bits >>= element_progress;
                }

                element_progress = std::mem::size_of::<Self>() - bits.leading_zeros() as usize;
            }
        }

        false
    }

    fn to_elements(slice: &[Self]) -> &[Self::Elem] {
        const { assert!(std::mem::align_of::<Self>() == std::mem::align_of::<Self::Elem>()) }

        unsafe {
            std::slice::from_raw_parts(
                slice.as_ptr() as *const Self::Elem,
                slice.len() * Self::record_size() / std::mem::size_of::<Self::Elem>(),
            )
        }
    }

    fn as_bytes(slice: &[Self]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                slice.as_ptr() as *const u8,
                slice.len() * Self::record_size(),
            )
        }
    }

    fn as_bytes_mut(slice: &mut [Self]) -> &mut [u8] {
        unsafe {
            std::slice::from_raw_parts_mut(
                slice.as_mut_ptr() as *mut u8,
                slice.len() * Self::record_size(),
            )
        }
    }

    fn file_capacity() -> usize {
        (1 << (32 - FILE_COUNT.ilog2())) * Self::record_size()
    }

    fn record_size() -> usize {
        std::mem::size_of::<Self>()
    }

    fn id_to_offsets(id: usize, count: usize) -> (usize, usize, usize) {
        let file_index = (id * Self::record_size()) / Self::file_capacity();
        let file_offset = (id * Self::record_size()) % Self::file_capacity();
        let rw_len =
            std::cmp::min(count * Self::record_size(), Self::file_capacity() - file_offset);
        (file_index, file_offset, rw_len / Self::record_size())
    }
}

#[derive(Codec)]
pub struct Context<T: SortedElement> {
    free_blocks: SortedCompactVec<T>,
}

impl<T: SortedElement + for<'a> Codec<'a>> Context<T> {
    pub fn open(root_dir: &Path) -> io::Result<Self> {
        let full_path = root_dir.join(CONTEXT_FILE_NAME);

        if !full_path.exists() {
            return Ok(Self::default());
        }

        let bytes = std::fs::read(full_path)?;

        Self::decode(&mut bytes.as_slice()).ok_or(io::ErrorKind::InvalidData.into())
    }

    pub fn save(&self, root_dir: &Path) -> io::Result<()> {
        let bytes = self.to_bytes();
        let full_path = root_dir.join(CONTEXT_FILE_NAME);
        std::fs::write(full_path, bytes)
    }
}

impl<T: SortedElement> Default for Context<T> {
    fn default() -> Self {
        Self { free_blocks: SortedCompactVec::new() }
    }
}

struct FileAccess {
    fd: std::fs::File,
    lock: std::sync::RwLock<()>,
}

pub struct Db<I, T> {
    files: Box<[FileAccess]>,
    #[allow(clippy::type_complexity)]
    _marker: std::marker::PhantomData<(T, fn(I) -> I)>,
}

impl<I: SortedElement, T: Record> Db<I, T> {
    pub fn open(root_dir: &Path) -> std::io::Result<Self> {
        let files = (0..FILE_COUNT)
            .map(|i| {
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(root_dir.join(format!("{GROUP_FILE_NAME_PREFIX}{i}.{}", T::EXT,)))
                    .map(|fd| FileAccess { fd, lock: RwLock::new(()) })
            })
            .collect::<Result<Box<[_]>, _>>()?;

        Ok(Self { files, _marker: std::marker::PhantomData })
    }

    pub fn fetch<'a, 'b>(
        &'b self,
        start: I,
        buffer: &mut &'a mut [T],
    ) -> io::Result<ReadAccess<'a, 'b, T>>
    where
        Simd<T::Elem, LANES>: SimdPartialEq<Mask = Mask<<T::Elem as SimdElement>::Mask, LANES>>,
    {
        let (file_index, file_offset, read_len) =
            T::id_to_offsets(start.try_into().ok().expect("doom"), buffer.len());
        let file = &self.files.get(file_index).ok_or(io::ErrorKind::NotFound)?;
        let slice = buffer.take_mut(..read_len).unwrap();
        let _guard = file.lock.read().ok().ok_or(io::ErrorKind::BrokenPipe)?;
        file.fd.read_exact_at(T::as_bytes_mut(slice), file_offset as u64)?;
        if T::contains_deleted(slice) {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(ReadAccess { _guard, slice })
    }

    pub fn fetch_iter<'a, 'b>(
        &'b self,
        start: I,
        mut buffer: &'a mut [T],
    ) -> impl Iterator<Item = io::Result<ReadAccess<'a, 'b, T>>>
    where
        Simd<T::Elem, LANES>: SimdPartialEq<Mask = Mask<<T::Elem as SimdElement>::Mask, LANES>>,
    {
        iter::from_fn(move || {
            if buffer.is_empty() {
                return None;
            }

            Some(self.fetch(start, &mut buffer))
        })
    }

    pub fn update<'a, 'b>(
        &'b self,
        range: Range<I>,
        update_space: &mut &'a mut [T],
    ) -> io::Result<WriteAccess<'a, 'b, T>>
    where
        Simd<T::Elem, LANES>: SimdPartialEq<Mask = Mask<<T::Elem as SimdElement>::Mask, LANES>>,
    {
        let (file_index, file_offset, read_len) =
            T::id_to_offsets(range.start.try_into().ok().expect("doom"), update_space.len());
        let file = &self.files.get(file_index).ok_or(io::ErrorKind::NotFound)?;
        let slice = update_space.take_mut(..read_len).unwrap();
        let _guard = file.lock.write().ok().ok_or(io::ErrorKind::BrokenPipe)?;
        file.fd.read_exact_at(T::as_bytes_mut(slice), file_offset as u64)?;
        if T::contains_deleted(slice) {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(WriteAccess { _guard, slice, fd: &file.fd })
    }

    pub fn write(&self, range: Range<I>, block: &mut &[T]) -> io::Result<()> {
        let (file_index, file_offset, write_len) =
            T::id_to_offsets(range.start.try_into().ok().expect("well, fuck"), block.len());
        let file = &self.files.get(file_index).ok_or(io::ErrorKind::NotFound)?;
        let to_write = block.take(..write_len).expect("it can only be less");
        let _guard = file.lock.write().unwrap();
        file.fd.write_all_at(T::as_bytes(to_write), file_offset as u64)?;
        Ok(())
    }

    pub fn push_exact(&self, block: &mut &[T], context: &mut Context<I>) -> io::Result<Range<I>>
    where
        Simd<I, LANES>: SimdOrd<Mask = Mask<I::Mask, LANES>>,
    {
        let size = block.len().try_into().ok().ok_or(io::ErrorKind::InvalidInput)?;
        let range = context.free_blocks.pop_exact(size).ok_or(io::ErrorKind::OutOfMemory)?;
        self.write(range.clone(), block)?;
        Ok(range)
    }

    pub fn push(&self, block: &mut &[T], context: &mut Context<I>) -> io::Result<Range<I>> {
        let size = block.len().try_into().ok().ok_or(io::ErrorKind::InvalidInput)?;
        let range = context.free_blocks.pop_n(size).ok_or(io::ErrorKind::OutOfMemory)?;
        self.write(range.clone(), block)?;
        Ok(range)
    }

    pub fn push_iter<'a>(
        &'a self,
        mut block: &'a [T],
        context: &'a mut Context<I>,
    ) -> impl Iterator<Item = io::Result<Range<I>>> + 'a {
        iter::from_fn(move || {
            if block.is_empty() {
                return None;
            }

            Some(self.push(&mut block, context))
        })
    }

    pub fn release(&self, range: Range<I>, context: &mut Context<I>) {
        context.free_blocks.push_range(range);
    }
}

pub struct ReadAccess<'a, 'b, T> {
    _guard: std::sync::RwLockReadGuard<'b, ()>,
    slice: &'a [T],
}

impl<T> Deref for ReadAccess<'_, '_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.slice
    }
}

pub struct WriteAccess<'a, 'b, T: Record> {
    _guard: std::sync::RwLockWriteGuard<'b, ()>,
    fd: &'b std::fs::File,
    slice: &'a mut [T],
}

impl<T: Record> Deref for WriteAccess<'_, '_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.slice
    }
}

impl<T: Record> DerefMut for WriteAccess<'_, '_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.slice
    }
}

impl<T: Record> Drop for WriteAccess<'_, '_, T> {
    fn drop(&mut self) {
        let _ = self.fd.write_all_at(T::as_bytes(self.slice), 0);
    }
}
