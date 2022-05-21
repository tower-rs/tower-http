//! Imposes a length limit on request bodies.
//!
//! # Example
//!
//! ```rust
//! use bytes::Bytes;
//! use http::{Request, Response, StatusCode};
//! use tower::{Service, ServiceExt, ServiceBuilder, BoxError};
//! use tower_http::limit::RequestBodyLimitLayer;
//! use http_body::{Limited, LengthLimitError};
//! use hyper::Body;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! async fn handle(req: Request<Limited<Body>>) -> Result<Response<Body>, BoxError> {
//!     hyper::body::to_bytes(req.into_body()).await?;
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! let mut svc = ServiceBuilder::new()
//!     // Limit incoming requests to 4096 bytes.
//!     .layer(RequestBodyLimitLayer::new(4096))
//!     .service_fn(handle);
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
//!
//! Using a custom error type:
//!
//! ```rust
//! use bytes::Bytes;
//! use http::{Request, Response, StatusCode};
//! use tower::{Service, ServiceExt, ServiceBuilder, BoxError};
//! use tower_http::limit::RequestBodyLimitLayer;
//! use http_body::{Limited, LengthLimitError};
//! use hyper::Body;
//!
//! #[derive(Debug)]
//! enum MyError {
//!     MySpecificError,
//!     Unknown(BoxError),
//! }
//!
//! impl std::fmt::Display for MyError {
//!     // ...
//! #    fn fmt(&self, _: &mut std::fmt::Formatter) -> std::fmt::Result {
//! #        Ok(())
//! #    }
//! }
//!
//! impl std::error::Error for MyError {
//!     fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
//!         match self {
//!             Self::Unknown(err) => Some(&**err),
//!             Self::MySpecificError => None,
//!         }
//!     }
//! }
//!
//! impl From<BoxError> for MyError {
//!     fn from(err: BoxError) -> Self {
//!         Self::Unknown(err)
//!     }
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! async fn handle(req: Request<Limited<Body>>) -> Result<Response<Body>, MyError> {
//!     hyper::body::to_bytes(req.into_body()).await?;
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! let mut svc = ServiceBuilder::new()
//!     // Limit incoming requests to 4096 bytes.
//!     .layer(RequestBodyLimitLayer::new(4096))
//!     .service_fn(handle);
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
use http_body::{combinators::UnsyncBoxBody, Body, Full, LengthLimitError, Limited};
use pin_project_lite::pin_project;
use std::{
    any,
    error::Error as StdError,
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies the [`LengthLimit`] middleware that intercepts requests
/// with body lengths greater than the configured limit and converts them into
/// `413 Payload Too Large` responses.
///
/// See the [module docs](self) for an example.
pub struct RequestBodyLimitLayer {
    limit: usize,
}

impl RequestBodyLimitLayer {
    /// Create a new `RequestBodyLimitLayer` with the given body length limit.
    pub fn new(limit: usize) -> Self {
        Self { limit }
    }
}

impl Clone for RequestBodyLimitLayer {
    fn clone(&self) -> Self {
        Self { limit: self.limit }
    }
}

impl Copy for RequestBodyLimitLayer {}

impl fmt::Debug for RequestBodyLimitLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestBodyLimitLayer")
            .field("limit", &self.limit)
            .finish()
    }
}

impl<S> Layer<S> for RequestBodyLimitLayer {
    type Service = RequestBodyLimit<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestBodyLimit {
            inner,
            limit: self.limit,
        }
    }
}

/// Middleware that intercepts requests with body lengths greater than the
/// configured limit and converts them into `413 Payload Too Large` responses.
///
/// See the [module docs](self) for an example.
pub struct RequestBodyLimit<S> {
    inner: S,
    limit: usize,
}

impl<S> RequestBodyLimit<S> {
    define_inner_service_accessors!();

    /// Create a new `RequestBodyLimit` with the given body length limit.
    pub fn new(inner: S, limit: usize) -> Self {
        Self { inner, limit }
    }
}

impl<S> Clone for RequestBodyLimit<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            limit: self.limit,
        }
    }
}

impl<S> Copy for RequestBodyLimit<S> where S: Copy {}

impl<S> fmt::Debug for RequestBodyLimit<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestBodyLimit")
            .field("service", &format_args!("{}", any::type_name::<S>()))
            .field("limit", &self.limit)
            .finish()
    }
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for RequestBodyLimit<S>
where
    S: Service<Request<Limited<ReqBody>>, Response = Response<ResBody>>,
    S::Error: Into<BoxError>,
    ResBody: Body<Data = Bytes> + Send + 'static,
{
    type Response = Response<UnsyncBoxBody<Bytes, ResBody::Error>>;
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
{
    type Output = Result<Response<UnsyncBoxBody<Bytes, ResBody::Error>>, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().future.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(data)) => {
                let (parts, body) = data.into_parts();
                let body = body.boxed_unsync();
                let resp = Response::from_parts(parts, body);

                Poll::Ready(Ok(resp))
            }
            Poll::Ready(Err(err)) => {
                let err = err.into();
                let mut source = Some(&*err as &(dyn StdError + 'static));
                while let Some(err) = source {
                    if let Some(_) = err.downcast_ref::<LengthLimitError>() {
                        let mut res = Response::new(
                            Full::from("length limit exceeded")
                                .map_err(|err| match err {})
                                .boxed_unsync(),
                        );
                        *res.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;

                        #[allow(clippy::declare_interior_mutable_const)]
                        const TEXT_PLAIN: HeaderValue =
                            HeaderValue::from_static("text/plain; charset=utf-8");
                        res.headers_mut()
                            .insert(http::header::CONTENT_TYPE, TEXT_PLAIN);

                        return Poll::Ready(Ok(res));
                    }
                    source = err.source();
                }
                Poll::Ready(Err(err))
            }
        }
    }
}
