//! Middleware that applies a timeout to requests.
//!
//! If the request does not complete within the specified timeout, it will be aborted and a
//! response with an empty body and a custom status code will be returned.
//!
//! # Differences from `tower::timeout`
//!
//! tower's [`Timeout`](tower::timeout::Timeout) middleware uses an error to signal timeout, i.e.
//! it changes the error type to [`BoxError`](tower::BoxError). For HTTP services that is rarely
//! what you want as returning errors will terminate the connection without sending a response.
//!
//! This middleware won't change the error type and instead returns a response with an empty body
//! and the specified status code. That means if your service's error type is [`Infallible`], it will
//! still be [`Infallible`] after applying this middleware.
//!
//! # Body timeouts
//!
//! Two body timeout wrappers are available for limiting how long a request or response body
//! transfer can take:
//!
//! - [`TimeoutBody`] resets its deadline each time a frame is received. Use this to detect
//!   idle connections where no data flows for a period of time.
//! - [`AbsoluteTimeoutBody`] starts a single timer at construction and never resets it. Use
//!   this to cap the total wall-clock time spent transferring a body, regardless of how
//!   frequently data arrives.
//!
//! Both are applied via their corresponding layer types ([`RequestBodyTimeoutLayer`] /
//! [`RequestBodyAbsoluteTimeoutLayer`] for request bodies, [`ResponseBodyTimeoutLayer`] /
//! [`ResponseBodyAbsoluteTimeoutLayer`] for response bodies). They can be stacked if you
//! want both an idle timeout and an absolute deadline.
//!
//! # Example
//!
//! ```
//! use http::{Request, Response, StatusCode};
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use std::{convert::Infallible, time::Duration};
//! use tower::ServiceBuilder;
//! use tower_http::timeout::TimeoutLayer;
//!
//! async fn handle(_: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, Infallible> {
//!     // ...
//!     # Ok(Response::new(Full::default()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let svc = ServiceBuilder::new()
//!     // Timeout requests after 30 seconds with the specified status code
//!     .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(30)))
//!     .service_fn(handle);
//! # Ok(())
//! # }
//! ```
//!
//! [`Infallible`]: std::convert::Infallible

mod absolute_body;
mod body;
mod service;

pub use absolute_body::AbsoluteTimeoutBody;
pub use body::{TimeoutBody, TimeoutError};
pub use service::{
    RequestBodyAbsoluteTimeout, RequestBodyAbsoluteTimeoutLayer, RequestBodyTimeout,
    RequestBodyTimeoutLayer, ResponseBodyAbsoluteTimeout, ResponseBodyAbsoluteTimeoutLayer,
    ResponseBodyTimeout, ResponseBodyTimeoutLayer, Timeout, TimeoutLayer,
};
