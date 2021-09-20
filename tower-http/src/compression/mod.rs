//! Middleware that compresses response bodies.
//!
//! The middleware will only compress response bodies above a certain size. By default,
//! this lower limit is 32 bytes.
//!
//! # Example
//!
//! Example showing how to respond with the compressed contents of a file.
//!
//! ```rust
//! use bytes::{Bytes, BytesMut};
//! use http::{Request, Response, header::ACCEPT_ENCODING};
//! use http_body::Body as _; // for Body::data
//! use hyper::Body;
//! use std::convert::Infallible;
//! use tokio::fs::{self, File};
//! use tokio_util::io::ReaderStream;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use tower_http::compression::CompressionLayer;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
//!     // Open the file.
//!     let file = File::open("Cargo.toml").await.expect("file missing");
//!     // Convert the file into a `Stream`.
//!     let stream = ReaderStream::new(file);
//!     // Convert the `Stream` into a `Body`.
//!     let body = Body::wrap_stream(stream);
//!     // Create response.
//!     Ok(Response::new(body))
//! }
//!
//! let mut service = ServiceBuilder::new()
//!     // Compress responses based on the `Accept-Encoding` header.
//!     .layer(CompressionLayer::new())
//!     .service_fn(handle);
//!
//! // Call the service.
//! let request = Request::builder()
//!     .header(ACCEPT_ENCODING, "gzip")
//!     .body(Body::empty())?;
//!
//! let response = service
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(response.headers()["content-encoding"], "gzip");
//!
//! // Read the body
//! let mut body = response.into_body();
//! let mut bytes = BytesMut::new();
//! while let Some(chunk) = body.data().await {
//!     let chunk = chunk?;
//!     bytes.extend_from_slice(&chunk[..]);
//! }
//! let bytes: Bytes = bytes.freeze();
//!
//! // The compressed body should be smaller 🤞
//! let uncompressed_len = fs::read_to_string("Cargo.toml").await?.len();
//! assert!(bytes.len() < uncompressed_len);
//! #
//! # Ok(())
//! # }
//! ```

use crate::compression_utils::AcceptEncoding;
use http::{header, HeaderMap, HeaderValue};

mod body;
mod future;
mod layer;
mod service;

pub use self::{
    body::CompressionBody, future::ResponseFuture, layer::CompressionLayer, service::Compression,
};

/// The default minimum size a body needs to have for us to compress it.
///
/// Used to stop the layer from compressing 0-sized responses.
const MIN_SIZE_DEFAULT: u16 = 32;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    #[cfg(feature = "compression-gzip")]
    Gzip,
    #[cfg(feature = "compression-deflate")]
    Deflate,
    #[cfg(feature = "compression-br")]
    Brotli,
    Identity,
}

impl Encoding {
    fn to_str(self) -> &'static str {
        match self {
            #[cfg(feature = "compression-gzip")]
            Encoding::Gzip => "gzip",
            #[cfg(feature = "compression-deflate")]
            Encoding::Deflate => "deflate",
            #[cfg(feature = "compression-br")]
            Encoding::Brotli => "br",
            Encoding::Identity => "identity",
        }
    }

    fn into_header_value(self) -> HeaderValue {
        HeaderValue::from_static(self.to_str())
    }

    #[allow(unused_variables)]
    fn parse(s: &str, accept: AcceptEncoding) -> Option<Encoding> {
        match s {
            #[cfg(feature = "compression-gzip")]
            "gzip" if accept.gzip() => Some(Encoding::Gzip),
            #[cfg(feature = "compression-deflate")]
            "deflate" if accept.deflate() => Some(Encoding::Deflate),
            #[cfg(feature = "compression-br")]
            "br" if accept.br() => Some(Encoding::Brotli),
            "identity" => Some(Encoding::Identity),
            _ => None,
        }
    }

    // based on https://github.com/http-rs/accept-encoding
    fn from_headers(headers: &HeaderMap, accept: AcceptEncoding) -> Self {
        let mut preferred_encoding = None;
        let mut max_qval = 0.0;

        for (encoding, qval) in encodings(headers, accept) {
            if (qval - 1.0f32).abs() < 0.01 {
                preferred_encoding = Some(encoding);
                break;
            } else if qval > max_qval {
                preferred_encoding = Some(encoding);
                max_qval = qval;
            }
        }

        preferred_encoding.unwrap_or(Encoding::Identity)
    }
}

// based on https://github.com/http-rs/accept-encoding
fn encodings(headers: &HeaderMap, accept: AcceptEncoding) -> Vec<(Encoding, f32)> {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|hval| hval.to_str().ok())
        .flat_map(|s| s.split(',').map(str::trim))
        .filter_map(|v| {
            let mut v = v.splitn(2, ";q=");

            let encoding = match Encoding::parse(v.next().unwrap(), accept) {
                Some(encoding) => encoding,
                None => return None, // ignore unknown encodings
            };

            let qval = if let Some(qval) = v.next() {
                let qval = match qval.parse::<f32>() {
                    Ok(f) => f,
                    Err(_) => return None,
                };
                if qval > 1.0 {
                    return None; // q-values over 1 are unacceptable
                }
                qval
            } else {
                1.0f32
            };

            Some((encoding, qval))
        })
        .collect::<Vec<(Encoding, f32)>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_compression::tokio::write::{BrotliDecoder, BrotliEncoder};
    use bytes::BytesMut;
    use flate2::read::GzDecoder;
    use http_body::Body as _;
    use hyper::{Body, Error, Request, Response, Server};
    use std::{io::Read, net::SocketAddr, str};
    use tokio::io::AsyncWriteExt;
    use tower::{make::Shared, service_fn, Service, ServiceExt};

    #[tokio::test]
    async fn works() {
        let svc = service_fn(handle);
        let mut svc = Compression::new(svc).min_size(0);

        // call the service
        let req = Request::builder()
            .header("accept-encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();

        // read the compressed body
        let mut body = res.into_body();
        let mut data = BytesMut::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.unwrap();
            data.extend_from_slice(&chunk[..]);
        }
        let compressed_data = data.freeze().to_vec();

        // decompress the body
        // doing this with flate2 as that is much easier than async-compression and blocking during
        // tests is fine
        let mut decoder = GzDecoder::new(&compressed_data[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();

        assert_eq!(decompressed, "Hello, World!");
    }

    #[allow(dead_code)]
    async fn is_compatible_with_hyper() {
        let svc = service_fn(handle);
        let svc = Compression::new(svc).min_size(0);

        let make_service = Shared::new(svc);

        let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
        let server = Server::bind(&addr).serve(make_service);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn min_size() {
        let svc = service_fn(handle);
        let mut svc = Compression::new(svc).min_size(1024);

        // call the service
        let req = Request::builder()
            .header("accept-encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();

        // body should be uncompressed given it's too small
        assert!(res.headers().get("content-encoding").is_none());

        // read the body
        let mut body = res.into_body();
        let mut data = BytesMut::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.unwrap();
            data.extend_from_slice(&chunk[..]);
        }

        assert_eq!(str::from_utf8(&data).unwrap(), "Hello, World!");
    }

    #[tokio::test]
    async fn no_recompress() {
        const DATA: &str = "Hello, World! I'm already compressed with br!";

        let svc = service_fn(|_| async {
            let buf = {
                let mut buf = Vec::new();

                let mut enc = BrotliEncoder::new(&mut buf);
                enc.write_all(DATA.as_bytes()).await?;
                enc.flush().await?;
                buf
            };

            let resp = Response::builder()
                .header("content-encoding", "br")
                .body(Body::from(buf))
                .unwrap();
            Ok::<_, std::io::Error>(resp)
        });
        let mut svc = Compression::new(svc);

        // call the service
        //
        // note: the accept-encoding doesn't match the content-encoding above, so that
        // we're able to see if the compression layer triggered or not
        let req = Request::builder()
            .header("accept-encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();

        // check we didn't recompress
        assert_eq!(
            res.headers()
                .get("content-encoding")
                .and_then(|h| h.to_str().ok())
                .unwrap_or_default(),
            "br",
        );

        // read the compressed body
        let mut body = res.into_body();
        let mut data = BytesMut::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.unwrap();
            data.extend_from_slice(&chunk[..]);
        }

        // decompress the body
        let data = {
            let mut output_buf = Vec::new();
            let mut decoder = BrotliDecoder::new(&mut output_buf);
            decoder
                .write_all(&data)
                .await
                .expect("couldn't brotli-decode");
            decoder.flush().await.expect("couldn't flush");
            output_buf
        };

        assert_eq!(data, DATA.as_bytes());
    }

    async fn handle(_req: Request<Body>) -> Result<Response<Body>, Error> {
        Ok(Response::new(Body::from("Hello, World!")))
    }
}
