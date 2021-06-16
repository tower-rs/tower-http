//! Middleware that adds timeouts to request bodies.
//!
//! Be careful using this with streaming requests as it might abort a stream
//! earlier than othwerwise intended.
//!
//! # Example
//!
//! ```
//! use tower_http::timeout::RequestBodyTimeoutLayer;
//! use tower::BoxError;
//! use hyper::{Request, Response, Body, Error};
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use std::time::Duration;
//! use bytes::Bytes;
//!
//! async fn handle<B>(request: Request<B>) -> Result<Response<Body>, BoxError>
//! where
//!     B: http_body::Body,
//!     B::Error: Into<BoxError>,
//! {
//!     // Buffer the whole request body. The timeout will be automatically
//!     // applied by `RequestBodyTimeoutLayer`.
//!     let body_bytes = hyper::body::to_bytes(request.into_body())
//!         .await
//!         .map_err(Into::into)?;
//!
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! let mut service = ServiceBuilder::new()
//!     // Make sure the request body completes with 100 milliseconds.
//!     .layer(RequestBodyTimeoutLayer::new(Duration::from_millis(100)))
//!     .service_fn(handle);
//!
//! // Create a response body with a channel that we can use to send data
//! // asynchronously.
//! let (mut tx, body) = hyper::Body::channel();
//!
//! tokio::spawn(async move {
//!     // Keep sending data forever. This would make the server hang if we
//!     // didn't use `RequestBodyTimeoutLayer`.
//!     loop {
//!         tokio::time::sleep(Duration::from_secs(1)).await;
//!         if tx.send_data(Bytes::from("foo")).await.is_err() {
//!             break;
//!         }
//!     }
//! });
//!
//! let req = Request::new(body);
//!
//! // Calling the service should fail with a timeout error since buffering the
//! // request body hits the timeout.
//! let err = service.ready().await?.call(req).await.unwrap_err();
//! assert!(err.is::<tower::timeout::error::Elapsed>());
//! # Ok(())
//! # }
//! ```

use super::TimeoutBody;
use http::Request;
use std::{
    task::{Context, Poll},
    time::Duration,
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`RequestBodyTimeoutLayer`] which adds a timeout to the
/// request body.
///
/// If receiving the request body doesn't complete within the specified time, an
/// error is returned.
///
/// See the [module docs](crate::timeout::request_body) for an example.
#[derive(Debug, Copy, Clone)]
pub struct RequestBodyTimeoutLayer {
    timeout: Duration,
}

impl RequestBodyTimeoutLayer {
    /// Create a new `RequestBodyTimeoutLayer`.
    pub fn new(timeout: Duration) -> Self {
        RequestBodyTimeoutLayer { timeout }
    }
}

impl<S> Layer<S> for RequestBodyTimeoutLayer {
    type Service = RequestBodyTimeout<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestBodyTimeout::new(inner, self.timeout)
    }
}

/// Middleware that adds a timeout to the request bodies.
///
/// If receiving the request body doesn't complete within the specified time, an
/// error is returned.
///
/// See the [module docs](crate::timeout::request_body) for an example.
#[derive(Debug, Copy, Clone)]
pub struct RequestBodyTimeout<S> {
    inner: S,
    timeout: Duration,
}

impl<S> RequestBodyTimeout<S> {
    /// Create a new `RequestBodyTimeout`.
    pub fn new(inner: S, timeout: Duration) -> Self {
        Self { inner, timeout }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a
    /// [`RequestBodyTimeoutLayer`]h middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(timeout: Duration) -> RequestBodyTimeoutLayer {
        RequestBodyTimeoutLayer::new(timeout)
    }
}

impl<S, ReqBody> Service<Request<ReqBody>> for RequestBodyTimeout<S>
where
    S: Service<Request<TimeoutBody<ReqBody>>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.inner
            .call(req.map(|body| TimeoutBody::new(body, self.timeout)))
    }
}
