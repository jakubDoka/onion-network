use {
    codec::{Buffer, Codec, Reminder},
    futures::Future,
    std::{future::poll_fn, io, pin::Pin, task::Poll, usize},
};

const PACKET_LEN_WIDTH: usize = 2;

pub struct AsocStream<A, S> {
    pub inner: S,
    pub assoc: A,
}

impl<A, S> AsocStream<A, S> {
    pub const fn new(inner: S, assoc: A) -> Self {
        Self { inner, assoc }
    }
}

impl<A: Clone, S: futures::Stream> futures::Stream for AsocStream<A, S> {
    type Item = (A, S::Item);

    fn poll_next(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Self::Item>> {
        unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.inner) }
            .poll_next(cx)
            .map(|opt| opt.map(|item| (self.assoc.clone(), item)))
    }
}

#[derive(Debug, Default)]
pub struct PacketReader {
    read_buffer: Vec<u8>,
    read_offset: usize,
}

impl PacketReader {
    fn poll_read_exact(
        &mut self,
        cx: &mut core::task::Context<'_>,
        stream: &mut (impl futures::AsyncRead + Unpin),
        amount: usize,
    ) -> Poll<Result<(), io::Error>> {
        if self.read_offset >= amount {
            return Poll::Ready(Ok(()));
        }

        if self.read_buffer.len() < amount {
            self.read_buffer.resize(amount, 0);
        }

        while self.read_offset < amount {
            let n = futures::ready!(Pin::new(&mut *stream)
                .poll_read(cx, &mut self.read_buffer[self.read_offset..amount]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
            self.read_offset += n;
        }

        Poll::Ready(Ok(()))
    }

    pub fn poll_packet<'a>(
        &'a mut self,
        cx: &mut core::task::Context<'_>,
        stream: &mut (impl futures::AsyncRead + Unpin),
    ) -> Poll<Result<&'a mut [u8], io::Error>> {
        futures::ready!(self.poll_read_exact(cx, stream, PACKET_LEN_WIDTH))?;

        let packet_size =
            u16::from_be_bytes(self.read_buffer[..PACKET_LEN_WIDTH].try_into().unwrap()) as usize;

        futures::ready!(self.poll_read_exact(cx, stream, packet_size + PACKET_LEN_WIDTH))?;

        let packet = &mut self.read_buffer[PACKET_LEN_WIDTH..packet_size + PACKET_LEN_WIDTH];
        self.read_offset = 0;
        Poll::Ready(Ok(packet))
    }

    pub async fn next_packet(
        &mut self,
        stream: &mut (impl futures::AsyncRead + Unpin),
    ) -> Result<Vec<u8>, io::Error> {
        poll_fn(|cx| self.poll_packet(cx, stream).map_ok(|v| v.to_vec())).await
    }

    pub async fn next_packet_as<T: for<'a> Codec<'a>>(
        &mut self,
        stream: &mut (impl futures::AsyncRead + Unpin),
    ) -> Result<T, io::Error> {
        poll_fn(|cx| {
            self.poll_packet(cx, stream).map(|v| {
                v.and_then(|v| T::decode(&mut &*v).ok_or(io::ErrorKind::InvalidData.into()))
            })
        })
        .await
    }
}

#[repr(transparent)]
pub struct NoCapOverflow {
    vec: Vec<u8>,
}

impl NoCapOverflow {
    pub fn new(vec: &mut Vec<u8>) -> &mut Self {
        unsafe { std::mem::transmute(vec) }
    }
}

impl Buffer for NoCapOverflow {
    fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()> {
        if self.vec.len() + slice.len() + 2 > self.vec.capacity() {
            return None;
        }
        self.vec.extend_from_slice(slice);
        Some(())
    }

    fn push(&mut self, byte: u8) -> Option<()> {
        if self.vec.len() + 2 == self.vec.capacity() {
            return None;
        }
        self.vec.push(byte);
        Some(())
    }
}

impl AsMut<[u8]> for NoCapOverflow {
    fn as_mut(&mut self) -> &mut [u8] {
        self.vec.as_mut()
    }
}

#[derive(Debug)]
pub struct PacketWriter {
    buffer: Vec<u8>,
    start: usize,
    end: usize,
    waker: Option<core::task::Waker>,
}

impl PacketWriter {
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self { buffer: Vec::with_capacity(cap), start: 0, end: 0, waker: None }
    }

    pub fn write_packet<'a>(&mut self, message: &impl Codec<'a>) -> Option<()> {
        let mut writer = self.guard();
        let reserved = writer.write([0u8; PACKET_LEN_WIDTH])?;
        let len = writer.write(message)?.len();
        reserved.copy_from_slice(&(len as u16).to_be_bytes());
        Some(())
    }

    pub fn guard(&mut self) -> PacketWriterGuard {
        let free_cap = self.buffer.capacity() - self.buffer.len();
        let space = if self.start > self.end { self.end..self.start } else { 0..self.start };
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
        if free_cap < space.len() {
            self.end *= usize::from(self.buffer.len() != self.end);
            PacketWriterGuard::Replacing {
                written: 0,
                target: &mut self.buffer[space],
                end: &mut self.end,
            }
        } else {
            PacketWriterGuard::Extending {
                end: if self.end >= self.start {
                    Ok(&mut self.end)
                } else {
                    Err(self.buffer.len())
                },
                target: &mut self.buffer,
            }
        }
    }

    pub fn poll(
        &mut self,
        cx: &mut core::task::Context<'_>,
        dest: &mut (impl futures::AsyncWrite + Unpin),
    ) -> Poll<Result<(), io::Error>> {
        loop {
            let lr = self.writable_parts();
            let Some(some_bytes) = <[_; 2]>::from(lr).into_iter().find(|s| !s.is_empty()) else {
                crate::set_waker(&mut self.waker, cx.waker());
                if self.start == self.end {
                    self.start = 0;
                    self.end = 0;
                }
                if self.start <= self.end {
                    self.buffer.truncate(self.end);
                }
                return Poll::Ready(Ok(()));
            };

            let n = futures::ready!(Pin::new(&mut *dest).poll_write(cx, some_bytes))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.start += n;
            self.start -= self.buffer.len() * usize::from(self.start > self.buffer.len());
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    fn writable_parts(&mut self) -> (&mut [u8], &mut [u8]) {
        match self.start.cmp(&self.end) {
            std::cmp::Ordering::Greater => {
                let (rest, first) = self.buffer.split_at_mut(self.start);
                let (second, _) = rest.split_at_mut(self.end);
                (first, second)
            }
            std::cmp::Ordering::Less => (&mut self.buffer[self.start..self.end], &mut []),
            std::cmp::Ordering::Equal => (&mut [], &mut []),
        }
    }
}

pub enum PacketWriterGuard<'a> {
    Extending { target: &'a mut Vec<u8>, end: Result<&'a mut usize, usize> },
    Replacing { written: usize, target: &'a mut [u8], end: &'a mut usize },
}

impl<'a> PacketWriterGuard<'a> {
    pub fn write<'b>(&mut self, value: impl Codec<'b>) -> Option<&'a mut [u8]> {
        struct RawSliceBuffer {
            start: *mut u8,
            end: *mut u8,
        }

        impl codec::Buffer for RawSliceBuffer {
            fn extend_from_slice(&mut self, slice: &[u8]) -> Option<()> {
                if (self.end as usize) < self.start as usize + slice.len() {
                    return None;
                }

                unsafe {
                    core::ptr::copy_nonoverlapping(slice.as_ptr(), self.start, slice.len());
                    self.start = self.start.add(slice.len());
                }

                Some(())
            }

            fn push(&mut self, byte: u8) -> Option<()> {
                if self.end == self.start {
                    return None;
                }

                unsafe {
                    *self.start = byte;
                    self.start = self.start.add(1);
                }

                Some(())
            }
        }

        impl AsMut<[u8]> for RawSliceBuffer {
            fn as_mut(&mut self) -> &mut [u8] {
                unsafe {
                    core::slice::from_raw_parts_mut(
                        self.start,
                        self.end as usize - self.start as usize,
                    )
                }
            }
        }

        match self {
            PacketWriterGuard::Extending { target, end, .. } => {
                let end = end.as_mut().map_or_else(|e| e, |v| &mut **v);
                let mut sbuf = RawSliceBuffer {
                    start: unsafe { target.as_mut_ptr().add(*end) },
                    end: unsafe { target.as_mut_ptr().add(target.capacity()) },
                };
                let failed = value.encode(&mut sbuf).is_none();
                if failed {
                    *end = target.len();
                    None
                } else {
                    // SAFETY: we do not reallocate the buffer, ever
                    let slice = unsafe {
                        core::slice::from_raw_parts_mut(
                            target.as_mut_ptr().add(*end),
                            sbuf.start as usize - *end - target.as_mut_ptr() as usize,
                        )
                    };
                    *end += slice.len();
                    Some(slice)
                }
            }
            PacketWriterGuard::Replacing { target, written, .. } => {
                let mut sub_target = &mut **target;
                let failed = value.encode(&mut sub_target).is_none();
                let remonder_len = sub_target.len();
                let space_taken = target.len() - remonder_len;
                let (written_slice, rest) =
                    unsafe { std::mem::take(target).split_at_mut_unchecked(space_taken) };
                *target = rest;

                if failed {
                    *written = 0;
                    None
                } else {
                    *written += written_slice.len();
                    Some(written_slice)
                }
            }
        }
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Option<&'a mut [u8]> {
        self.write(Reminder(bytes))
    }
}

impl Drop for PacketWriterGuard<'_> {
    fn drop(&mut self) {
        match self {
            &mut PacketWriterGuard::Extending { end: Ok(&mut end) | Err(end), ref mut target } => unsafe {
                target.set_len(end);
            },
            PacketWriterGuard::Replacing { end, written, .. } => **end += *written,
        }
    }
}

pub struct ClosingStream<S> {
    stream: S,
    error: u8,
}

impl<S> ClosingStream<S> {
    pub const fn new(stream: S, error: u8) -> Self {
        Self { stream, error }
    }
}

impl<S: futures::AsyncWrite + Unpin> Future for ClosingStream<S> {
    type Output = Result<(), io::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        match futures::ready!(Pin::new(&mut this.stream).poll_write(cx, &[this.error]))? {
            0 => Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
            _ => Poll::Ready(Ok(())),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, core::task::Context, futures::task::noop_waker_ref};

    #[test]
    fn test_write() {
        let mut writer = PacketWriter::new(14);
        let mut buf = [3u8; 10];
        assert_eq!(writer.guard().write(buf), Some(&mut buf[..]));

        writer.end = 6;
        let mut buf = [1u8; 4];
        assert_eq!(writer.guard().write(buf), Some(&mut buf[..]));

        writer.start = 8;
        let mut buf = [2u8; 4];
        assert_eq!(writer.guard().write(buf), Some(&mut buf[..]));
    }

    #[test]
    fn test_poll() {
        struct DummyWrite(usize);

        impl futures::AsyncWrite for DummyWrite {
            fn poll_write(
                mut self: Pin<&mut Self>,
                _: &mut core::task::Context<'_>,
                buf: &[u8],
            ) -> Poll<Result<usize, io::Error>> {
                Poll::Ready(Ok(buf.len().min(std::mem::take(&mut self.0))))
            }

            fn poll_flush(
                self: Pin<&mut Self>,
                _: &mut core::task::Context<'_>,
            ) -> Poll<Result<(), io::Error>> {
                Poll::Ready(Ok(()))
            }

            fn poll_close(
                self: Pin<&mut Self>,
                _: &mut core::task::Context<'_>,
            ) -> Poll<Result<(), io::Error>> {
                Poll::Ready(Ok(()))
            }
        }

        let mut writer = PacketWriter::new(100);

        for i in 0..10 {
            writer.guard().write([0u8; 20]).unwrap();
            _ = writer.poll(&mut Context::from_waker(noop_waker_ref()), &mut DummyWrite(11 + i));
        }

        for _ in 0..100 {
            _ = writer.poll(&mut Context::from_waker(noop_waker_ref()), &mut DummyWrite(20));
            writer.guard().write([0u8; 20]).unwrap();
        }
    }

    #[test]
    fn test_overflow() {
        let mut writer = PacketWriter::new(10);
        let buf = [0u8; 11];
        assert_eq!(writer.guard().write(buf), None);
    }
}
