use {
    arrayvec::{ArrayString, ArrayVec},
    core::{convert::Infallible, marker::PhantomData},
    std::{ops::Range, sync::Arc, u32, usize},
};

pub const PACKET_LEN_WIDTH: usize = std::mem::size_of::<PacketLen>();
pub type PacketLen = u16;

#[must_use]
pub fn encode_len(len: usize) -> [u8; PACKET_LEN_WIDTH] {
    (len as PacketLen).to_be_bytes()
}

#[must_use]
pub fn decode_len(bytes: [u8; PACKET_LEN_WIDTH]) -> usize {
    PacketLen::from_be_bytes(bytes) as usize
}

pub struct WritableBuffer<'a, T> {
    pub buffer: &'a mut T,
}

impl<'a, T: Buffer> std::io::Write for WritableBuffer<'a, T> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buf).ok_or(std::io::ErrorKind::OutOfMemory)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a, T: Codec<'a>> Codec<'a> for Range<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.start.encode(buffer)?;
        self.end.encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self { start: T::decode(buffer)?, end: T::decode(buffer)? })
    }
}

pub trait Buffer: AsMut<[u8]> {
    #[must_use = "handle the error"]
    fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()>;
    #[must_use = "handle the error"]
    fn push(&mut self, byte: u8) -> Option<()>;
}

impl Buffer for Vec<u8> {
    fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()> {
        self.extend_from_slice(slice);
        Some(())
    }

    fn push(&mut self, byte: u8) -> Option<()> {
        self.push(byte);
        Some(())
    }
}

impl Buffer for &mut [u8] {
    fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()> {
        self.take_mut(..slice.len())?.copy_from_slice(slice);
        Some(())
    }

    fn push(&mut self, byte: u8) -> Option<()> {
        *self.take_first_mut()? = byte;
        Some(())
    }
}

pub trait Codec<'a>: Sized {
    #[must_use = "handle the error"]
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()>;
    fn decode(buffer: &mut &'a [u8]) -> Option<Self>;

    fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        self.encode(&mut buffer).expect("to encode");
        buffer
    }

    fn to_packet(&self) -> Vec<u8> {
        let mut buffer = vec![0; 4];
        self.encode(&mut buffer).expect("to encode");
        buffer.splice(..4, encode_len(buffer.len() - 4));
        buffer
    }
}

impl<'a, 'b, T: Codec<'a>> Codec<'a> for &'b T {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        (*self).encode(buffer)
    }

    fn decode(_: &mut &'a [u8]) -> Option<Self> {
        unreachable!("&T is not a valid codec")
    }
}

impl Codec<'_> for Infallible {
    fn encode(&self, _: &mut impl Buffer) -> Option<()> {
        match self {
            &s => match s {},
        }
    }

    fn decode(_: &mut &[u8]) -> Option<Self> {
        None
    }
}

impl<'a, T: Codec<'a>> Codec<'a> for std::collections::VecDeque<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let len = usize::decode(buffer)?;
        if len > buffer.len() {
            return None;
        }
        let mut s = Self::with_capacity(len);
        for _ in 0..len {
            s.push_back(<T>::decode(buffer)?);
        }
        Some(s)
    }
}

impl<'a, T> Codec<'a> for PhantomData<T> {
    fn encode(&self, _: &mut impl Buffer) -> Option<()> {
        Some(())
    }

    fn decode(_: &mut &'a [u8]) -> Option<Self> {
        Some(Self)
    }
}

impl<'a, R: Codec<'a>, E: Codec<'a>> Codec<'a> for Result<R, E> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        match self {
            Ok(r) => {
                true.encode(buffer)?;
                r.encode(buffer)
            }
            Err(e) => {
                false.encode(buffer)?;
                e.encode(buffer)
            }
        }
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let is_ok = <bool>::decode(buffer)?;
        Some(if is_ok { Ok(<R>::decode(buffer)?) } else { Err(<E>::decode(buffer)?) })
    }
}

use {
    futures::{AsyncRead, AsyncReadExt},
    std::hash::BuildHasher,
};
pub trait CodecExt: for<'a> Codec<'a> {
    #[allow(async_fn_in_trait)]
    async fn from_stream(mut stream: impl AsyncRead + Unpin) -> std::io::Result<Self> {
        let mut len = [0; 2];
        stream.read_exact(&mut len).await?;
        let len = decode_len(len);
        let mut buffer = vec![0; len];
        stream.read_exact(&mut buffer).await?;
        // SAFETY: compiler is stupid, we implement Codec<'static>
        Self::decode(&mut &buffer[..]).ok_or_else(|| std::io::ErrorKind::InvalidData.into())
    }
}

impl<T: for<'a> Codec<'a>> CodecExt for T {}

impl Codec<'_> for () {
    fn encode(&self, _buffer: &mut impl Buffer) -> Option<()> {
        Some(())
    }

    fn decode(_buffer: &mut &[u8]) -> Option<Self> {
        Some(())
    }
}

pub struct Base128Bytes(u64, bool);

impl Base128Bytes {
    #[must_use]
    pub fn new(value: u64) -> Self {
        Self(value, true)
    }
}

impl Iterator for Base128Bytes {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if !core::mem::take(&mut self.1) && self.0 == 0 {
            return None;
        }

        let byte = (self.0 & 0b0111_1111) as u8;
        self.0 >>= 7;
        if self.0 != 0 {
            self.0 |= 0b1000_0000;
        }
        Some(byte)
    }
}

fn base128_encode(mut value: u64, buffer: &mut impl Buffer) -> Option<()> {
    loop {
        let mut byte = (value & 0b0111_1111) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0b1000_0000;
        }
        buffer.push(byte)?;
        if value == 0 {
            break Some(());
        }
    }
}

fn base128_decode(buffer: &mut &[u8]) -> Option<u64> {
    let mut value = 0;
    let mut shift = 0;
    let worst_case_size = 10;
    for (advanced, byte) in (*buffer).iter().take(worst_case_size).copied().enumerate() {
        value |= u64::from(byte & 0b0111_1111) << shift;
        shift += 7;
        if byte & 0b1000_0000 == 0 {
            *buffer = &buffer[advanced + 1..];
            return Some(value);
        }
    }
    None
}

macro_rules! impl_int {
    ($($t:ty),*) => {$(
            impl<'a> Codec<'a> for $t {
                fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
                    base128_encode(*self as u64, buffer)
                }

                fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
                    base128_decode(buffer).map(|v| v as $t)
                }
            }
    )*};
}

impl_int!(u16, u32, u64, u128, usize);

impl<'a> Codec<'a> for bool {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.push(u8::from(*self))
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let Some((&bool_byte @ (0 | 1), rest)) = buffer.split_first() else {
            return None;
        };
        *buffer = rest;
        Some(bool_byte == 1)
    }
}

impl<'a> Codec<'a> for u8 {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.push(*self)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let (&byte, rest) = buffer.split_first()?;
        *buffer = rest;
        Some(byte)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Reminder<'a>(pub &'a [u8]);

impl<'a> Codec<'a> for Reminder<'a> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.extend_from_slice(self.0)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self(core::mem::take(buffer)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnedReminder(pub Arc<[u8]>);

impl<'a> Codec<'a> for OwnedReminder {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.extend_from_slice(&self.0)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self(core::mem::take(buffer).into()))
    }
}

/// Some structures suport optimized codec when placed as the last part of the message
#[derive(Debug, Clone, Copy)]
pub struct Unbound<T>(pub T);

impl<'a> Codec<'a> for Unbound<Vec<u8>> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.extend_from_slice(&self.0)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self(core::mem::take(buffer).to_vec()))
    }
}

impl<'a> Codec<'a> for String {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_str().encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let str = <&str>::decode(buffer)?;
        Some(str.to_string())
    }
}

impl<'a> Codec<'a> for &'a str {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_bytes().encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let bytes = <&[u8]>::decode(buffer)?;
        core::str::from_utf8(bytes).ok()
    }
}

impl<'a, T: Codec<'a>, const LEN: usize> Codec<'a> for ArrayVec<T, LEN> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let len = <usize>::decode(buffer)?;
        if len > LEN {
            return None;
        }
        let mut s = Self::default();
        for _ in 0..len {
            s.push(<T>::decode(buffer)?);
        }
        Some(s)
    }
}

impl<'a, const LEN: usize> Codec<'a> for ArrayString<LEN> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_bytes().encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let bytes = <&[u8]>::decode(buffer)?;
        let str = core::str::from_utf8(bytes).ok()?;
        Self::from(str).ok()
    }
}

impl<'a, T: Codec<'a>> Codec<'a> for Vec<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let len = <usize>::decode(buffer)?;
        if len > buffer.len() {
            return None;
        }
        let mut s = Self::with_capacity(len);
        for _ in 0..len {
            s.push(<T>::decode(buffer)?);
        }
        Some(s)
    }
}

impl<'a, T: Codec<'a>> Codec<'a> for Arc<[T]> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self.iter() {
            i.encode(buffer)?;
        }
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(<Vec<T>>::decode(buffer)?.into())
    }
}

impl<'a> Codec<'a> for &'a [u8] {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        buffer.extend_from_slice(self)?;
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let len = <usize>::decode(buffer)?;
        if buffer.len() < len {
            return None;
        }

        let (bytes, rest) = buffer.split_at(len);
        *buffer = rest;
        Some(bytes)
    }
}

impl<'a, T: Codec<'a>, const SIZE: usize> Codec<'a> for [T; SIZE] {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let tries = [(); SIZE].map(|()| <T>::decode(buffer));
        if tries.iter().any(std::option::Option::is_none) {
            return None;
        }
        Some(tries.map(|t| t.expect("to be some, since we checked")))
    }
}

impl<'a> Codec<'a> for Box<[u8]> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_ref().encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(<&[u8]>::decode(buffer)?.into())
    }
}

impl<'a, T: Codec<'a>> Codec<'a> for Option<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        match self {
            Some(t) => {
                true.encode(buffer)?;
                t.encode(buffer)
            }
            None => false.encode(buffer),
        }
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let is_some = <bool>::decode(buffer)?;
        Some(if is_some { Some(<T>::decode(buffer)?) } else { None })
    }
}

impl<'a, K: Codec<'a> + Eq + std::hash::Hash, V: Codec<'a>, H: BuildHasher + Default> Codec<'a>
    for std::collections::HashMap<K, V, H>
{
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for (k, v) in self {
            k.encode(buffer)?;
            v.encode(buffer)?;
        }
        Some(())
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let len = usize::decode(buffer)?;
        if len * 2 > buffer.len() {
            return None;
        }
        let mut s = Self::with_capacity_and_hasher(len, H::default());
        for _ in 0..len {
            let k = K::decode(buffer)?;
            let v = V::decode(buffer)?;
            s.insert(k, v);
        }
        Some(s)
    }
}

macro_rules! derive_tuples {
    ($($($t:ident),*;)*) => {$(
        #[allow(non_snake_case)]
        impl<'a, $($t: Codec<'a>),*> Codec<'a> for ($($t,)*) {
            fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
                let ($($t,)*) = self;
                $($t.encode(buffer)?;)*
                Some(())
            }

            fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
                Some(($(<$t>::decode(buffer)?,)*))
            }
        }
    )*};
}

derive_tuples! {
    A;
    A, B;
    A, B, C;
    A, B, C, D;
    A, B, C, D, E;
    A, B, C, D, E, F;
}
