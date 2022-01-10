use std::io;

use socket2::{Domain, Protocol, SockAddr, Type};
use std::net::SocketAddr;
use tokio::io::unix::AsyncFd;

use super::mio;

pub struct Socket {
    socket: AsyncFd<mio::Socket>,
}

impl Socket {
    pub fn new(domain: Domain, type_: Type, protocol: Protocol) -> io::Result<Self> {
        let socket = mio::Socket::new(domain, type_, protocol)?;
        let socket = AsyncFd::new(socket)?;
        Ok(Self { socket })
    }

    pub async fn send_to<T>(&self, buf: T, target: SocketAddr) -> io::Result<()>
    where
        T: AsRef<[u8]>,
    {
        let n = loop {
            let mut guard = self.socket.writable().await?;
            match guard.try_io(|inner| {
                inner
                    .get_ref()
                    .send_to(buf.as_ref(), &SockAddr::from(target))
            }) {
                Ok(res) => break res?,
                Err(_would_block) => continue,
            }
        };
        if n != buf.as_ref().len() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "failed to send entire packet",
            ));
        }
        Ok(())
    }

    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize, io::Error> {
        loop {
            let mut guard = self.socket.readable().await?;
            match guard.try_io(|inner| inner.get_ref().recv(buf)) {
                Ok(res) => return res,
                Err(_would_block) => continue,
            }
        }
    }
}
