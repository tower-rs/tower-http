//! `async fn(HttpRequest) -> Result<HttpResponse, Error>`
//!
//! # Overview
//!
//! `tower-http` is a library that provides HTTP-specific middlewares and utilities built on top of
//! [`tower`].
//!
//! All middlewares uses the [`http`] and [`http-body`] crates as the HTTP abstractions. That means
//! they're compatible with any library or framework that also uses those crates, such as
//! [`hyper`].
//!
//! # Example server
//!
//! This example shows how to apply middlewares from `tower-http` to a [`Service`] and then run
//! that service using [`hyper`].
//!
//! ```rust,no_run
//! use tower_http::{
//!     add_extension::AddExtensionLayer,
//!     compression::CompressionLayer,
//!     propagate_header::PropagateHeaderLayer,
//!     sensitive_headers::SetSensitiveRequestHeadersLayer,
//!     set_header::SetResponseHeadersLayer,
//!     trace::TraceLayer,
//! };
//! use tower::{ServiceBuilder, service_fn, make::Shared};
//! use http::{Request, Response, header::{HeaderName, CONTENT_TYPE, AUTHORIZATION}};
//! use hyper::{Body, Error, server::Server, service::make_service_fn};
//! use std::{sync::Arc, net::SocketAddr, convert::Infallible, iter::once};
//! # struct DatabaseConnectionPool;
//! # impl DatabaseConnectionPool {
//! #     fn new() -> DatabaseConnectionPool { DatabaseConnectionPool }
//! # }
//! # fn content_length_from_response<B>(_: &http::Response<B>) -> Option<http::HeaderValue> { None }
//!
//! // Our request handler. This is where we would implement the application logic
//! // for responding to HTTP requests...
//! async fn handler(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     // ...
//!     # todo!()
//! }
//!
//! // Shared state across all request handlers --- in this case, a pool of database connections.
//! struct State {
//!     pool: DatabaseConnectionPool,
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // Construct the shared state.
//!     let state = State {
//!         pool: DatabaseConnectionPool::new(),
//!     };
//!
//!     // Use `tower`'s `ServiceBuilder` API to build a stack of `tower` middleware
//!     // wrapping our request handler.
//!     let service = ServiceBuilder::new()
//!         // Mark the `Authorization` request header as sensitive so it doesn't show in logs
//!         .layer(SetSensitiveRequestHeadersLayer::new(once(AUTHORIZATION)))
//!         // High level logging of requests and responses
//!         .layer(TraceLayer::new_for_http())
//!         // Share an `Arc<State>` with all requests
//!         .layer(AddExtensionLayer::new(Arc::new(state)))
//!         // Compress responses
//!         .layer(CompressionLayer::new())
//!         // Propagate `X-Request-Id`s from requests to responses
//!         .layer(PropagateHeaderLayer::new(HeaderName::from_static("x-request-id")))
//!         // If the response has a known size set the `Content-Length` header
//!         .layer(SetResponseHeadersLayer::overriding(CONTENT_TYPE, content_length_from_response))
//!         // Wrap a `Service` in our middleware stack
//!         .service_fn(handler);
//!
//!     // And run our service using `hyper`
//!     let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
//!     Server::bind(&addr)
//!         .serve(Shared::new(service))
//!         .await
//!         .expect("server error");
//! }
//! ```
//!
//! Keep in mind that while this example uses [`hyper`], `tower-http` supports any HTTP
//! client/server implementation that uses the [`http`] and [`http-body`] crates.
//!
//! # Example client
//!
//! `tower-http` middlewares can also be applied to HTTP clients:
//!
//! ```rust,no_run
//! use tower_http::{
//!     decompression::DecompressionLayer,
//!     set_header::SetRequestHeaderLayer,
//! };
//! use tower::{ServiceBuilder, Service, ServiceExt};
//! use hyper::Body;
//! use http::{Request, Response, HeaderValue, header::USER_AGENT};
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut client = ServiceBuilder::new()
//!         // Set a `User-Agent` header on all requests
//!         .layer(SetRequestHeaderLayer::<_, Body>::overriding(
//!             USER_AGENT,
//!             HeaderValue::from_static("tower-http demo")
//!         ))
//!         // Decompress response bodies
//!         .layer(DecompressionLayer::new())
//!         // Wrap a `hyper::Client` in our middleware stack
//!         .service(hyper::Client::new());
//!
//!     // Make a request
//!     let request = Request::builder()
//!         .uri("http://example.com")
//!         .body(Body::empty())
//!         .unwrap();
//!
//!     let response = client
//!         .ready()
//!         .await
//!         .unwrap()
//!         .call(request)
//!         .await
//!         .unwrap();
//! }
//! ```
//!
//! # Feature Flags
//!
//! All middleware are disabled by default and can be enabled using [cargo features].
//!
//! For example, to enable the [`Trace`] middleware, add the "trace" feature flag in
//! your `Cargo.toml`:
//!
//! ```toml
//! tower-http = { version = "0.1.0", features = ["trace"] }
//! ```
//!
//! You can use `"full"` to enable everything:
//!
//! ```toml
//! tower-http = { version = "0.1.0", features = ["full"] }
//! ```
//!
//! # Getting Help
//!
//! First, see if the answer to your question can be found in the API documentation. If the answer
//! is not there, there is an active community in the [Tower Discord channel][chat]. We would be
//! happy to try to answer your question. If that doesn't work, try opening an [issue] with the
//! question.
//!
//! [`tower`]: https://crates.io/crates/tower
//! [`http`]: https://crates.io/crates/http
//! [`http-body`]: https://crates.io/crates/http-body
//! [`hyper`]: https://crates.io/crates/hyper
//! [cargo features]: https://doc.rust-lang.org/cargo/reference/features.html
//! [`AddExtension`]: crate::add_extension::AddExtension
//! [`Service`]: https://docs.rs/tower/latest/tower/trait.Service.html
//! [chat]: https://discord.gg/tokio
//! [issue]: https://github.com/tower-rs/tower-http/issues/new
//! [`Trace`]: crate::trace::Trace

#![doc(html_root_url = "https://docs.rs/tower-http/0.1.0")]
#![warn(
    clippy::all,
    clippy::dbg_macro,
    clippy::todo,
    clippy::empty_enum,
    clippy::enum_glob_use,
    clippy::pub_enum_variant_names,
    clippy::mem_forget,
    clippy::unused_self,
    clippy::filter_map_next,
    clippy::needless_continue,
    clippy::needless_borrow,
    clippy::match_wildcard_for_single_variants,
    clippy::if_let_mutex,
    clippy::mismatched_target_os,
    clippy::await_holding_lock,
    clippy::match_on_vec_items,
    clippy::imprecise_flops,
    clippy::suboptimal_flops,
    clippy::lossy_float_literal,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::fn_params_excessive_bools,
    clippy::exit,
    clippy::inefficient_to_string,
    clippy::linkedlist,
    clippy::macro_use_imports,
    clippy::option_option,
    clippy::verbose_file_reads,
    clippy::unnested_or_patterns,
    rust_2018_idioms,
    future_incompatible,
    nonstandard_style,
    missing_docs
)]
#![deny(unreachable_pub, broken_intra_doc_links, private_in_public)]
#![allow(
    elided_lifetimes_in_paths,
    // TODO: Remove this once the MSRV bumps to 1.42.0 or above.
    clippy::match_like_matches_macro,
    clippy::type_complexity
)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(test, allow(clippy::float_cmp))]

#[macro_use]
pub(crate) mod macros;

#[cfg(feature = "set-header")]
#[cfg_attr(docsrs, doc(cfg(feature = "set-header")))]
pub mod set_header;

#[cfg(feature = "propagate-header")]
#[cfg_attr(docsrs, doc(cfg(feature = "propagate-header")))]
pub mod propagate_header;

#[cfg(feature = "compression")]
#[cfg_attr(docsrs, doc(cfg(feature = "compression")))]
pub mod compression;

#[cfg(feature = "add-extension")]
#[cfg_attr(docsrs, doc(cfg(feature = "add-extension")))]
pub mod add_extension;

#[cfg(feature = "sensitive-headers")]
#[cfg_attr(docsrs, doc(cfg(feature = "sensitive-headers")))]
pub mod sensitive_headers;

#[cfg(feature = "decompression")]
#[cfg_attr(docsrs, doc(cfg(feature = "decompression")))]
pub mod decompression;

#[cfg(any(feature = "compression", feature = "decompression"))]
mod compression_utils;

#[cfg(feature = "map-response-body")]
#[cfg_attr(docsrs, doc(cfg(feature = "map-response-body")))]
pub mod map_response_body;

#[cfg(feature = "map-request-body")]
#[cfg_attr(docsrs, doc(cfg(feature = "map-request-body")))]
pub mod map_request_body;

#[cfg(feature = "trace")]
#[cfg_attr(docsrs, doc(cfg(feature = "trace")))]
pub mod trace;

#[cfg(feature = "follow-redirect")]
#[cfg_attr(docsrs, doc(cfg(feature = "follow-redirect")))]
pub mod follow_redirect;

pub mod classify;
pub mod services;

/// Error type containing either a body error or an IO error.
///
/// This type is used to combine errors produced by response bodies with compression or
/// decompression applied. The body itself can produce errors of type `E` whereas compression or
/// decompression can produce [`io::Error`]s.
///
/// [`io::Error`]: std::io::Error
#[cfg(any(feature = "compression", feature = "decompression"))]
#[cfg_attr(
    docsrs,
    doc(cfg(any(feature = "compression", feature = "decompression")))
)]
#[derive(Debug)]
pub enum BodyOrIoError<E> {
    /// Errors produced by the body.
    Body(E),
    /// IO errors produced by compression or decompression.
    Io(std::io::Error),
}

#[cfg(any(feature = "compression", feature = "decompression"))]
impl<E> std::fmt::Display for BodyOrIoError<E>
where
    E: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BodyOrIoError::Io(inner) => inner.fmt(f),
            BodyOrIoError::Body(inner) => inner.fmt(f),
        }
    }
}

#[cfg(any(feature = "compression", feature = "decompression"))]
impl<E> std::error::Error for BodyOrIoError<E>
where
    E: std::error::Error,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BodyOrIoError::Io(inner) => inner.source(),
            BodyOrIoError::Body(inner) => inner.source(),
        }
    }
}

/// The latency unit used to report latencies by middlewares.
#[non_exhaustive]
#[derive(Copy, Clone, Debug)]
pub enum LatencyUnit {
    /// Use milliseconds.
    Millis,
    /// Use microseconds.
    Micros,
    /// Use nanoseconds.
    Nanos,
}
