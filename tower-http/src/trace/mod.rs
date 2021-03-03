//! Middleware that adds high level [tracing] to a [`Service`].
//!
//! # Example
//!
//! Adding tracing to your service can be as simple as:
//!
//! ```rust
//! use http::{Request, Response};
//! use hyper::Body;
//! use tower::{ServiceBuilder, service_fn, ServiceExt, Service};
//! use tower_http::trace::TraceLayer;
//! use std::convert::Infallible;
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, Infallible> {
//!     Ok(Response::new(Body::from("foo")))
//! }
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Setup tracing
//! tracing_subscriber::fmt::init();
//!
//! let mut service = ServiceBuilder::new()
//!     .layer(TraceLayer::new_for_http())
//!     .service(service_fn(handle));
//!
//! let request = Request::new(Body::from("foo"));
//!
//! let response = service
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! If you run this application with `RUST_LOG=tower_http=trace cargo run` you should see logs like:
//!
//! ```text
//! Mar 05 20:50:28.523 DEBUG request{method=GET path="/foo"}: tower_http::trace::on_request: started processing request
//! Mar 05 20:50:28.524 DEBUG request{method=GET path="/foo"}: tower_http::trace::on_response: finished processing request latency=1 ms status=200
//! ```
//!
//! TODO(david): Document these things
//! - gRPC support
//! - Setting classifiers
//! - Customizing what to do on request, response, eos, failure
//!
//! [tracing]: https://crates.io/crates/tracing
//! [`Service`]: tower_service::Service

// TODO(david): Document all the things
#![allow(missing_docs)]

use tracing::Level;

pub use self::{
    body::ResponseBody,
    future::ResponseFuture,
    layer::TraceLayer,
    make_span::{DefaultMakeSpan, MakeSpan},
    on_eos::{DefaultOnEos, OnEos},
    on_failure::{DefaultOnFailure, OnFailure},
    on_request::{DefaultOnRequest, OnRequest},
    on_response::{DefaultOnResponse, OnResponse},
    service::Trace,
};

mod body;
mod future;
mod layer;
mod make_span;
mod on_eos;
mod on_failure;
mod on_request;
mod on_response;
mod service;

const DEFAULT_MESSAGE_LEVEL: Level = Level::DEBUG;
const DEFAULT_ERROR_LEVEL: Level = Level::ERROR;
