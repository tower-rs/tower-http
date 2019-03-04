#![doc(html_root_url = "https://docs.rs/tower-http-service/0.1.0")]
#![deny(missing_docs, missing_debug_implementations, unreachable_pub)]
#![cfg_attr(test, deny(warnings))]

//! Specialization of `tower::Service` for working with HTTP services.

extern crate bytes;
extern crate futures;
extern crate http;
extern crate tokio_buf;
extern crate tower_service;

mod body;
mod sealed;
mod service;
pub mod util;

pub use body::Body;
pub use service::HttpService;
