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
//! use tower_http::set_response_header::SetResponseHeaderLayer;
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
//!         //
//!         // `if_not_present` will only insert the header if it does not already
//!         // have a value.
//!         SetResponseHeaderLayer::<_, Body>::if_not_present(
//!             header::CONTENT_TYPE,
//!             HeaderValue::from_static("text/html"),
//!         )
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
//!         //
//!         // `overriding` will insert the header and override any previous values it
//!         // may have.
//!         SetResponseHeaderLayer::overriding(
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
use http::{header::HeaderName, HeaderValue, Response};
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
pub struct SetResponseHeaderLayer<M, T> {
    header_name: HeaderName,
    make: M,
    mode: InsertHeaderMode,
    _marker: PhantomData<fn() -> T>,
}

impl<M, T> fmt::Debug for SetResponseHeaderLayer<M, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetResponseHeaderLayer")
            .field("header_name", &self.header_name)
            .field("mode", &self.mode)
            .field("make", &std::any::type_name::<M>())
            .finish()
    }
}

impl<M, T> SetResponseHeaderLayer<M, T>
where
    M: MakeHeaderValue<T>,
{
    /// Create a new [`SetResponseHeaderLayer`]. If a previous value exists for the same header, it
    /// is removed and replaced with the new header value.
    pub fn overriding(header_name: HeaderName, make: M) -> Self {
        Self::new(header_name, make, InsertHeaderMode::Override)
    }

    /// Create a new [`SetResponseHeaderLayer`]. The new header is always added, preserving any
    /// existing values. If previous values exist, the header will have multiple values.
    pub fn appending(header_name: HeaderName, make: M) -> Self {
        Self::new(header_name, make, InsertHeaderMode::Append)
    }

    /// Create a new [`SetResponseHeaderLayer`]. If a previous value exists for the header, the new
    /// value is not inserted.
    pub fn if_not_present(header_name: HeaderName, make: M) -> Self {
        Self::new(header_name, make, InsertHeaderMode::IfNotPresent)
    }

    fn new(header_name: HeaderName, make: M, mode: InsertHeaderMode) -> Self
    where
        M: MakeHeaderValue<T>,
    {
        Self {
            make,
            header_name,
            mode,
            _marker: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum InsertHeaderMode {
    Override,
    Append,
    IfNotPresent,
}

impl Default for InsertHeaderMode {
    fn default() -> Self {
        Self::Override
    }
}

impl<T, S, M> Layer<S> for SetResponseHeaderLayer<M, T>
where
    M: MakeHeaderValue<T> + Clone,
{
    type Service = SetResponseHeader<S, M>;

    fn layer(&self, inner: S) -> Self::Service {
        SetResponseHeader {
            inner,
            header_name: self.header_name.clone(),
            make: self.make.clone(),
            mode: self.mode,
        }
    }
}

impl<M, T> Clone for SetResponseHeaderLayer<M, T>
where
    M: Clone,
{
    fn clone(&self) -> Self {
        Self {
            make: self.make.clone(),
            header_name: self.header_name.clone(),
            mode: self.mode,
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
    /// Create a new [`SetResponseHeader`]. If a previous value exists for the same header, it is
    /// removed and replaced with the new header value.
    pub fn overriding(inner: S, header_name: HeaderName, make: M) -> Self {
        Self::new(inner, header_name, make, InsertHeaderMode::Override)
    }

    /// Create a new [`SetResponseHeader`]. The new header is always added, preserving any existing
    /// values. If previous values exist, the header will have multiple values.
    pub fn appending(inner: S, header_name: HeaderName, make: M) -> Self {
        Self::new(inner, header_name, make, InsertHeaderMode::Append)
    }

    /// Create a new [`SetResponseHeader`]. If a previous value exists for the header, the new
    /// value is not inserted.
    pub fn if_not_present(inner: S, header_name: HeaderName, make: M) -> Self {
        Self::new(inner, header_name, make, InsertHeaderMode::IfNotPresent)
    }

    fn new(inner: S, header_name: HeaderName, make: M, mode: InsertHeaderMode) -> Self {
        Self {
            inner,
            header_name,
            make,
            mode,
        }
    }

    define_inner_service_accessors!();
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

impl<Req, ResBody, S, M> Service<Req> for SetResponseHeader<S, M>
where
    S: Service<Req, Response = Response<ResBody>>,
    M: MakeHeaderValue<Response<ResBody>> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        ResponseFuture {
            future: self.inner.call(req),
            header_name: self.header_name.clone(),
            make: self.make.clone(),
            mode: self.mode,
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
            InsertHeaderMode::Override => {
                if let Some(value) = this.make.make_header_value(&res) {
                    res.headers_mut().insert(this.header_name.clone(), value);
                }
            }
            InsertHeaderMode::IfNotPresent => {
                if !res.headers().contains_key(&*this.header_name) {
                    if let Some(value) = this.make.make_header_value(&res) {
                        res.headers_mut().insert(this.header_name.clone(), value);
                    }
                }
            }
            InsertHeaderMode::Append => {
                if let Some(value) = this.make.make_header_value(&res) {
                    res.headers_mut().append(this.header_name.clone(), value);
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

#[cfg(test)]
mod tests {
    use super::*;
    use http::header;
    use hyper::Body;
    use std::convert::Infallible;
    use tower::{service_fn, ServiceExt};

    #[tokio::test]
    async fn test_override_mode() {
        let svc = SetResponseHeader::overriding(
            service_fn(|_req: ()| async {
                let res = Response::builder()
                    .header(header::CONTENT_TYPE, "good-content")
                    .body(Body::empty())
                    .unwrap();
                Ok::<_, Infallible>(res)
            }),
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html"),
        );

        let res = svc.oneshot(()).await.unwrap();

        let mut values = res.headers().get_all(header::CONTENT_TYPE).iter();
        assert_eq!(values.next().unwrap(), "text/html");
        assert_eq!(values.next(), None);
    }

    #[tokio::test]
    async fn test_append_mode() {
        let svc = SetResponseHeader::appending(
            service_fn(|_req: ()| async {
                let res = Response::builder()
                    .header(header::CONTENT_TYPE, "good-content")
                    .body(Body::empty())
                    .unwrap();
                Ok::<_, Infallible>(res)
            }),
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html"),
        );

        let res = svc.oneshot(()).await.unwrap();

        let mut values = res.headers().get_all(header::CONTENT_TYPE).iter();
        assert_eq!(values.next().unwrap(), "good-content");
        assert_eq!(values.next().unwrap(), "text/html");
        assert_eq!(values.next(), None);
    }

    #[tokio::test]
    async fn test_skip_if_present_mode() {
        let svc = SetResponseHeader::if_not_present(
            service_fn(|_req: ()| async {
                let res = Response::builder()
                    .header(header::CONTENT_TYPE, "good-content")
                    .body(Body::empty())
                    .unwrap();
                Ok::<_, Infallible>(res)
            }),
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html"),
        );

        let res = svc.oneshot(()).await.unwrap();

        let mut values = res.headers().get_all(header::CONTENT_TYPE).iter();
        assert_eq!(values.next().unwrap(), "good-content");
        assert_eq!(values.next(), None);
    }

    #[tokio::test]
    async fn test_skip_if_present_mode_when_not_present() {
        let svc = SetResponseHeader::if_not_present(
            service_fn(|_req: ()| async {
                let res = Response::builder().body(Body::empty()).unwrap();
                Ok::<_, Infallible>(res)
            }),
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html"),
        );

        let res = svc.oneshot(()).await.unwrap();

        let mut values = res.headers().get_all(header::CONTENT_TYPE).iter();
        assert_eq!(values.next().unwrap(), "text/html");
        assert_eq!(values.next(), None);
    }
}
