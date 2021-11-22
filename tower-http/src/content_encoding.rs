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
    #[allow(dead_code)]
    Identity,
}

impl Encoding {
    #[allow(dead_code)]
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
    pub(crate) fn to_file_extension(self) -> Option<&'static std::ffi::OsStr> {
        match self {
            Encoding::Gzip => Some(std::ffi::OsStr::new(".gz")),
            Encoding::Deflate => Some(std::ffi::OsStr::new(".zz")),
            Encoding::Brotli => Some(std::ffi::OsStr::new(".br")),
            Encoding::Identity => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn into_header_value(self) -> http::HeaderValue {
        http::HeaderValue::from_static(self.to_str())
    }

    #[cfg(any(
        feature = "compression-gzip",
        feature = "compression-br",
        feature = "compression-deflate",
        feature = "fs",
    ))]
    fn parse(s: &str, _supported_encoding: impl SupportedEncodings) -> Option<Encoding> {
        match s {
            #[cfg(any(feature = "fs", feature = "compression-gzip"))]
            "gzip" if _supported_encoding.gzip() => Some(Encoding::Gzip),
            #[cfg(any(feature = "fs", feature = "compression-deflate"))]
            "deflate" if _supported_encoding.deflate() => Some(Encoding::Deflate),
            #[cfg(any(feature = "fs", feature = "compression-br"))]
            "br" if _supported_encoding.br() => Some(Encoding::Brotli),
            "identity" => Some(Encoding::Identity),
            _ => None,
        }
    }

    #[cfg(any(
        feature = "compression-gzip",
        feature = "compression-br",
        feature = "compression-deflate",
    ))]
    // based on https://github.com/http-rs/accept-encoding
    pub(crate) fn from_headers(
        headers: &http::HeaderMap,
        supported_encoding: impl SupportedEncodings,
    ) -> Self {
        Encoding::preferred_encoding(&encodings(headers, supported_encoding))
            .unwrap_or(Encoding::Identity)
    }

    #[cfg(any(
        feature = "compression-gzip",
        feature = "compression-br",
        feature = "compression-deflate",
        feature = "fs",
    ))]
    pub(crate) fn preferred_encoding(accepted_encodings: &[(Encoding, f32)]) -> Option<Self> {
        let mut preferred_encoding = None;
        let mut max_qval = 0.0;

        for (encoding, qval) in accepted_encodings {
            if (qval - 1.0f32).abs() < 0.01 {
                preferred_encoding = Some(*encoding);
                break;
            } else if *qval > max_qval {
                preferred_encoding = Some(*encoding);
                max_qval = *qval;
            }
        }
        preferred_encoding
    }
}

#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-deflate",
    feature = "fs",
))]
// based on https://github.com/http-rs/accept-encoding
pub(crate) fn encodings(
    headers: &http::HeaderMap,
    supported_encoding: impl SupportedEncodings,
) -> Vec<(Encoding, f32)> {
    headers
        .get_all(http::header::ACCEPT_ENCODING)
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
