//! `async fn(HttpRequest) -> Result<HttpResponse, Error>`
//!
//! # Overview
//!
//! tower-http is a library that provides HTTP specific middlewares and utilities built on top of
//! [tower].
//!
//! All middlewares uses the [http] and [http-body] crates as the HTTP abstractions. That means
//! they're compatible with any library or framework that also uses those crates, such as [hyper].
//! Some middlewares might bring in other dependencies for doing various things.
//!
//! # Example
//!
//! ```rust
//! use tower_http::{
//!     add_extension::AddExtensionLayer,
//!     compression::CompressionLayer,
//! };
//! use tower::{ServiceBuilder, service_fn};
//! use http::{Request, Response};
//! use hyper::Body;
//! use std::sync::Arc;
//! # struct Error;
//! # struct DatabaseConnectionPool;
//! # impl DatabaseConnectionPool {
//! #     fn new() -> DatabaseConnectionPool { DatabaseConnectionPool }
//! # }
//! # async fn run_http_service<T>(_: T) {}
//!
//! async fn handler(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     // ...
//!     # todo!()
//! }
//!
//! struct State {
//!     pool: DatabaseConnectionPool,
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let state = State {
//!         pool: DatabaseConnectionPool::new(),
//!     };
//!
//!     let service = ServiceBuilder::new()
//!         // Share an `Arc<State>` with all requests
//!         .layer(AddExtensionLayer::new(Arc::new(state)))
//!         // Compress responses
//!         .layer(CompressionLayer::new())
//!         // Wrap a `Service` in our middleware stack
//!         .service(service_fn(handler));
//!
//!     // Run our service using some HTTP server
//!     run_http_service(service).await;
//! }
//! ```
//!
//! # Feature toggles
//!
//! All middlewares are disabled by default and can be enabled using [cargo features].
//!
//! For example to enable [`AddExtension`] you would add this to your `Cargo.toml`:
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
//! [tower]: https://crates.io/crates/tower
//! [http]: https://crates.io/crates/http
//! [http-body]: https://crates.io/crates/http-body
//! [hyper]: https://crates.io/crates/hyper
//! [cargo features]: https://doc.rust-lang.org/cargo/reference/features.html
//! [`AddExtension`]: crate::add_extension::AddExtension

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

#[cfg(feature = "compression")]
#[cfg_attr(docsrs, doc(cfg(feature = "compression")))]
pub mod compression;

#[cfg(feature = "add-extension")]
#[cfg_attr(docsrs, doc(cfg(feature = "add-extension")))]
pub mod add_extension;

#[cfg(feature = "decompression")]
#[cfg_attr(docsrs, doc(cfg(feature = "decompression")))]
pub mod decompression;

mod accept_encoding;
