//! Contains all Http Connection utilities.
//!
//! This module provides a `HttpMakeConnection`, this trait provides a
//! HTTP aware connection. This is for use with libraries like `tower-hyper`.

use http_connection::HttpConnection;
use std::future::Future;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};
use tower_service::Service;

/// A Http aware connection creator.
///
/// This type is a trait alias that produces `HttpConnection` aware
/// connections.
pub trait HttpMakeConnection<Target>: sealed::Sealed<Target> {
    /// The transport provided by this service that is HTTP aware.
    type Connection: HttpConnection + AsyncRead + AsyncWrite;

    /// Errors produced by the connecting service
    type Error;

    /// The future that eventually produces the transport
    type Future: Future<Output = Result<Self::Connection, Self::Error>>;

    /// Returns `Ready` when it is able to make more connections.
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;

    /// Connect and return a transport asynchronously
    fn make_connection(&mut self, target: Target) -> Self::Future;
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

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Service::poll_ready(self, cx)
    }

    fn make_connection(&mut self, target: Target) -> Self::Future {
        Service::call(self, target)
    }
}

mod sealed {
    pub trait Sealed<Target> {}
}
