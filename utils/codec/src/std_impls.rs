use {
    crate::{Buffer, Decode, Encode},
    std::{
        borrow::{Borrow, Cow},
        collections::HashSet,
        hash::BuildHasher,
        io,
        sync::Arc,
    },
};

impl<K: Encode, V: Encode, H: BuildHasher> Encode for std::collections::HashMap<K, V, H> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for (k, v) in self {
            k.encode(buffer)?;
            v.encode(buffer)?;
        }
        Some(())
    }
}

impl<'a, K: Decode<'a> + Eq + std::hash::Hash, V: Decode<'a>, H: BuildHasher + Default> Decode<'a>
    for std::collections::HashMap<K, V, H>
{
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

impl<T: Encode> Encode for HashSet<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }
}

impl<'a, T: Decode<'a> + Eq + std::hash::Hash> Decode<'a> for HashSet<T> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let len = usize::decode(buffer)?;
        if len > buffer.len() {
            return None;
        }
        let mut s = Self::with_capacity(len);
        for _ in 0..len {
            s.insert(<T>::decode(buffer)?);
        }
        Some(s)
    }
}

impl Encode for Box<[u8]> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_ref().encode(buffer)
    }
}

impl<'a> Decode<'a> for Box<[u8]> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(<&[u8]>::decode(buffer)?.into())
    }
}

impl<T: ToOwned + Encode> Encode for Cow<'_, T>
where
    T::Owned: Encode,
{
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_ref().borrow().encode(buffer)
    }
}

impl<'a, T: ToOwned + Decode<'a>> Decode<'a> for Cow<'a, T>
where
    T::Owned: Decode<'a>,
{
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Cow::Owned(<T::Owned>::decode(buffer)?))
    }
}

impl<K: Encode, V: Encode> Encode for std::collections::BTreeMap<K, V> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for (k, v) in self {
            k.encode(buffer)?;
            v.encode(buffer)?;
        }
        Some(())
    }
}

impl<'a, K: Decode<'a> + Ord, V: Decode<'a>> Decode<'a> for std::collections::BTreeMap<K, V> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let len = usize::decode(buffer)?;
        if len * 2 > buffer.len() {
            return None;
        }
        let mut s = Self::new();
        for _ in 0..len {
            let k = K::decode(buffer)?;
            let v = V::decode(buffer)?;
            s.insert(k, v);
        }
        Some(s)
    }
}

impl<T: Encode + Ord> Encode for std::collections::BTreeSet<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }
}

impl<'a, T: Decode<'a> + Ord> Decode<'a> for std::collections::BTreeSet<T> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let len = usize::decode(buffer)?;
        if len > buffer.len() {
            return None;
        }
        let mut s = Self::new();
        for _ in 0..len {
            s.insert(<T>::decode(buffer)?);
        }
        Some(s)
    }
}

impl<T: Encode> Encode for std::collections::VecDeque<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }
}

impl<'a, T: Decode<'a>> Decode<'a> for std::collections::VecDeque<T> {
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

const ERROR_LIST: &'static [io::ErrorKind] = &[
    io::ErrorKind::NotFound,
    io::ErrorKind::PermissionDenied,
    io::ErrorKind::ConnectionRefused,
    io::ErrorKind::ConnectionReset,
    io::ErrorKind::ConnectionAborted,
    io::ErrorKind::NotConnected,
    io::ErrorKind::AddrInUse,
    io::ErrorKind::AddrNotAvailable,
    io::ErrorKind::BrokenPipe,
    io::ErrorKind::AlreadyExists,
    io::ErrorKind::WouldBlock,
    io::ErrorKind::InvalidInput,
    io::ErrorKind::InvalidData,
    io::ErrorKind::TimedOut,
    io::ErrorKind::WriteZero,
    io::ErrorKind::Interrupted,
    io::ErrorKind::Unsupported,
    io::ErrorKind::UnexpectedEof,
    io::ErrorKind::OutOfMemory,
    io::ErrorKind::Other,
];

impl Encode for io::ErrorKind {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        ERROR_LIST.iter().position(|e| e == self).unwrap_or(127).encode(buffer)
    }
}

impl<'a> Decode<'a> for io::ErrorKind {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let idx = usize::decode(buffer)?;
        ERROR_LIST.get(idx).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ReminderOwned(pub Vec<u8>);

impl ReminderOwned {
    pub fn as_slice(&self) -> crate::Reminder {
        crate::Reminder(&self.0)
    }
}

impl AsRef<[u8]> for ReminderOwned {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Encode for ReminderOwned {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.extend_from_slice(&self.0)
    }
}

impl<'a> Decode<'a> for ReminderOwned {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self(std::mem::take(buffer).to_vec()))
    }
}

pub struct ReminderVec<T>(pub Vec<T>);

impl<T> AsRef<[T]> for ReminderVec<T> {
    fn as_ref(&self) -> &[T] {
        self.0.as_ref()
    }
}

impl<T: Encode> Encode for ReminderVec<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        for i in &self.0 {
            i.encode(buffer)?;
        }
        Some(())
    }
}

impl<'a, T: Decode<'a>> Decode<'a> for ReminderVec<T> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let mut s = Vec::new();
        while !buffer.is_empty() {
            s.push(<T>::decode(buffer)?);
        }
        Some(Self(s))
    }
}

impl Encode for String {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_str().encode(buffer)
    }
}

impl<'a> Decode<'a> for String {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        let str = <&str>::decode(buffer)?;
        Some(str.to_string())
    }
}

impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self {
            i.encode(buffer)?;
        }
        Some(())
    }
}

impl<'a, T: Decode<'a>> Decode<'a> for Vec<T> {
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

impl<T: Encode> Encode for Arc<[T]> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.len().encode(buffer)?;
        for i in self.iter() {
            i.encode(buffer)?;
        }
        Some(())
    }
}

impl<'a, T: Decode<'a>> Decode<'a> for Arc<[T]> {
    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(<Vec<T>>::decode(buffer)?.into())
    }
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

pub struct WritableBuffer<'a, T> {
    pub buffer: &'a mut T,
}

impl<'a, T: Buffer> io::Write for WritableBuffer<'a, T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf).ok_or(io::ErrorKind::OutOfMemory)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<A: smallvec::Array<Item = u8>> Buffer for smallvec::SmallVec<A> {
    fn push(&mut self, byte: u8) -> Option<()> {
        self.push(byte);
        Some(())
    }

    fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()> {
        self.extend_from_slice(slice);
        Some(())
    }
}
