//! `async fn(HttpRequest) -> Result<HttpResponse, Error>`
//!
//! # Overview
//!
//! `tower-http` is a library that provides HTTP-specific middlewares and utilities built on top of
//! the [`tower`] and [`http`] crates.
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
//!     sensitive_header::SetSensitiveHeaderLayer,
//!     set_response_header::SetResponseHeaderLayer,
//! };
//! use tower::{ServiceBuilder, service_fn};
//! use http::{Request, Response, header::{HeaderName, CONTENT_TYPE, AUTHORIZATION}};
//! use hyper::{Body, Error, server::Server, service::make_service_fn};
//! use std::{sync::Arc, net::SocketAddr, convert::Infallible};
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
//! /// Shared state across all request handlers --- in this case, a pool of database connections.
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
//!         // Share an `Arc<State>` with all requests
//!         .layer(AddExtensionLayer::new(Arc::new(state)))
//!         // Compress responses
//!         .layer(CompressionLayer::new())
//!         // Propagate `X-Request-Header`s from requests to responses
//!         .layer(PropagateHeaderLayer::new(HeaderName::from_static("x-request-id")))
//!         // Mark the `Authorization` header as sensitive so it doesn't show in logs
//!         .layer(SetSensitiveHeaderLayer::new(AUTHORIZATION))
//!         // If the response has a known size set the `Content-Type` header
//!         .layer(SetResponseHeaderLayer::overriding(CONTENT_TYPE, content_length_from_response))
//!         // Wrap a `Service` in our middleware stack
//!         .service(service_fn(handler));
//!
//!     // And run our service using `hyper`
//!     let make_service = make_service_fn(move |_conn| {
//!         let service = service.clone();
//!         async move {
//!             Ok::<_, Infallible>(service)
//!         }
//!     });
//!
//!     let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
//!
//!     let server = Server::bind(&addr).serve(make_service);
//!
//!     if let Err(e) = server.await {
//!         eprintln!("server error: {}", e);
//!     }
//! }
//! ```
//!
//! Keep in mind that while this example uses [`hyper`], `tower-http` supports any HTTP
//! client/server implementation that uses the [`http`] and [`http-body`] crates.
//!
//! # Feature Flags
//!
//! All middleware are disabled by default and can be enabled using [cargo features].
//!
//! For example, to enable the [`AddExtension`] middleware, add the "add-extension" feature flag
//! in your`Cargo.toml`:
//!
//! ```toml
//! tower-http = { version = "0.1.0", features = ["add-extension"] }
//! ```
//!
//! You can use `"full"` to enable everything:
//!
//! ```toml
//! tower-http = { version = "0.1.0", features = ["full"] }
//! ```
//!
//! [`tower`]: https://crates.io/crates/tower
//! [`http`]: https://crates.io/crates/http
//! [`http-body`]: https://crates.io/crates/http-body
//! [`hyper`]: https://crates.io/crates/hyper
//! [cargo features]: https://doc.rust-lang.org/cargo/reference/features.html
//! [`AddExtension`]: crate::add_extension::AddExtension
//! [`Service`]: https://docs.rs/tower/latest/tower/trait.Service.html

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
#![allow(elided_lifetimes_in_paths, clippy::type_complexity)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(test, allow(clippy::float_cmp))]

#[macro_use]
pub(crate) mod macros;

#[cfg(feature = "set-response-header")]
#[cfg_attr(docsrs, doc(cfg(feature = "set-response-header")))]
pub mod set_response_header;

#[cfg(feature = "propagate-header")]
#[cfg_attr(docsrs, doc(cfg(feature = "propagate-header")))]
pub mod propagate_header;

#[cfg(feature = "compression")]
#[cfg_attr(docsrs, doc(cfg(feature = "compression")))]
pub mod compression;

#[cfg(feature = "add-extension")]
#[cfg_attr(docsrs, doc(cfg(feature = "add-extension")))]
pub mod add_extension;

#[cfg(feature = "sensitive-header")]
#[cfg_attr(docsrs, doc(cfg(feature = "sensitive-header")))]
pub mod sensitive_header;

#[cfg(feature = "decompression")]
#[cfg_attr(docsrs, doc(cfg(feature = "decompression")))]
pub mod decompression;

#[cfg(feature = "map-response-body")]
#[cfg_attr(docsrs, doc(cfg(feature = "map-response-body")))]
pub mod map_response_body;

#[cfg(feature = "map-request-body")]
#[cfg_attr(docsrs, doc(cfg(feature = "map-request-body")))]
pub mod map_request_body;

pub mod services;

mod accept_encoding;
