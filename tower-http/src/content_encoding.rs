use std::ffi::OsStr;

use http::{header, HeaderMap, HeaderValue};

pub(crate) trait SupportedEncodings: Copy {
    fn gzip(&self) -> bool;
    fn deflate(&self) -> bool;
    fn br(&self) -> bool;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    #[cfg(any(feature = "fs", feature = "compression-gzip"))]
    Gzip,
    #[cfg(any(feature = "fs", feature = "compression-deflate"))]
    Deflate,
    #[cfg(any(feature = "fs", feature = "compression-br"))]
    Brotli,
    Identity,
}

impl Encoding {
    fn to_str(self) -> &'static str {
        match self {
            #[cfg(any(feature = "fs", feature = "compression-gzip"))]
            Encoding::Gzip => "gzip",
            #[cfg(any(feature = "fs", feature = "compression-deflate"))]
            Encoding::Deflate => "deflate",
            #[cfg(any(feature = "fs", feature = "compression-br"))]
            Encoding::Brotli => "br",
            Encoding::Identity => "identity",
        }
    }

    #[cfg(feature = "fs")]
    pub(crate) fn to_file_extension(self) -> Option<&'static OsStr> {
        match self {
            Encoding::Gzip => Some(OsStr::new(".gz")),
            Encoding::Deflate => Some(OsStr::new(".zz")),
            Encoding::Brotli => Some(OsStr::new(".br")),
            Encoding::Identity => None,
        }
    }

    pub(crate) fn into_header_value(self) -> HeaderValue {
        HeaderValue::from_static(self.to_str())
    }

    #[allow(unused_variables)]
    fn parse(s: &str, supported_encoding: impl SupportedEncodings) -> Option<Encoding> {
        match s {
            #[cfg(any(feature = "fs", feature = "compression-gzip"))]
            "gzip" if supported_encoding.gzip() => Some(Encoding::Gzip),
            #[cfg(any(feature = "fs", feature = "compression-deflate"))]
            "deflate" if supported_encoding.deflate() => Some(Encoding::Deflate),
            #[cfg(any(feature = "fs", feature = "compression-br"))]
            "br" if supported_encoding.br() => Some(Encoding::Brotli),
            "identity" => Some(Encoding::Identity),
            _ => None,
        }
    }

    // based on https://github.com/http-rs/accept-encoding
    pub(crate) fn from_headers(
        headers: &HeaderMap,
        supported_encoding: impl SupportedEncodings,
    ) -> Self {
        let mut preferred_encoding = None;
        let mut max_qval = 0.0;

        for (encoding, qval) in encodings(headers, supported_encoding) {
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
fn encodings(
    headers: &HeaderMap,
    supported_encoding: impl SupportedEncodings,
) -> Vec<(Encoding, f32)> {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|hval| hval.to_str().ok())
        .flat_map(|s| s.split(',').map(str::trim))
        .filter_map(|v| {
            let mut v = v.splitn(2, ";q=");

            let encoding = match Encoding::parse(v.next().unwrap(), supported_encoding) {
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
