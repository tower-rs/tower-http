//! Contains all Http Connection utilities.
//!
//! This module provides a `HttpMakeConnection` and a `HttpConnection` trait. These traits
//! decorate an `AsyncRead + AsyncWrite` and a `MakeConnection` that provides HTTP aware
//! connections.

use futures::{Future, Poll};
use http::Version;
use std::net::SocketAddr;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tcp::TcpStream;
use tower_service::Service;

/// A Http aware connection creator.
///
/// This type is a trait alias that produces `HttpConnection` aware
/// connections.
pub trait HttpMakeConnection<Target>: sealed::Sealed<Target> {
    /// The transport provided by this service that is HTTP aware.
    type Connection: HttpConnection;

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
///
/// This connection is a `AsyncRead + AsyncWrite` stream that provides information
/// on what http versions were determinted `ALPN` negotiation or what the remote address
/// this stream is connected too.
pub trait HttpConnection: AsyncRead + AsyncWrite {
    /// Returns the version that this stream is set too.
    ///
    /// For `version` this indicates that this stream is accepting http frames of the version
    /// returned. If `None` is returned then there has been no prior negotiation for the http
    /// version.
    fn version(&self) -> Option<Version>;

    /// Returns the remote address that this connection is connected to.
    fn remote_addr(&self) -> Option<SocketAddr>;
}

impl HttpConnection for TcpStream {
    fn version(&self) -> Option<Version> {
        None
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        self.peer_addr().ok()
    }
}

impl<C, Target> sealed::Sealed<Target> for C where C: Service<Target> {}

impl<C, Target> HttpMakeConnection<Target> for C
where
    C: Service<Target>,
    C::Response: HttpConnection,
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
