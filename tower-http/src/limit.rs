//! Imposes a length limit on request bodies.
//!
//! # Example
//!
//! ```rust
//! use bytes::Bytes;
//! use http::{Request, Response, StatusCode};
//! use std::convert::Infallible;
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower_http::limit::RequestBodyLimitLayer;
//! use hyper::Body;
//! use http_body::Limited;
//! use tower_http::BoxError;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//!  async fn handle(req: Request<Limited<Body>>) -> Result<Response<Body>, BoxError>
//! {
//!     hyper::body::to_bytes(req.into_body()).await?;
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! let mut svc = ServiceBuilder::new()
//!     // Limit incoming requests to 4096 bytes.
//!     .layer(RequestBodyLimitLayer::new(4096))
//!     .service_fn(handle);
//!
//! fn test_svc<S: Service<Request<Body>>>(s: &S) {}
//! test_svc(&svc);
//!
//! // Call the service.
//! let request = Request::new(Body::empty());
//!
//! let response = svc.ready().await?.call(request).await?;
//!
//! assert_eq!(response.status(), 200);
//!
//! // Call the service with a body that is too large.
//! let request = Request::new(Body::from(Bytes::from(vec![0u8; 4097])));
//!
//! let response = svc.ready().await?.call(request).await?;
//!
//! assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
//!
//! #
//! # Ok(())
//! # }
//! ```

use crate::BoxError;
use bytes::Bytes;
use http::{HeaderValue, Request, Response, StatusCode};
use http_body::combinators::UnsyncBoxBody;
use http_body::{Body, Full, LengthLimitError, Limited};
use pin_project_lite::pin_project;
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::{
    any, fmt,
    marker::PhantomData,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies the [`LengthLimit`] middleware that intercepts requests
/// with body lengths greater than the configured limit and converts them into
/// `413 Payload Too Large` responses.
///
/// See the [module docs](self) for an example.
pub struct RequestBodyLimitLayer<B> {
    limit: usize,
    _ty: PhantomData<fn() -> B>,
}

impl<B> RequestBodyLimitLayer<B> {
    /// Create a new `RequestBodyLimitLayer` with the given body length limit.
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            _ty: PhantomData,
        }
    }
}

impl<B> Clone for RequestBodyLimitLayer<B> {
    fn clone(&self) -> Self {
        Self {
            limit: self.limit,
            _ty: PhantomData,
        }
    }
}

impl<B> Copy for RequestBodyLimitLayer<B> {}

impl<B> fmt::Debug for RequestBodyLimitLayer<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestBodyLimitLayer")
            .field("body", &any::type_name::<B>())
            .field("limit", &self.limit)
            .finish()
    }
}

impl<B, S> Layer<S> for RequestBodyLimitLayer<B> {
    type Service = RequestBodyLimit<S, B>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestBodyLimit {
            inner,
            limit: self.limit,
            _ty: PhantomData,
        }
    }
}

/// Middleware that intercepts requests with body lengths greater than the
/// configured limit and converts them into `413 Payload Too Large` responses.
///
/// See the [module docs](self) for an example.
pub struct RequestBodyLimit<S, B> {
    inner: S,
    limit: usize,
    _ty: PhantomData<fn() -> B>,
}

impl<S, B> RequestBodyLimit<S, B> {
    define_inner_service_accessors!();

    /// Create a new `RequestBodyLimit` with the given body length limit.
    pub fn new(inner: S, limit: usize) -> Self {
        Self {
            inner,
            limit,
            _ty: PhantomData,
        }
    }
}

impl<S, B> Clone for RequestBodyLimit<S, B>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            limit: self.limit,
            _ty: PhantomData,
        }
    }
}

impl<S, B> Copy for LengthLimited<S, B> where S: Copy {}

impl<S, B> fmt::Debug for LengthLimited<S, B>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LengthLimited")
            .field("inner", &self.inner)
            .field("service", &format_args!("{}", any::type_name::<B>()))
            .finish()
    }
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for LengthLimited<S, ReqBody>
where
    S: Service<Request<Limited<ReqBody>>, Response = Response<ResBody>>,
    S::Error: Into<BoxError>,
    ResBody: Body<Data = Bytes> + Send + 'static,
    ResBody::Error: Into<BoxError>,
{
    type Response = Response<UnsyncBoxBody<Bytes, BoxError>>;
    type Error = BoxError;
    type Future = ResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(|e| e.into())
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let (parts, body) = req.into_parts();
        let body = Limited::new(body, self.limit);
        let req = Request::from_parts(parts, body);

        ResponseFuture {
            future: self.inner.call(req),
        }
    }
}

pin_project! {
    /// Response future for [`LengthLimit`].
    pub struct ResponseFuture<F> {
        #[pin]
        future: F,
    }
}

impl<F, ResBody, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    E: Into<BoxError>,
    ResBody: Body<Data = Bytes> + Send + 'static,
    ResBody::Error: Into<BoxError>,
{
    type Output = Result<Response<UnsyncBoxBody<Bytes, BoxError>>, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().future.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(data)) => {
                let (parts, body) = data.into_parts();
                let body = body.map_err(|err| err.into()).boxed_unsync();
                let resp = Response::from_parts(parts, body);

                Poll::Ready(Ok(resp))
            }
            Poll::Ready(Err(err)) => {
                let err = err.into();
                if let Some(_) = err.downcast_ref::<LengthLimitError>() {
                    let mut res = Response::new(
                        Full::from("length limit exceeded")
                            .map_err(|err| err.into())
                            .boxed_unsync(),
                    );
                    *res.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;

                    #[allow(clippy::declare_interior_mutable_const)]
                    const TEXT_PLAIN: HeaderValue =
                        HeaderValue::from_static("text/plain; charset=utf-8");
                    res.headers_mut()
                        .insert(http::header::CONTENT_TYPE, TEXT_PLAIN);

                    Poll::Ready(Ok(res))
                } else {
                    Poll::Ready(Err(err))
                }
            }
        }
    }
}
