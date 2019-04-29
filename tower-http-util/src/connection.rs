//! Contains all Http Connection utilities.

use futures::{Future, Poll};
use http::Version;
use std::net::SocketAddr;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tcp::TcpStream;
use tower_service::Service;

/// A Http aware connection creator.
pub trait HttpMakeConnection<Target>: sealed::Sealed<Target> {
    /// The transport provided by this service that is HTTP aware.
    type Connection: HttpConnection + AsyncRead + AsyncWrite;

    /// Errors produced by the connecting service
    type Error;

    /// The future that eventually produces the transport
    type Future: Future<Item = Self::Connection, Error = Self::Error>;

    /// Returns `Ready` when it is able to make more connections.
    fn poll_ready(&mut self) -> Poll<(), Self::Error>;

    /// Connect and return a transport asynchronously
    fn make_connection(&mut self, target: Target) -> Self::Future;
}

/// Represents a HTTP aware connection.
pub trait HttpConnection {
    /// Returns the version that this stream is set too.
    fn version(&self) -> Version;

    /// Returns the remote address that this connection is connected to.
    fn remote_addr(&self) -> std::io::Result<SocketAddr>;
}

impl HttpConnection for TcpStream {
    fn version(&self) -> Version {
        Version::default()
    }

    fn remote_addr(&self) -> std::io::Result<SocketAddr> {
        self.peer_addr()
    }
}

impl<C, Target> sealed::Sealed<Target> for C where C: Service<Target> {}

impl<C, Target> HttpMakeConnection<Target> for C
where
    C: Service<Target>,
    C::Response: HttpConnection + AsyncRead + AsyncWrite,
{
    type Connection = C::Response;
    type Error = C::Error;
    type Future = C::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Service::poll_ready(self)
    }

    fn make_connection(&mut self, target: Target) -> Self::Future {
        Service::call(self, target)
    }
}

mod sealed {
    pub trait Sealed<Target> {}
}
