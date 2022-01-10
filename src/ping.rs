use std::io;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use futures::stream::Stream;
use parking_lot::Mutex;
use rand::random;
use socket2::{Domain, Protocol, Type};
use tracing::{debug, error, trace};

use tokio::time;

use crate::packet::{EchoReply, EchoRequest, IcmpV4, IcmpV6, ICMP_HEADER_SIZE};
use crate::packet::{IpV4Packet, IpV4Protocol};
use crate::socket::Socket;
use crate::{errors, Error};

const DEFAULT_TIMEOUT: u64 = 2;
const TOKEN_SIZE: usize = 24;
const ECHO_REQUEST_BUFFER_SIZE: usize = ICMP_HEADER_SIZE + TOKEN_SIZE;
type Token = [u8; TOKEN_SIZE];
type EchoRequestBuffer = [u8; ECHO_REQUEST_BUFFER_SIZE];

#[derive(Clone)]
struct PingState {
    inner: Arc<Mutex<HashMap<Token, oneshot::Sender<Instant>>>>,
}

impl PingState {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn insert(&self, key: Token, value: oneshot::Sender<Instant>) {
        self.inner.lock().insert(key, value);
    }

    fn remove(&self, key: &[u8]) -> Option<oneshot::Sender<Instant>> {
        self.inner.lock().remove(key)
    }
}

/// Ping the same host several times.
pub struct PingChain {
    pinger: Pinger,
    hostname: IpAddr,
    ident: Option<u16>,
    seq_cnt: Option<u16>,
    timeout: Option<Duration>,
}

impl PingChain {
    fn new(pinger: Pinger, hostname: IpAddr) -> Self {
        Self {
            pinger,
            hostname,
            ident: None,
            seq_cnt: None,
            timeout: None,
        }
    }

    /// Set ICMP ident. Default value is randomized.
    pub fn ident(mut self, ident: u16) -> Self {
        self.ident = Some(ident);
        self
    }

    /// Set ICMP seq_cnt, this value will be incremented by one for every `send`.
    /// Default value is 0.
    pub fn seq_cnt(mut self, seq_cnt: u16) -> Self {
        self.seq_cnt = Some(seq_cnt);
        self
    }

    /// Set ping timeout. Default timeout is two seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Send ICMP request and wait for response.
    pub async fn send(&mut self) -> Result<Option<Duration>, errors::Error> {
        let ident = match self.ident {
            Some(ident) => ident,
            None => {
                let ident = random();
                self.ident = Some(ident);
                ident
            }
        };

        let seq_cnt = match self.seq_cnt {
            Some(seq_cnt) => {
                self.seq_cnt = Some(seq_cnt.wrapping_add(1));
                seq_cnt
            }
            None => {
                self.seq_cnt = Some(1);
                0
            }
        };

        let timeout = match self.timeout {
            Some(timeout) => timeout,
            None => {
                let timeout = Duration::from_secs(DEFAULT_TIMEOUT);
                self.timeout = Some(timeout);
                timeout
            }
        };

        self.pinger
            .ping(self.hostname, ident, seq_cnt, timeout)
            .await
    }

    pub fn stream(mut self) -> impl Stream<Item = Result<Option<Duration>, Error>> {
        async_stream::try_stream! {
            loop {
                let resp = self.send().await?;
                yield resp;
            }
        }
    }
}

/// ICMP packets sender and receiver.
#[derive(Clone)]
pub struct Pinger {
    inner: Arc<PingInner>,
}

struct PingInner {
    sockets: Sockets,
    state: PingState,
    _v4_finalize: Option<oneshot::Sender<()>>,
    _v6_finalize: Option<oneshot::Sender<()>>,
}

enum Sockets {
    V4(Arc<Socket>),
    V6(Arc<Socket>),
    Both { v4: Arc<Socket>, v6: Arc<Socket> },
}

impl Sockets {
    fn new() -> io::Result<Self> {
        let mb_v4socket = Socket::new(Domain::IPV4, Type::DGRAM, Protocol::ICMPV4);
        let mb_v6socket = Socket::new(Domain::IPV6, Type::DGRAM, Protocol::ICMPV6);
        match (mb_v4socket, mb_v6socket) {
            (Ok(v4_socket), Ok(v6_socket)) => Ok(Sockets::Both {
                v4: Arc::new(v4_socket),
                v6: Arc::new(v6_socket),
            }),
            (Ok(v4_socket), Err(_)) => Ok(Sockets::V4(Arc::new(v4_socket))),
            (Err(_), Ok(v6_socket)) => Ok(Sockets::V6(Arc::new(v6_socket))),
            (Err(err), Err(_)) => Err(err),
        }
    }

    fn v4(&self) -> Option<Arc<Socket>> {
        match *self {
            Sockets::V4(ref socket) => Some(socket.clone()),
            Sockets::Both { ref v4, .. } => Some(v4.clone()),
            Sockets::V6(_) => None,
        }
    }

    fn v6(&self) -> Option<Arc<Socket>> {
        match *self {
            Sockets::V4(_) => None,
            Sockets::Both { ref v6, .. } => Some(v6.clone()),
            Sockets::V6(ref socket) => Some(socket.clone()),
        }
    }
}

impl Pinger {
    /// Create new `Pinger` instance, will fail if unable to create both IPv4 and IPv6 sockets.
    pub fn new() -> Result<Self, Error> {
        let sockets = Sockets::new()?;

        let state = PingState::new();
        let v4_finalize = if let Some(v4_socket) = sockets.v4() {
            let (s, r) = oneshot::channel();
            let receiver = Receiver::<IcmpV4>::new(v4_socket, state.clone());
            tokio::spawn(async move {
                tokio::select! {
                    _ = receiver.recv() => {}
                    _ = r => {}
                }
            });
            Some(s)
        } else {
            None
        };

        let v6_finalize = if let Some(v6_socket) = sockets.v6() {
            let (s, r) = oneshot::channel();
            let receiver = Receiver::<IcmpV6>::new(v6_socket.clone(), state.clone());
            tokio::spawn(async move {
                tokio::select! {
                    _ = receiver.recv() => {}
                    _ = r => {}
                }
            });

            Some(s)
        } else {
            None
        };

        let inner = PingInner {
            sockets,
            state,
            _v4_finalize: v4_finalize,
            _v6_finalize: v6_finalize,
        };

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Ping the same host several times.
    pub fn chain(&self, hostname: IpAddr) -> PingChain {
        PingChain::new(self.clone(), hostname)
    }

    /// Send ICMP request and wait for response.
    pub async fn ping(
        &self,
        hostname: IpAddr,
        ident: u16,
        seq_cnt: u16,
        timeout: Duration,
    ) -> Result<Option<Duration>, errors::Error> {
        let (sender, receiver) = oneshot::channel();

        let deadline = Instant::now() + timeout;

        let token = random();
        self.inner.state.insert(token, sender);

        let dest = SocketAddr::new(hostname, 0);
        let mut buffer: EchoRequestBuffer = [0; ECHO_REQUEST_BUFFER_SIZE];

        let request = EchoRequest {
            ident,
            seq_cnt,
            payload: &token,
        };

        let (encode_result, mb_socket) = {
            if dest.is_ipv4() {
                (
                    request.encode::<IcmpV4>(&mut buffer[..]),
                    self.inner.sockets.v4(),
                )
            } else {
                (
                    request.encode::<IcmpV6>(&mut buffer[..]),
                    self.inner.sockets.v6(),
                )
            }
        };

        let socket = match mb_socket {
            Some(socket) => socket,
            None => {
                return Err(errors::Error::InvalidProtocol);
            }
        };

        if let Err(err) = encode_result {
            error!(?err);
            return Err(errors::Error::InternalError);
        }
        let start_time = Instant::now();
        let state = self.inner.state.clone();

        socket.send_to(buffer, dest).await?;
        tokio::select! {
             _ = time::sleep_until(deadline.into()) => {
                state.remove(&token);
                Ok(None)
            },
            r = receiver => {
                state.remove(&token);
                let stop_time = r.map_err(|err| {
                    error!(?err);
                    errors::Error::InternalError
                })?;
                Ok(Some(stop_time - start_time))
            }
        }
    }
}

struct Receiver<M> {
    socket: Arc<Socket>,
    state: PingState,
    _phantom: ::std::marker::PhantomData<M>,
}

trait ParseReply {
    fn reply_payload(data: &[u8]) -> Option<&[u8]>;
}

impl ParseReply for IcmpV4 {
    fn reply_payload(data: &[u8]) -> Option<&[u8]> {
        if let Ok(ipv4_packet) = IpV4Packet::decode(data) {
            if ipv4_packet.protocol != IpV4Protocol::Icmp {
                return None;
            }

            if let Ok(reply) = EchoReply::decode::<IcmpV4>(ipv4_packet.data) {
                return Some(reply.payload);
            }
        }
        None
    }
}

impl ParseReply for IcmpV6 {
    fn reply_payload(data: &[u8]) -> Option<&[u8]> {
        if let Ok(reply) = EchoReply::decode::<IcmpV6>(data) {
            return Some(reply.payload);
        }
        None
    }
}

impl<M: ParseReply> Receiver<M> {
    fn new(socket: Arc<Socket>, state: PingState) -> Self {
        Self {
            socket,
            state,
            _phantom: ::std::marker::PhantomData,
        }
    }

    async fn recv(&self) -> io::Result<()> {
        let mut buf = [0; 2048];
        let (now, payload) = loop {
            let n = self.socket.recv(&mut buf).await?;
            if let Some(payload) = M::reply_payload(&buf[..n]) {
                let now = Instant::now();
                break (now, payload);
            }
        };
        if let Some(sender) = self.state.remove(payload) {
            sender.send(now).unwrap_or_default();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[tokio::test]
    #[traced_test]
    async fn test_pinger() {
        let pinger = Pinger::new().unwrap();
        pinger
            .ping(
                "127.0.0.1".parse().unwrap(),
                rand::random(),
                0,
                Duration::from_secs(1),
            )
            .await
            .unwrap();
    }
}
