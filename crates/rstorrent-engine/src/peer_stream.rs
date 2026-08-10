//! Concrete peer byte-stream boundary shared by TCP and uTP.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::peer_runtime::PeerTransport;
use crate::utp_runtime::UtpStream;

#[derive(Debug)]
pub(crate) enum PeerStream {
    Tcp(TcpStream),
    Utp(Box<UtpStream>),
}

impl PeerStream {
    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Self::Tcp(stream) => stream.local_addr(),
            Self::Utp(stream) => Ok(stream.local_addr()),
        }
    }

    pub(crate) fn peer_addr(&self) -> io::Result<SocketAddr> {
        match self {
            Self::Tcp(stream) => stream.peer_addr(),
            Self::Utp(stream) => Ok(stream.peer_addr()),
        }
    }

    pub(crate) const fn transport(&self) -> PeerTransport {
        match self {
            Self::Tcp(_) => PeerTransport::Tcp,
            Self::Utp(_) => PeerTransport::Utp,
        }
    }

    pub(crate) async fn readable(&self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.readable().await,
            Self::Utp(stream) => stream.readable().await,
        }
    }

    pub(crate) async fn writable(&self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.writable().await,
            Self::Utp(stream) => stream.writable().await,
        }
    }

    pub(crate) fn try_read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.try_read(bytes),
            Self::Utp(stream) => stream.try_read(bytes),
        }
    }

    pub(crate) fn try_write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.try_write(bytes),
            Self::Utp(stream) => stream.try_write(bytes),
        }
    }
}

impl From<TcpStream> for PeerStream {
    fn from(stream: TcpStream) -> Self {
        Self::Tcp(stream)
    }
}

impl From<UtpStream> for PeerStream {
    fn from(stream: UtpStream) -> Self {
        Self::Utp(Box::new(stream))
    }
}

impl AsyncRead for PeerStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        destination: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(stream) => Pin::new(stream).poll_read(context, destination),
            Self::Utp(stream) => Pin::new(stream.as_mut()).poll_read(context, destination),
        }
    }
}

impl AsyncWrite for PeerStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.get_mut() {
            Self::Tcp(stream) => Pin::new(stream).poll_write(context, bytes),
            Self::Utp(stream) => Pin::new(stream.as_mut()).poll_write(context, bytes),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Self::Tcp(stream) => Pin::new(stream).poll_flush(context),
            Self::Utp(stream) => Pin::new(stream.as_mut()).poll_flush(context),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match self.get_mut() {
            Self::Tcp(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Utp(stream) => Pin::new(stream.as_mut()).poll_shutdown(context),
        }
    }
}
