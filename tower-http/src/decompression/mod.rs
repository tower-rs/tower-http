//! Middleware for decompressing Requests and Responses

mod body;
pub mod request;
mod response;

pub use self::{
    body::DecompressionBody,
    request::{
        layer::RequestDecompressionLayer,
        service::RequestDecompression
    },
    response::{
        future::ResponseFuture, layer::ResponseDecompressionLayer as DecompressionLayer,
        service::ResponseDecompression as Decompression,
    },
};
