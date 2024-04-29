#![feature(iter_next_chunk)]
#![feature(impl_trait_in_assoc_type)]

use {
    libp2p_core::{multiaddr::Protocol as MP, transport::TransportError as TE, Multiaddr},
    std::{
        cell::Cell,
        fmt,
        future::Future,
        io,
        pin::Pin,
        rc::Rc,
        task::{Poll, Waker},
    },
    wasm_bindgen::{prelude::Closure, JsCast, JsValue},
    web_sys::{window, CloseEvent, MessageEvent, WebSocket},
};

fn parse_multiaddr(ma: &Multiaddr) -> Result<String, &'static str> {
    let Ok([ip, MP::Tcp(port), MP::Ws(path)]) = ma.iter().next_chunk() else {
        return Err("expected /ip/tcp/ws");
    };

    let ip = match ip {
        MP::Ip4(ip) => ip.to_string(),
        MP::Ip6(ip) => ip.to_string(),
        _ => return Err("expected /ip4 or /ip6 as the first component"),
    };

    Ok(format!("ws://{ip}:{port}{path}"))
}

pub struct Transport {
    trottle_period: i32,
}

impl Transport {
    #[must_use]
    pub fn new(trottle_period: i32) -> Self {
        Self { trottle_period }
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new(100)
    }
}

impl libp2p_core::Transport for Transport {
    type Dial = ConnectionFut;
    type Error = Error;
    type ListenerUpgrade = std::future::Pending<Result<Connection, Self::Error>>;
    type Output = Connection;

    fn listen_on(
        &mut self,
        _: libp2p_core::transport::ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TE<Self::Error>> {
        Err(TE::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, _: libp2p_core::transport::ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TE<Self::Error>> {
        let Ok(addr) = parse_multiaddr(&addr) else {
            return Err(TE::MultiaddrNotSupported(addr));
        };

        Connection::dial(&addr, self.trottle_period).map_err(TE::Other)
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, libp2p_core::transport::TransportError<Self::Error>> {
        Err(libp2p_core::transport::TransportError::MultiaddrNotSupported(addr))
    }

    fn poll(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_core::transport::TransportEvent<Self::ListenerUpgrade, Self::Error>>
    {
        Poll::Pending
    }

    fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}

#[derive(Debug)]
pub struct Error(JsValue);

impl From<JsValue> for Error {
    fn from(e: JsValue) -> Self {
        Self(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::error::Error for Error {}

unsafe impl Send for Error {}
unsafe impl Sync for Error {}

struct Timeout {
    _closure: Closure<dyn FnMut()>,
    id: i32,
}

impl Timeout {
    fn new<F: FnMut() + 'static>(dur: i32, f: F) -> Self {
        let closure = Closure::<dyn FnMut()>::new(f);
        let id = window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                dur,
            )
            .unwrap();
        Self { _closure: closure, id }
    }
}

impl Drop for Timeout {
    fn drop(&mut self) {
        window().unwrap().clear_timeout_with_handle(self.id);
    }
}

type CloseCallback = Closure<dyn FnMut(CloseEvent)>;
type ReadCallback = Closure<dyn FnMut(MessageEvent)>;

struct ConnectionState {
    _state_closure: CloseCallback,
    state_waker: Cell<Option<Waker>>,

    _read_closure: ReadCallback,
    read_waker: Cell<Option<Waker>>,

    read_buf: Cell<Vec<u8>>,

    trottle_callback: Cell<Option<Timeout>>,
}

impl ConnectionState {
    fn new(ws: WebSocket) -> Rc<Self> {
        Rc::<Self>::new_cyclic(|state| {
            let close_state = state.clone();
            let _close_closure = Closure::<dyn FnMut(CloseEvent)>::new(move |_| {
                if let Some(waker) = close_state.upgrade().and_then(|s| s.state_waker.take()) {
                    waker.wake();
                }
            });
            ws.set_onopen(Some(_close_closure.as_ref().unchecked_ref()));
            ws.set_onclose(Some(_close_closure.as_ref().unchecked_ref()));
            ws.set_onerror(Some(_close_closure.as_ref().unchecked_ref()));

            let read_state = state.clone();
            let _read_closure = Closure::<dyn FnMut(_)>::new(move |e: MessageEvent| {
                let Some(state) = read_state.upgrade() else { return };
                let Ok(array) = e.data().dyn_into::<js_sys::ArrayBuffer>() else { return };

                let array = js_sys::Uint8Array::new(&array);
                let mut buf = state.read_buf.take();
                buf.extend(array.to_vec());
                state.read_buf.set(buf);

                if let Some(waker) = state.read_waker.take() {
                    waker.wake();
                }
            });
            ws.set_onmessage(Some(_read_closure.as_ref().unchecked_ref()));

            Self {
                _state_closure: _close_closure,
                state_waker: Cell::new(None),
                _read_closure,
                read_waker: Cell::new(None),
                read_buf: Cell::new(Vec::new()),
                trottle_callback: Cell::new(None),
            }
        })
    }
}

pub struct Connection {
    inner: WebSocket,
    state: Rc<ConnectionState>,
    trottle_period: i32,
}

impl Connection {
    pub fn dial(url: &str, trottle_period: i32) -> Result<ConnectionFut, Error> {
        let sock = WebSocket::new(url)?;
        sock.set_binary_type(web_sys::BinaryType::Arraybuffer);
        let state = ConnectionState::new(sock.clone());
        let conn = Some(Self { inner: sock, state, trottle_period });
        Ok(ConnectionFut(conn))
    }
}

impl futures::AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        if matches!(self.inner.ready_state(), WebSocket::CLOSING | WebSocket::CLOSED) {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        self.state.read_waker.set(Some(cx.waker().clone()));
        let mut inner_buf = self.state.read_buf.take();
        let written = buf.len().min(inner_buf.len());
        buf[..written].copy_from_slice(&inner_buf[..written]);
        inner_buf.drain(..written);
        self.state.read_buf.set(inner_buf);

        if written > 0 {
            Poll::Ready(Ok(written))
        } else {
            Poll::Pending
        }
    }
}

const MAX_BUFFER_SIZE: usize = 1024 * 1024 * 10;

impl futures::AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if matches!(self.inner.ready_state(), WebSocket::CLOSING | WebSocket::CLOSED) {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        let remining = MAX_BUFFER_SIZE - self.inner.buffered_amount() as usize;
        if remining == 0 {
            if let Some(cb) = self.state.trottle_callback.take() {
                self.state.trottle_callback.set(Some(cb));
                return Poll::Pending;
            }

            let waker = cx.waker().clone();
            let inner_state = Rc::downgrade(&self.state);
            let cb = Timeout::new(self.trottle_period, move || {
                let Some(state) = inner_state.upgrade() else {
                    return;
                };
                state.trottle_callback.take();
                waker.wake_by_ref();
            });
            self.state.trottle_callback.set(Some(cb));

            return Poll::Pending;
        }

        Poll::Ready(
            self.inner
                .send_with_u8_array(buf)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{e:?}")))
                .map(|()| buf.len()),
        )
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<io::Result<()>> {
        if matches!(self.inner.ready_state(), WebSocket::CLOSING | WebSocket::CLOSED) {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<io::Result<()>> {
        match self.inner.ready_state() {
            WebSocket::CLOSING | WebSocket::CLOSED => Poll::Ready(Ok(())),
            WebSocket::CONNECTING | WebSocket::OPEN => {
                self.inner.close().unwrap();
                self.state.state_waker.set(Some(cx.waker().clone()));
                Poll::Pending
            }
            _ => unreachable!(),
        }
    }
}

unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}

impl Drop for Connection {
    fn drop(&mut self) {
        self.inner.set_onopen(None);
        self.inner.set_onclose(None);
        self.inner.set_onerror(None);
        self.inner.set_onmessage(None);
        if matches!(self.inner.ready_state(), WebSocket::OPEN | WebSocket::CONNECTING) {
            self.inner.close().unwrap();
        }
    }
}

pub struct ConnectionFut(Option<Connection>);

impl Future for ConnectionFut {
    type Output = Result<Connection, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let s = &mut *self;
        let Some(conn) = s.0.take() else {
            return Poll::Pending;
        };

        match conn.inner.ready_state() {
            WebSocket::OPEN => Poll::Ready(Ok(conn)),
            WebSocket::CLOSING | WebSocket::CLOSED => {
                Poll::Ready(Err(Error(JsValue::from_str("connection aborted"))))
            }
            WebSocket::CONNECTING => {
                conn.state.state_waker.set(Some(cx.waker().clone()));
                s.0 = Some(conn);
                Poll::Pending
            }
            _ => unreachable!(),
        }
    }
}
