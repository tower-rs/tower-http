//! Imposes a length limit on request bodies.
//!
//! This layer will also intercept requests with a `Content-Length` header
//! larger than the allowable limit and return an immediate error before
//! reading any of the body.
//!
//! Note that payload length errors can be used by adversaries to attempt to
//! smuggle requests. When an incoming stream is dropped due to an over-sized
//! payload, servers should close the connection or resynchronize by
//! optimistically consuming some data in an attempt to reach the end of the
//! current HTTP frame. If the incoming stream cannot be resynchronized,
//! then the connection should be closed.
//!
//! # Examples
//!
//! If the `Content-Length` header indicates a payload that is larger than
//! the acceptable limit, then the response will be rejected whether or not
//! the body is read.
//!
//! ```rust
//! use bytes::Bytes;
//! use std::convert::Infallible;
//! use http::{Request, Response, StatusCode};
//! use http_body::{Limited, LengthLimitError};
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower_http::limit::RequestBodyLimitLayer;
//! use hyper::Body;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! async fn handle(req: Request<Limited<Body>>) -> Result<Response<Body>, Infallible> {
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
//! will return an error. This error should be checked to determine if
//! it is a
//!
//! Note that if the body is never read, or never attempts to consume the
//! body beyond the length limit, then no error will be generated.
//!
//! ```rust
//! # use bytes::Bytes;
//! # use std::convert::Infallible;
//! # use http::{Request, Response, StatusCode};
//! # use http_body::{Limited, LengthLimitError};
//! # use tower::{Service, ServiceExt, ServiceBuilder, BoxError};
//! # use tower_http::limit::RequestBodyLimitLayer;
//! # use hyper::Body;
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! async fn handle(req: Request<Limited<Body>>) -> Result<Response<Body>, BoxError> {
//!     match hyper::body::to_bytes(req.into_body()).await {
//!         Ok(data) => Ok(Response::new(Body::empty())),
//!         Err(err) => {
//!             if let Some(_) = tower_http::limit::try_as_length_limit_error(&*err) {
//!                 let mut resp = Response::new(Body::empty());
//!                 *resp.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
//!                 Ok(resp)
//!             } else {
//!                 Err(err)
//!             }
//!         }
//!     }
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
//! # use tower::{Service, ServiceExt, ServiceBuilder};
//! # use tower_http::limit::RequestBodyLimitLayer;
//! # use http_body::{Limited, LengthLimitError};
//! # use hyper::Body;
//! # use std::convert::Infallible;
//! use tower_http::map_request_body::MapRequestBodyLayer;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! async fn handle(req: Request<Limited<Body>>) -> Result<Response<Body>, Infallible> {
//!     let data = hyper::body::to_bytes(req.into_body()).await;
//!     let resp = match data {
//!         Ok(data) => Response::new(Body::from(data)),
//!         Err(err) => {
//!             if let Some(_) = tower_http::limit::try_as_length_limit_error(&*err) {
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

mod body;
mod future;
mod layer;
mod service;

pub use body::ResponseBody;
pub use future::ResponseFuture;
pub use layer::RequestBodyLimitLayer;
pub use service::RequestBodyLimit;

use http_body::LengthLimitError;
use std::error::Error as StdError;

/// Identifies whether a given error is caused by a length limit error.
pub fn try_as_length_limit_error<'err>(
    err: &'err (dyn StdError + 'static),
) -> Option<&'err LengthLimitError> {
    let mut source = Some(err);
    while let Some(err) = source {
        if let Some(lle) = err.downcast_ref::<LengthLimitError>() {
            return Some(lle);
        }
        source = err.source();
    }
    None
}
