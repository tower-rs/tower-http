//! Middleware that compresses response bodies.

use crate::compression_utils::AcceptEncoding;
use http::{header, HeaderMap};

mod body;
mod future;
mod layer;
mod service;

pub use self::{
    body::CompressionBody, future::ResponseFuture, layer::CompressionLayer, service::Compression,
};

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
