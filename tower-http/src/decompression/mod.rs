mod body;
mod response;
mod request;

pub use self::body::DecompressionBody;

pub use self::response::{
    future::ResponseFuture as ResponseFuture, layer::ResponseDecompressionLayer as DecompressionLayer,
    service::ResponseDecompression as Decompression,
};

pub use self::request::{
    layer::RequestDecompressionLayer,
    service::RequestDecompression,
};