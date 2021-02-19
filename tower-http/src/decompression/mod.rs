//! Middleware that decompresses response bodies.
//!
//! # Example
//!
//! ```rust
//! use bytes::BytesMut;
//! use http::{Request, Response};
//! use http_body::Body as _; // for Body::data
//! use hyper::Body;
//! use std::convert::Infallible;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use tower_http::{compression::Compression, decompression::DecompressionLayer};
//! #
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
//! #     let body = Body::from("Hello, World!");
//! #     Ok(Response::new(body))
//! # }
//!
//! // Some opaque service that applies compression.
//! let service = Compression::new(service_fn(handle));
//!
//! // Our HTTP client.
//! let mut client = ServiceBuilder::new()
//!     // Automatically decompress response bodies.
//!     .layer(DecompressionLayer::new())
//!     .service(service);
//!
//! // Call the service.
//! //
//! // `DecompressionLayer` takes care of setting `Accept-Encoding`.
//! let request = Request::new(Body::empty());
//!
//! let response = client
//!     .ready_and()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! // Read the body
//! let mut body = response.into_body();
//! let mut bytes = BytesMut::new();
//! while let Some(chunk) = body.data().await {
//!     let chunk = chunk?;
//!     bytes.extend_from_slice(&chunk[..]);
//! }
//! let body = String::from_utf8(bytes.to_vec())?;
//!
//! assert_eq!(body, "Hello, World!");
//! #
//! # Ok(())
//! # }
//! ```

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
