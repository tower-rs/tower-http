//! Set a header on the response.
//!
//! # Example
//!
//! ```
//! use http::{Request, Response, header::{HeaderName, HeaderValue}};
//! use std::convert::Infallible;
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower_http::set_response_header::SetResponseHeaderLayer;
//! use http_body::Body as _; // for `Body::size_hint`
//! use hyper::Body;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let echo_service = tower::service_fn(|request: Request<Body>| async move {
//!     Ok::<_, Infallible>(Response::new(request.into_body()))
//! });
//!
//! let mut svc = ServiceBuilder::new()
//!     .layer(
//!         // Layer that sets `Content-Type: text/html` on responses.
//!         //
//!         // We have to add `::<_, Body>` since Rust cannot infer the body type when
//!         // we don't use a closure to produce the header value.
//!         SetResponseHeaderLayer::<_, Body>::new(
//!             HeaderName::from_static("content-type"),
//!             HeaderValue::from_static("text/html"),
//!         )
//!     )
//!     .layer(
//!         // Layer that sets `Content-Length` if the body has a known size.
//!         // Bodies with streaming responses wont have a known size.
//!         SetResponseHeaderLayer::new(
//!             HeaderName::from_static("content-length"),
//!             |response: &Response<Body>| {
//!                 if let Some(size) = response.body().size_hint().exact() {
//!                     Some(HeaderValue::from_str(&size.to_string()).unwrap())
//!                 } else {
//!                     None
//!                 }
//!             }
//!         )
//!     )
//!     .service(echo_service);
//!
//! let request = Request::new(Body::from("<strong>Hello, World</strong>"));
//!
//! let response = svc.ready_and().await?.call(request).await?;
//!
//! assert_eq!(response.headers()["content-type"], "text/html");
//! assert_eq!(response.headers()["content-length"], "29");
//! #
//! # Ok(())
//! # }

use futures_util::ready;
use http::{header::HeaderName, HeaderValue, Request, Response};
use pin_project::pin_project;
use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};
use std::{future::Future, marker::PhantomData};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`SetResponseHeader`] which adds a response header.
///
/// See [`SetResponseHeader`] for more details.
pub struct SetResponseHeaderLayer<M, B> {
    header_name: HeaderName,
    make: M,
    override_existing: bool,
    _marker: PhantomData<fn() -> B>,
}

impl<M, B> fmt::Debug for SetResponseHeaderLayer<M, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetResponseHeaderLayer")
            .field("header_name", &self.header_name)
            .field("override_existing", &self.override_existing)
            .field("make", &std::any::type_name::<M>())
            .finish()
    }
}

impl<M, B> SetResponseHeaderLayer<M, B> {
    /// Create a new [`SetResponseHeaderLayer`].
    pub fn new(header_name: HeaderName, make: M) -> Self
    where
        M: MakeHeaderValue<B>,
    {
        Self {
            make,
            header_name,
            override_existing: true,
            _marker: PhantomData,
        }
    }

    /// Should the header be overriden if the response already contains it?
    ///
    /// Defaults to `true`.
    pub fn override_existing(mut self, override_existing: bool) -> Self {
        self.override_existing = override_existing;
        self
    }
}

impl<B, S, M> Layer<S> for SetResponseHeaderLayer<M, B>
where
    M: MakeHeaderValue<B> + Clone,
{
    type Service = SetResponseHeader<S, M>;

    fn layer(&self, inner: S) -> Self::Service {
        SetResponseHeader {
            inner,
            header_name: self.header_name.clone(),
            make: self.make.clone(),
            override_existing: self.override_existing,
        }
    }
}

impl<M, B> Clone for SetResponseHeaderLayer<M, B>
where
    M: Clone,
{
    fn clone(&self) -> Self {
        Self {
            make: self.make.clone(),
            header_name: self.header_name.clone(),
            override_existing: self.override_existing,
            _marker: PhantomData,
        }
    }
}

/// Middleware that sets a header on the response.
#[derive(Clone)]
pub struct SetResponseHeader<S, M> {
    inner: S,
    header_name: HeaderName,
    make: M,
    override_existing: bool,
}

impl<S, M> SetResponseHeader<S, M> {
    /// Create a new [`SetResponseHeader`].
    pub fn new(inner: S, header_name: HeaderName, make: M) -> Self {
        Self {
            inner,
            header_name,
            make,
            override_existing: true,
        }
    }

    /// Should the header be overriden if the response already contains it?
    ///
    /// Defaults to `true`.
    pub fn override_existing(mut self, override_existing: bool) -> Self {
        self.override_existing = override_existing;
        self
    }
}

impl<S, M> fmt::Debug for SetResponseHeader<S, M>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetResponseHeaderLayer")
            .field("inner", &self.inner)
            .field("header_name", &self.header_name)
            .field("override_existing", &self.override_existing)
            .field("make", &std::any::type_name::<M>())
            .finish()
    }
}

impl<ReqBody, ResBody, S, M> Service<Request<ReqBody>> for SetResponseHeader<S, M>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    M: MakeHeaderValue<ResBody> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        ResponseFuture {
            future: self.inner.call(req),
            header_name: self.header_name.clone(),
            make: self.make.clone(),
            override_existing: self.override_existing,
        }
    }
}

/// Response future for [`SetResponseHeader`].
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F, M> {
    #[pin]
    future: F,
    header_name: HeaderName,
    make: M,
    override_existing: bool,
}

impl<F, ResBody, E, M> Future for ResponseFuture<F, M>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    M: MakeHeaderValue<ResBody>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        if let Some(value) = this.make.make_header_value(&res) {
            if res.headers().contains_key(&*this.header_name) {
                if *this.override_existing {
                    res.headers_mut().insert(this.header_name.clone(), value);
                }
            } else {
                res.headers_mut().insert(this.header_name.clone(), value);
            }
        }

        Poll::Ready(Ok(res))
    }
}

/// Trait for producing header values from responses.
///
/// Used by [`SetResponseHeader`].
///
/// You shouldn't normally have to implement this trait since its implemented for closures with the
/// correct type.
///
/// It is also implemented directly for `HeaderValue` so if you just want to add a fixed value you
/// can suply one directly to [`SetResponseHeaderLayer`].
pub trait MakeHeaderValue<B> {
    /// Try to create a header value from the response.
    fn make_header_value(&mut self, response: &Response<B>) -> Option<HeaderValue>;
}

impl<F, B> MakeHeaderValue<B> for F
where
    F: FnMut(&Response<B>) -> Option<HeaderValue>,
{
    fn make_header_value(&mut self, response: &Response<B>) -> Option<HeaderValue> {
        self(response)
    }
}

impl<B> MakeHeaderValue<B> for HeaderValue {
    fn make_header_value(&mut self, _response: &Response<B>) -> Option<HeaderValue> {
        Some(self.clone())
    }
}
