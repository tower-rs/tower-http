//! Middleware that decompresses response bodies.

mod body;
mod future;
mod layer;
mod service;

pub use self::{
    body::{DecompressionBody, Error},
    future::ResponseFuture,
    layer::DecompressionLayer,
    service::Decompression,
};
