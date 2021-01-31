//! Middleware that decompresses response bodies.

mod body;
mod future;
mod layer;
mod service;

pub use self::{
    body::DecompressionBody, future::ResponseFuture, layer::DecompressionLayer,
    service::Decompression,
};
