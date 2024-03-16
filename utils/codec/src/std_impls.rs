use {
    crate::{Buffer, Codec},
    std::{
        borrow::{Borrow, Cow},
        hash::BuildHasher,
        io,
        sync::Arc,
    },
};

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

impl<'a> Codec<'a> for Box<[u8]> {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_ref().encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(<&[u8]>::decode(buffer)?.into())
    }
}

impl<'a, T: ToOwned + Codec<'a>> Codec<'a> for Cow<'a, T>
where
    T::Owned: Codec<'a>,
{
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        self.as_ref().borrow().encode(buffer)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Cow::Owned(<T::Owned>::decode(buffer)?))
    }
}

impl<'a, K: Codec<'a> + Ord, V: Codec<'a>> Codec<'a> for std::collections::BTreeMap<K, V> {
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
        let mut s = Self::new();
        for _ in 0..len {
            let k = K::decode(buffer)?;
            let v = V::decode(buffer)?;
            s.insert(k, v);
        }
        Some(s)
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ReminderOwned(pub Vec<u8>);

impl<'a> Codec<'a> for ReminderOwned {
    fn encode(&self, buffer: &mut impl Buffer) -> Option<()> {
        buffer.extend_from_slice(&self.0)
    }

    fn decode(buffer: &mut &'a [u8]) -> Option<Self> {
        Some(Self(std::mem::take(buffer).to_vec()))
    }
}

impl AsRef<[u8]> for ReminderOwned {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
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
