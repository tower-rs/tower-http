//! Imposes a length limit on request bodies.
//!
//! This layer will also intercept requests with a `Content-Length` header
//! larger than the allowable limit and return an immediate error before
//! reading any of the body.
//!
//! Handling of any unread payload beyond the length limit depends on the
//! underlying server implementation.
//!
//! # Examples
//!
//! If the `Content-Length` header indicates a payload that is larger than
//! the acceptable limit, then the response will be rejected whether or not
//! the body is read.
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
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! let mut svc = ServiceBuilder::new()
//!     // Limit incoming requests to 4096 bytes.
//!     .layer(RequestBodyLimitLayer::new(4096))
//!     .service_fn(handle);
//!
//! // Call the service with a header that indicates the body is too large.
//! let mut request = Request::new(Body::empty());
//! request.headers_mut().insert(
//!     http::header::CONTENT_LENGTH,
//!     http::HeaderValue::from_static("5000"),
//! );
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
//! If no `Content-Length` header is present, then the body will be read
//! until the length limit has been reached. If it is reached, the body
//! will return an error. If this error is bubbled up, then this layer
//! will return an appropriate `413 Payload Too Large` response.
//!
//! Note that if the body is never read, or never attempts to consume the
//! body beyond the length limit, then no error will be generated.
//!
//! ```rust
//! # use bytes::Bytes;
//! # use http::{Request, Response, StatusCode};
//! # use tower::{Service, ServiceExt, ServiceBuilder, BoxError};
//! # use tower_http::limit::RequestBodyLimitLayer;
//! # use http_body::{Limited, LengthLimitError};
//! # use hyper::Body;
//! #
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
//! This automatic error response mechanism will also work if the error
//! returned by the body is available in the source chain.
//!
//! ```rust
//! # use bytes::Bytes;
//! # use http::{Request, Response, StatusCode};
//! # use tower::{Service, ServiceExt, ServiceBuilder, BoxError};
//! # use tower_http::limit::RequestBodyLimitLayer;
//! # use http_body::{Limited, LengthLimitError};
//! # use hyper::Body;
//! #
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
//!
//! If the automatic `413 Payload Too Large` response and handling
//! of `Content-Length` headers is not desired, consider directly using
//! [`MapRequestBody`] to wrap the request body with [`http_body::Limited`].
//!
//! [`MapRequestBody`]: crate::map_request_body
//!
//! ```rust
//! # use bytes::Bytes;
//! # use http::{Request, Response, StatusCode};
//! # use tower::{Service, ServiceExt, ServiceBuilder, BoxError};
//! # use tower_http::limit::RequestBodyLimitLayer;
//! # use http_body::{Limited, LengthLimitError};
//! # use hyper::Body;
//! # use std::convert::Infallible;
//! use tower_http::map_request_body::MapRequestBodyLayer;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! async fn handle(req: Request<Limited<Body>>) -> Result<Response<Body>, Infallible> {
//!     let data = hyper::body::to_bytes(req.into_body()).await;
//!     let resp = match data {
//!         Ok(data) => Response::new(Body::from(data)),
//!         Err(err) => {
//!             if err.downcast_ref::<LengthLimitError>().is_some() {
//!                 let body = Body::from("Whoa there! Too much data! Teapot mode!");
//!                 let mut resp = Response::new(body);
//!                 *resp.status_mut() = StatusCode::IM_A_TEAPOT;
//!                 resp
//!             } else {
//!                 let mut resp = Response::new(Body::from(err.to_string()));
//!                 *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
//!                 resp
//!             }
//!         }
//!     };
//!     Ok(resp)
//! }
//!
//! let mut svc = ServiceBuilder::new()
//!     // Limit incoming requests to 4096 bytes, but no automatic response.
//!     .layer(MapRequestBodyLayer::new(|b| Limited::new(b, 4096)))
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
//! assert_eq!(response.status(), StatusCode::IM_A_TEAPOT);
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
    future::{ready, Future},
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies the [`RequestBodyLimit`] middleware that intercepts requests
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
    S::Future: 'static,
    ResBody: Body<Data = Bytes> + Send + 'static,
{
    type Response = Response<UnsyncBoxBody<Bytes, ResBody::Error>>;
    type Error = BoxError;
    type Future = ResponseFuture<ResBody, Self::Error>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(|e| e.into())
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let (parts, body) = req.into_parts();
        let content_length = parts
            .headers
            .get(http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok()?.parse::<usize>().ok());

        let future: Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>> =
            match content_length {
                Some(len) if len > self.limit => Box::pin(ready(Ok(create_error_response()))),
                _ => {
                    let body = Limited::new(body, self.limit);
                    let req = Request::from_parts(parts, body);
                    let fut = self.inner.call(req);
                    Box::pin(async move {
                        fut.await
                            .map(|res| {
                                let (parts, body) = res.into_parts();
                                let body = body.boxed_unsync();
                                Response::from_parts(parts, body)
                            })
                            .map_err(|err| err.into())
                    })
                }
            };

        ResponseFuture { future }
    }
}

pin_project! {
    /// Response future for [`RequestBodyLimit`].
    pub struct ResponseFuture<ResBody, E>
    where
        ResBody: Body<Data = Bytes>
    {
        #[pin]
        future: Pin<Box<dyn Future<Output = Result<Response<UnsyncBoxBody<Bytes, ResBody::Error>>, E>>>>,
    }
}

impl<ResBody, E> Future for ResponseFuture<ResBody, E>
where
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
                        return Poll::Ready(Ok(create_error_response()));
                    }
                    source = err.source();
                }
                Poll::Ready(Err(err))
            }
        }
    }
}

fn create_error_response<E>() -> Response<UnsyncBoxBody<Bytes, E>> {
    let mut res = Response::new(
        Full::from("length limit exceeded")
            .map_err(|err| match err {})
            .boxed_unsync(),
    );
    *res.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;

    #[allow(clippy::declare_interior_mutable_const)]
    const TEXT_PLAIN: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");
    res.headers_mut()
        .insert(http::header::CONTENT_TYPE, TEXT_PLAIN);

    res
}
