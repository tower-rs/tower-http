//! Middlewares that mark headers as [sensitive].
//!
//! [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
//!
//! # Example
//!
//! ```
//! use tower_http::sensitive_header::SetSensitiveHeaderLayer;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use http::{Request, Response, header::AUTHORIZATION};
//! use hyper::Body;
//! use std::convert::Infallible;
//!
//! async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
//!     // ...
//!     # Ok(Response::new(Body::empty()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut service = ServiceBuilder::new()
//!     // Mark the `Authorization` header as sensitive so it doesn't show in logs
//!     //
//!     // `SetSensitiveHeaderLayer` will mark the header as sensitive on both the
//!     // request and response.
//!     .layer(SetSensitiveHeaderLayer::new(AUTHORIZATION))
//!     .service(service_fn(handle));
//!
//! // Call the service.
//! let response = service
//!     .ready()
//!     .await?
//!     .call(Request::new(Body::empty()))
//!     .await?;
//! # Ok(())
//! # }
//! ```

use futures_util::ready;
use http::{header::HeaderName, Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Mark a header as [sensitive] on both requests and responses.
///
/// Produces [`SetSensitiveHeader`] services.
///
/// See the [module docs](crate::sensitive_header) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveHeaderLayer<I> {
    headers: I,
}

impl<I> SetSensitiveHeaderLayer<I> {
    /// Create a new [`SetSensitiveHeaderLayer`].
    pub fn new(headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName> + Clone,
    {
        Self { headers }
    }
}

impl<S, I> Layer<S> for SetSensitiveHeaderLayer<I>
where
    I: IntoIterator<Item = HeaderName> + Clone,
{
    type Service = SetSensitiveHeader<S, I>;

    fn layer(&self, inner: S) -> Self::Service {
        SetSensitiveRequestHeader::new(
            SetSensitiveResponseHeader::new(inner, self.headers.clone()),
            self.headers.clone(),
        )
    }
}

/// Mark a header as [sensitive] on both requests and responses.
///
/// See the [module docs](crate::sensitive_header) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
pub type SetSensitiveHeader<S, I> = SetSensitiveRequestHeader<SetSensitiveResponseHeader<S, I>, I>;

/// Mark a request header as [sensitive].
///
/// Produces [`SetSensitiveRequestHeader`] services.
///
/// See the [module docs](crate::sensitive_header) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveRequestHeaderLayer<I> {
    headers: I,
}

impl<I> SetSensitiveRequestHeaderLayer<I> {
    /// Create a new [`SetSensitiveRequestHeaderLayer`].
    pub fn new(headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        Self { headers }
    }
}

impl<S, I> Layer<S> for SetSensitiveRequestHeaderLayer<I>
where
    I: Clone,
{
    type Service = SetSensitiveRequestHeader<S, I>;

    fn layer(&self, inner: S) -> Self::Service {
        SetSensitiveRequestHeader {
            inner,
            headers: self.headers.clone(),
        }
    }
}

/// Mark a request header as [sensitive].
///
/// See the [module docs](crate::sensitive_header) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveRequestHeader<S, I> {
    inner: S,
    headers: I,
}

impl<S, I> SetSensitiveRequestHeader<S, I> {
    /// Create a new [`SetSensitiveRequestHeader`] service.
    pub fn new(inner: S, headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        Self { inner, headers }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `SetSensitiveRequestHeader` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(headers: I) -> SetSensitiveRequestHeaderLayer<I>
    where
        I: IntoIterator<Item = HeaderName>,
    {
        SetSensitiveRequestHeaderLayer::new(headers)
    }
}

impl<ReqBody, ResBody, S, I> Service<Request<ReqBody>> for SetSensitiveRequestHeader<S, I>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    I: IntoIterator<Item = HeaderName> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        for header in self.headers.clone() {
            if let Some(value) = req.headers_mut().get_mut(&header) {
                value.set_sensitive(true);
            }
        }

        self.inner.call(req)
    }
}

/// Mark a response header as [sensitive].
///
/// Produces [`SetSensitiveResponseHeader`] services.
///
/// See the [module docs](crate::sensitive_header) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveResponseHeaderLayer<I> {
    headers: I,
}

impl<I> SetSensitiveResponseHeaderLayer<I> {
    /// Create a new [`SetSensitiveResponseHeaderLayer`].
    pub fn new(headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        Self { headers }
    }
}

impl<S, I> Layer<S> for SetSensitiveResponseHeaderLayer<I>
where
    I: Clone,
{
    type Service = SetSensitiveResponseHeader<S, I>;

    fn layer(&self, inner: S) -> Self::Service {
        SetSensitiveResponseHeader {
            inner,
            headers: self.headers.clone(),
        }
    }
}

/// Mark a response header as [sensitive].
///
/// See the [module docs](crate::sensitive_header) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveResponseHeader<S, I> {
    inner: S,
    headers: I,
}

impl<S, I> SetSensitiveResponseHeader<S, I> {
    /// Create a new [`SetSensitiveResponseHeader`] service.
    pub fn new(inner: S, headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        Self { inner, headers }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `SetSensitiveResponseHeader` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(headers: I) -> SetSensitiveResponseHeaderLayer<I>
    where
        I: IntoIterator<Item = HeaderName>,
    {
        SetSensitiveResponseHeaderLayer::new(headers)
    }
}

impl<ReqBody, ResBody, S, I> Service<Request<ReqBody>> for SetSensitiveResponseHeader<S, I>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    I: IntoIterator<Item = HeaderName> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = SetSensitiveResponseHeaderResponseFuture<S::Future, I>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        SetSensitiveResponseHeaderResponseFuture {
            future: self.inner.call(req),
            headers: Some(self.headers.clone()),
        }
    }
}

/// Response future for [`SetSensitiveResponseHeader`].
#[pin_project]
#[derive(Debug)]
pub struct SetSensitiveResponseHeaderResponseFuture<F, I> {
    #[pin]
    future: F,
    headers: Option<I>,
}

impl<F, ResBody, I, E> Future for SetSensitiveResponseHeaderResponseFuture<F, I>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    I: IntoIterator<Item = HeaderName>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        let headers = this.headers.take().unwrap();
        for header in headers.into_iter() {
            if let Some(value) = res.headers_mut().get_mut(&header) {
                value.set_sensitive(true);
            }
        }

        Poll::Ready(Ok(res))
    }
}
