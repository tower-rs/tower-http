//! Set a header on the response.
//!
//! The header value to be set may be provided as a fixed value when the
//! middleware is constructed, or determined dynamically based on the response
//! by a closure. See the [`MakeHeaderValue`] trait for details.
//!
//! # Example
//!
//! Setting a header from a fixed value provided when the middleware is constructed:
//!
//! ```
//! use http::{Request, Response, header::{self, HeaderValue}};
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower_http::set_response_header::{SetResponseHeaderLayer, InsertHeaderMode};
//! use hyper::Body;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let render_html = tower::service_fn(|request: Request<Body>| async move {
//! #     Ok::<_, std::convert::Infallible>(Response::new(request.into_body()))
//! # });
//! #
//! let mut svc = ServiceBuilder::new()
//!     .layer(
//!         // Layer that sets `Content-Type: text/html` on responses.
//!         //
//!         // We have to add `::<_, Body>` since Rust cannot infer the body type when
//!         // we don't use a closure to produce the header value.
//!         SetResponseHeaderLayer::<_, Body>::new(
//!             header::CONTENT_TYPE,
//!             HeaderValue::from_static("text/html"),
//!         )
//!         // Don't insert the header if it is already present.
//!         .mode(InsertHeaderMode::SkipIfPresent)
//!     )
//!     .service(render_html);
//!
//! let request = Request::new(Body::empty());
//!
//! let response = svc.ready_and().await?.call(request).await?;
//!
//! assert_eq!(response.headers()["content-type"], "text/html");
//! #
//! # Ok(())
//! # }
//! ```
//!
//! Setting a header based on a value determined dynamically from the response:
//!
//! ```
//! use http::{Request, Response, header::{self, HeaderValue}};
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower_http::set_response_header::SetResponseHeaderLayer;
//! use hyper::Body;
//! use http_body::Body as _; // for `Body::size_hint`
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let render_html = tower::service_fn(|request: Request<Body>| async move {
//! #     Ok::<_, std::convert::Infallible>(Response::new(Body::from("1234567890")))
//! # });
//! #
//! let mut svc = ServiceBuilder::new()
//!     .layer(
//!         // Layer that sets `Content-Length` if the body has a known size.
//!         // Bodies with streaming responses wont have a known size.
//!         SetResponseHeaderLayer::new(
//!             http::header::CONTENT_LENGTH,
//!             |response: &Response<Body>| {
//!                 if let Some(size) = response.body().size_hint().exact() {
//!                     // If the response body has a known size, returning `Some` will
//!                     // set the `Content-Length` header to that value.
//!                     Some(HeaderValue::from_str(&size.to_string()).unwrap())
//!                 } else {
//!                     // If the response body doesn't have a known size, return `None`
//!                     // to skip setting the header on this response.
//!                     None
//!                 }
//!             }
//!         )
//!     )
//!     .service(render_html);
//!
//! let request = Request::new(Body::empty());
//!
//! let response = svc.ready_and().await?.call(request).await?;
//!
//! assert_eq!(response.headers()["content-length"], "10");
//! #
//! # Ok(())
//! # }
//! ```

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
    mode: InsertHeaderMode,
    _marker: PhantomData<fn() -> B>,
}

impl<M, B> fmt::Debug for SetResponseHeaderLayer<M, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetResponseHeaderLayer")
            .field("header_name", &self.header_name)
            .field("mode", &self.mode)
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
            mode: InsertHeaderMode::OverrideExisting,
            _marker: PhantomData,
        }
    }

    /// Set which mode to use when inserting the header.
    ///
    /// Defaults to [`InsertHeaderMode::OverrideExisting`].
    pub fn mode(mut self, mode: InsertHeaderMode) -> Self {
        self.mode = mode;
        self
    }
}

/// The mode to use when inserting a header into a request or response.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum InsertHeaderMode {
    /// Insert the header, overriding any previous values the header might have.
    OverrideExisting,
    /// Append the header and keep any previous values.
    Append,
    /// Insert the header only if it is not already present.
    SkipIfPresent,
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
            mode: self.mode.clone(),
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
            mode: self.mode.clone(),
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
    mode: InsertHeaderMode,
}

impl<S, M> SetResponseHeader<S, M> {
    /// Create a new [`SetResponseHeader`].
    pub fn new(inner: S, header_name: HeaderName, make: M) -> Self {
        Self {
            inner,
            header_name,
            make,
            mode: InsertHeaderMode::OverrideExisting,
        }
    }

    /// Set which mode to use when inserting the header.
    ///
    /// Defaults to [`InsertHeaderMode::OverrideExisting`].
    pub fn mode(mut self, mode: InsertHeaderMode) -> Self {
        self.mode = mode;
        self
    }
}

impl<S, M> fmt::Debug for SetResponseHeader<S, M>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetResponseHeader")
            .field("inner", &self.inner)
            .field("header_name", &self.header_name)
            .field("mode", &self.mode)
            .field("make", &std::any::type_name::<M>())
            .finish()
    }
}

impl<ReqBody, ResBody, S, M> Service<Request<ReqBody>> for SetResponseHeader<S, M>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    M: MakeHeaderValue<Response<ResBody>> + Clone,
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
            mode: self.mode.clone(),
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
    mode: InsertHeaderMode,
}

impl<F, ResBody, E, M> Future for ResponseFuture<F, M>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    M: MakeHeaderValue<Response<ResBody>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        match *this.mode {
            InsertHeaderMode::OverrideExisting => {
                if let Some(value) = this.make.make_header_value(&res) {
                    res.headers_mut().insert(this.header_name.clone(), value);
                }
            }
            InsertHeaderMode::SkipIfPresent => {
                if !res.headers().contains_key(&*this.header_name) {
                    if let Some(value) = this.make.make_header_value(&res) {
                        res.headers_mut().insert(this.header_name.clone(), value);
                    }
                }
            }
            InsertHeaderMode::Append => {
                if let Some(value) = this.make.make_header_value(&res) {
                    res.headers_mut().insert(this.header_name.clone(), value);
                }
            }
        }

        Poll::Ready(Ok(res))
    }
}

/// Trait for producing header values.
///
/// Used by [`SetResponseHeader`].
///
/// This trait is implemented for closures with the correct type signature. Typically
/// users will not have to implement this trait for their own types.
///
/// It is also implemented directly for [`HeaderValue`]. When a fixed header value
/// should be added to all responses, it can be  supplied directly to
/// [`SetResponseHeaderLayer`].
pub trait MakeHeaderValue<T> {
    /// Try to create a header value from the request or response.
    fn make_header_value(&mut self, message: &T) -> Option<HeaderValue>;
}

impl<F, T> MakeHeaderValue<T> for F
where
    F: FnMut(&T) -> Option<HeaderValue>,
{
    fn make_header_value(&mut self, message: &T) -> Option<HeaderValue> {
        self(message)
    }
}

impl<T> MakeHeaderValue<T> for HeaderValue {
    fn make_header_value(&mut self, _message: &T) -> Option<HeaderValue> {
        Some(self.clone())
    }
}

impl<T> MakeHeaderValue<T> for Option<HeaderValue> {
    fn make_header_value(&mut self, _message: &T) -> Option<HeaderValue> {
        self.clone()
    }
}
