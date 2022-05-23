//! Imposes a length limit on request bodies.
//!
//! This layer will also intercept requests with a `Content-Length` header
//! larger than the allowable limit and return an immediate error before
//! reading any of the body. The response returned will request that the
//! connection be reset to prevent request smuggling attempts.
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
//! Note: In order to prevent request smuggling, it is important to reset
//! the connection using a `Connection: close` header.
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
//!                 resp.headers_mut().insert(
//!                     http::header::CONNECTION,
//!                     http::HeaderValue::from_static("close"),
//!                 );
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

mod body;
mod future;
mod layer;
mod service;

pub use body::ResponseBody;
pub use future::ResponseFuture;
pub use layer::RequestBodyLimitLayer;
pub use service::RequestBodyLimit;
