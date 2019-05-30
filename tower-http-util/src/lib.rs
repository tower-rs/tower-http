#![doc(html_root_url = "https://docs.rs/tower-http-service/0.1.0")]
#![deny(missing_docs, missing_debug_implementations, unreachable_pub)]
#![cfg_attr(test, deny(warnings))]

//! Specialization of `tower::Service` for working with HTTP services.

pub mod body;
pub mod connection;
pub mod service;

mod sealed;
