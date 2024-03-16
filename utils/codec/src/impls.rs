use {
    crate::{Buffer, Codec, Reminder},
    core::{convert::Infallible, marker::PhantomData, ops::Range},
};

#[cfg(feature = "arrayvec")]
impl<const SIZE: usize> Buffer for arrayvec::ArrayVec<u8, SIZE> {
    fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()> {
        self.try_extend_from_slice(slice).ok()
    }

    fn push(&mut self, byte: u8) -> Option<()> {
        self.try_push(byte).ok()
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

impl<'a, T: Codec<'a>> Codec<'a> for Range<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.start.encode(buffer)?;
        self.end.encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self { start: T::decode(buffer)?, end: T::decode(buffer)? })
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

impl Codec<'_> for () {
    fn encode(&self, _buffer: &mut impl Buffer) -> Option<()> {
        Some(())
    }

    fn decode(_buffer: &mut &[u8]) -> Option<Self> {
        Some(())
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

impl<'a> Codec<'a> for Reminder<'a> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.extend_from_slice(self.0)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self(core::mem::take(buffer)))
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

#[cfg(feature = "arrayvec")]
impl<'a, T: Codec<'a>, const LEN: usize> Codec<'a> for arrayvec::ArrayVec<T, LEN> {
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

#[cfg(feature = "arrayvec")]
impl<'a, const LEN: usize> Codec<'a> for arrayvec::ArrayString<LEN> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_bytes().encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let bytes = <&[u8]>::decode(buffer)?;
        let str = core::str::from_utf8(bytes).ok()?;
        Self::from(str).ok()
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
        if tries.iter().any(core::option::Option::is_none) {
            return None;
        }
        Some(tries.map(|t| t.expect("to be some, since we checked")))
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
