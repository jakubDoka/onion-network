use {
    crate::{Buffer, Decode, Encode, Reminder},
    core::{
        convert::Infallible,
        marker::PhantomData,
        net::{IpAddr, SocketAddr},
        ops::Range,
    },
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

impl<T: Encode> Encode for Range<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.start.encode(buffer)?;
        self.end.encode(buffer)
    }
}

impl<'a, T: Decode<'a>> Decode<'a> for Range<T> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self { start: T::decode(buffer)?, end: T::decode(buffer)? })
    }
}

impl<'b, T: Encode + ?Sized> Encode for &'b T {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        (*self).encode(buffer)
    }
}

impl Encode for Infallible {
    fn encode(&self, _: &mut impl Buffer) -> Option<()> {
        match self {
            &s => match s {},
        }
    }
}

impl<'a> Decode<'a> for Infallible {
    fn decode(_: &mut &'a [u8]) -> Option<Self> {
        None
    }
}

impl<T> Encode for PhantomData<T> {
    fn encode(&self, _: &mut impl Buffer) -> Option<()> {
        Some(())
    }
}

impl<'a, T> Decode<'a> for PhantomData<T> {
    fn decode(_: &mut &'a [u8]) -> Option<Self> {
        Some(Self)
    }
}

impl<'a> Decode<'a> for SocketAddr {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self::new(<IpAddr>::decode(buffer)?, <u16>::decode(buffer)?))
    }
}

impl Encode for SocketAddr {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.ip().encode(buffer)?;
        self.port().encode(buffer)
    }
}

impl Encode for IpAddr {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        match self {
            IpAddr::V4(v4) => {
                0u8.encode(buffer)?;
                v4.encode(buffer)
            }
            IpAddr::V6(v6) => {
                1u8.encode(buffer)?;
                v6.encode(buffer)
            }
        }
    }
}

impl<'a> Decode<'a> for IpAddr {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        match <u8>::decode(buffer)? {
            0 => std::net::Ipv4Addr::decode(buffer).map(IpAddr::V4),
            1 => std::net::Ipv6Addr::decode(buffer).map(IpAddr::V6),
            _ => None,
        }
    }
}

impl Encode for std::net::Ipv4Addr {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.extend_from_slice(&self.octets())
    }
}

impl<'a> Decode<'a> for std::net::Ipv4Addr {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let octets = <[u8; 4]>::decode(buffer)?;
        Some(Self::from(octets))
    }
}

impl Encode for std::net::Ipv6Addr {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.extend_from_slice(&self.octets())
    }
}

impl<'a> Decode<'a> for std::net::Ipv6Addr {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let octets = <[u8; 16]>::decode(buffer)?;
        Some(Self::from(octets))
    }
}

impl<R: Encode, E: Encode> Encode for Result<R, E> {
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
}

impl<'a, R: Decode<'a>, E: Decode<'a>> Decode<'a> for Result<R, E> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let is_ok = <bool>::decode(buffer)?;
        Some(if is_ok { Ok(R::decode(buffer)?) } else { Err(E::decode(buffer)?) })
    }
}

impl Encode for () {
    fn encode(&self, _buffer: &mut impl Buffer) -> Option<()> {
        Some(())
    }
}

impl Decode<'_> for () {
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
        impl Encode for $t {
            fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
                base128_encode(*self as u64, buffer)
            }
        }

        impl<'a> Decode<'a> for $t {
            fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
                base128_decode(buffer)?.try_into().ok()
            }
        }
    )*};
}

impl_int!(u16, u32, u64, u128, usize);

impl Encode for bool {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.push(u8::from(*self))
    }
}

impl<'a> Decode<'a> for bool {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let Some((&bool_byte @ (0 | 1), rest)) = buffer.split_first() else {
            return None;
        };
        *buffer = rest;
        Some(bool_byte == 1)
    }
}

impl Encode for u8 {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.push(*self)
    }
}

impl<'a> Decode<'a> for u8 {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let (&byte, rest) = buffer.split_first()?;
        *buffer = rest;
        Some(byte)
    }
}

impl Encode for Reminder<'_> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.extend_from_slice(self.0)
    }
}

impl<'a> Decode<'a> for Reminder<'a> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self(core::mem::take(buffer)))
    }
}

impl Encode for &str {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_bytes().encode(buffer)
    }
}

impl<'a> Decode<'a> for &'a str {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let bytes = <&[u8]>::decode(buffer)?;
        core::str::from_utf8(bytes).ok()
    }
}

#[cfg(feature = "arrayvec")]
#[cfg(feature = "arrayvec")]
impl<T: Encode, const LEN: usize> Encode for arrayvec::ArrayVec<T, LEN> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }
}

#[cfg(feature = "arrayvec")]
impl<'a, T: Decode<'a>, const LEN: usize> Decode<'a> for arrayvec::ArrayVec<T, LEN> {
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
impl<const LEN: usize> Encode for arrayvec::ArrayString<LEN> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_bytes().encode(buffer)
    }
}

#[cfg(feature = "arrayvec")]
impl<'a, const LEN: usize> Decode<'a> for arrayvec::ArrayString<LEN> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let bytes = <&[u8]>::decode(buffer)?;
        Self::from(core::str::from_utf8(bytes).ok()?).ok()
    }
}

impl Encode for [u8] {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        buffer.extend_from_slice(self)
    }
}

impl<'a> Decode<'a> for &'a [u8] {
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

impl<T: Encode, const LEN: usize> Encode for [T; LEN] {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }
}

impl<'a, T: Decode<'a>, const LEN: usize> Decode<'a> for [T; LEN] {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let mut s = std::mem::MaybeUninit::<Self>::uninit().transpose();
        for (i, v) in s.iter_mut().enumerate() {
            match <T>::decode(buffer) {
                Some(t) => _ = v.write(t),
                None => {
                    for j in &mut s[0..i] {
                        unsafe { j.as_mut_ptr().drop_in_place() }
                    }
                    return None;
                }
            }
        }
        Some(unsafe { s.transpose().assume_init() })
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        match self {
            Some(t) => {
                true.encode(buffer)?;
                t.encode(buffer)
            }
            None => false.encode(buffer),
        }
    }
}

impl<'a, T: Decode<'a>> Decode<'a> for Option<T> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let is_some = <bool>::decode(buffer)?;
        Some(if is_some { Some(<T>::decode(buffer)?) } else { None })
    }
}

macro_rules! derive_tuples {
    ($($($t:ident),*;)*) => {$(
        #[allow(non_snake_case)]
        impl<$($t: Encode),*> Encode for ($($t,)*) {
            fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
                let ($($t,)*) = self;
                $($t.encode(buffer)?;)*
                Some(())
            }
        }

        impl<'a, $($t: Decode<'a>),*> Decode<'a> for ($($t,)*) {
            fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
                Some(($($t::decode(buffer)?,)*))
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
