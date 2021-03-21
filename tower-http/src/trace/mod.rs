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
    on_body_chunk::{DefaultOnBodyChunk, OnBodyChunk},
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
mod on_body_chunk;
mod on_eos;
mod on_failure;
mod on_request;
mod on_response;
mod service;

const DEFAULT_MESSAGE_LEVEL: Level = Level::DEBUG;
const DEFAULT_ERROR_LEVEL: Level = Level::ERROR;

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::{HeaderMap, Request, Response, StatusCode};
    use hyper::Body;
    use once_cell::sync::Lazy;
    use std::{
        sync::atomic::{AtomicU32, Ordering},
        time::Duration,
    };
    use tower::{BoxError, Service, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn unary_request() {
        static ON_REQUEST_COUNT: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));
        static ON_RESPONSE_COUNT: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));
        static ON_BODY_CHUNK_COUNT: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));
        static ON_EOS: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));
        static ON_FAILURE: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));

        let trace_layer = TraceLayer::<_, BoxError>::new_for_http()
            .on_request(|_req: &Request<Body>| {
                ON_REQUEST_COUNT.fetch_add(1, Ordering::SeqCst);
            })
            .on_response(|_res: &Response<Body>, _latency: Duration| {
                ON_RESPONSE_COUNT.fetch_add(1, Ordering::SeqCst);
            })
            .on_body_chunk(|_chunk: &Bytes, _latency: Duration| {
                ON_BODY_CHUNK_COUNT.fetch_add(1, Ordering::SeqCst);
            })
            .on_eos(|_trailers: Option<&HeaderMap>, _latency: Duration| {
                ON_EOS.fetch_add(1, Ordering::SeqCst);
            })
            .on_failure(|_err: StatusCode, _latency: Duration| {
                ON_FAILURE.fetch_add(1, Ordering::SeqCst);
            });

        let mut svc = ServiceBuilder::new().layer(trace_layer).service_fn(echo);

        let res = svc
            .ready()
            .await
            .unwrap()
            .call(Request::new(Body::from("foobar")))
            .await
            .unwrap();

        assert_eq!(1, ON_REQUEST_COUNT.load(Ordering::SeqCst), "request");
        assert_eq!(1, ON_RESPONSE_COUNT.load(Ordering::SeqCst), "request");
        assert_eq!(0, ON_BODY_CHUNK_COUNT.load(Ordering::SeqCst), "body chunk");
        assert_eq!(0, ON_EOS.load(Ordering::SeqCst), "eos");
        assert_eq!(0, ON_FAILURE.load(Ordering::SeqCst), "failure");

        hyper::body::to_bytes(res.into_body()).await.unwrap();
        assert_eq!(1, ON_BODY_CHUNK_COUNT.load(Ordering::SeqCst), "body chunk");
        assert_eq!(0, ON_EOS.load(Ordering::SeqCst), "eos");
        assert_eq!(0, ON_FAILURE.load(Ordering::SeqCst), "failure");
    }

    #[tokio::test]
    async fn streaming_response() {
        static ON_REQUEST_COUNT: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));
        static ON_RESPONSE_COUNT: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));
        static ON_BODY_CHUNK_COUNT: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));
        static ON_EOS: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));
        static ON_FAILURE: Lazy<AtomicU32> = Lazy::new(|| AtomicU32::new(0));

        let trace_layer = TraceLayer::<_, BoxError>::new_for_http()
            .on_request(|_req: &Request<Body>| {
                ON_REQUEST_COUNT.fetch_add(1, Ordering::SeqCst);
            })
            .on_response(|_res: &Response<Body>, _latency: Duration| {
                ON_RESPONSE_COUNT.fetch_add(1, Ordering::SeqCst);
            })
            .on_body_chunk(|_chunk: &Bytes, _latency: Duration| {
                ON_BODY_CHUNK_COUNT.fetch_add(1, Ordering::SeqCst);
            })
            .on_eos(|_trailers: Option<&HeaderMap>, _latency: Duration| {
                ON_EOS.fetch_add(1, Ordering::SeqCst);
            })
            .on_failure(|_err: StatusCode, _latency: Duration| {
                ON_FAILURE.fetch_add(1, Ordering::SeqCst);
            });

        let mut svc = ServiceBuilder::new()
            .layer(trace_layer)
            .service_fn(streaming_body);

        let res = svc
            .ready()
            .await
            .unwrap()
            .call(Request::new(Body::empty()))
            .await
            .unwrap();

        assert_eq!(1, ON_REQUEST_COUNT.load(Ordering::SeqCst), "request");
        assert_eq!(1, ON_RESPONSE_COUNT.load(Ordering::SeqCst), "request");
        assert_eq!(0, ON_BODY_CHUNK_COUNT.load(Ordering::SeqCst), "body chunk");
        assert_eq!(0, ON_EOS.load(Ordering::SeqCst), "eos");
        assert_eq!(0, ON_FAILURE.load(Ordering::SeqCst), "failure");

        hyper::body::to_bytes(res.into_body()).await.unwrap();
        assert_eq!(3, ON_BODY_CHUNK_COUNT.load(Ordering::SeqCst), "body chunk");
        assert_eq!(0, ON_EOS.load(Ordering::SeqCst), "eos");
        assert_eq!(0, ON_FAILURE.load(Ordering::SeqCst), "failure");
    }

    async fn echo(req: Request<Body>) -> Result<Response<Body>, BoxError> {
        Ok(Response::new(req.into_body()))
    }

    async fn streaming_body(_req: Request<Body>) -> Result<Response<Body>, BoxError> {
        use futures::stream::iter;

        let stream = iter(vec![
            Ok::<_, BoxError>(Bytes::from("one")),
            Ok::<_, BoxError>(Bytes::from("two")),
            Ok::<_, BoxError>(Bytes::from("three")),
        ]);

        let body = Body::wrap_stream(stream);

        Ok(Response::new(body))
    }
}
