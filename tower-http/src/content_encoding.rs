pub(crate) trait SupportedEncodings: Copy {
    fn gzip(&self) -> bool;
    fn deflate(&self) -> bool;
    fn br(&self) -> bool;
    fn zstd(&self) -> bool;
}

// This enum's variants are ordered from least to most preferred.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, PartialEq, Eq)]
pub(crate) enum Encoding {
    #[allow(dead_code)]
    Identity,
    #[cfg(any(feature = "fs", feature = "compression-deflate"))]
    Deflate,
    #[cfg(any(feature = "fs", feature = "compression-gzip"))]
    Gzip,
    #[cfg(any(feature = "fs", feature = "compression-br"))]
    Brotli,
    #[cfg(any(feature = "fs", feature = "compression-zstd"))]
    Zstd,
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
            #[cfg(any(feature = "fs", feature = "compression-zstd"))]
            Encoding::Zstd => "zstd",
            Encoding::Identity => "identity",
        }
    }

    #[cfg(feature = "fs")]
    pub(crate) fn to_file_extension(self) -> Option<&'static std::ffi::OsStr> {
        match self {
            Encoding::Gzip => Some(std::ffi::OsStr::new(".gz")),
            Encoding::Deflate => Some(std::ffi::OsStr::new(".zz")),
            Encoding::Brotli => Some(std::ffi::OsStr::new(".br")),
            Encoding::Zstd => Some(std::ffi::OsStr::new(".zst")),
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
        feature = "compression-zstd",
        feature = "fs",
    ))]
    fn parse(s: &str, _supported_encoding: impl SupportedEncodings) -> Option<Encoding> {
        #[cfg(any(feature = "fs", feature = "compression-gzip"))]
        if (s.eq_ignore_ascii_case("gzip") || s.eq_ignore_ascii_case("x-gzip"))
            && _supported_encoding.gzip()
        {
            return Some(Encoding::Gzip);
        }

        #[cfg(any(feature = "fs", feature = "compression-deflate"))]
        if s.eq_ignore_ascii_case("deflate") && _supported_encoding.deflate() {
            return Some(Encoding::Deflate);
        }

        #[cfg(any(feature = "fs", feature = "compression-br"))]
        if s.eq_ignore_ascii_case("br") && _supported_encoding.br() {
            return Some(Encoding::Brotli);
        }

        #[cfg(any(feature = "fs", feature = "compression-zstd"))]
        if s.eq_ignore_ascii_case("zstd") && _supported_encoding.zstd() {
            return Some(Encoding::Zstd);
        }

        if s.eq_ignore_ascii_case("identity") {
            return Some(Encoding::Identity);
        }

        None
    }

    #[cfg(any(
        feature = "compression-gzip",
        feature = "compression-br",
        feature = "compression-zstd",
        feature = "compression-deflate",
    ))]
    // based on https://github.com/http-rs/accept-encoding
    //
    // Returns `Some(encoding)` for the best acceptable encoding, or `None` if the client's
    // preferences cannot be satisfied (406 Not Acceptable per RFC 9110 §12.5.3).
    pub(crate) fn from_headers(
        headers: &http::HeaderMap,
        supported_encoding: impl SupportedEncodings,
    ) -> Option<Self> {
        preferred_encoding_with_wildcard(headers, supported_encoding)
    }

    #[cfg(any(
        feature = "compression-gzip",
        feature = "compression-br",
        feature = "compression-zstd",
        feature = "compression-deflate",
        feature = "fs",
    ))]
    pub(crate) fn preferred_encoding(
        accepted_encodings: impl Iterator<Item = (Encoding, QValue)>,
    ) -> Option<Self> {
        accepted_encodings
            .filter(|(_, qvalue)| qvalue.0 > 0)
            .max_by_key(|&(encoding, qvalue)| (qvalue, encoding))
            .map(|(encoding, _)| encoding)
    }
}

// Allowed q-values are numbers between 0 and 1 with at most 3 digits in the fractional part. They
// are presented here as an unsigned integer between 0 and 1000.
#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-zstd",
    feature = "compression-deflate",
    feature = "fs",
))]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct QValue(u16);

#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-zstd",
    feature = "compression-deflate",
    feature = "fs",
))]
impl QValue {
    #[inline]
    pub(crate) fn one() -> Self {
        Self(1000)
    }

    // Parse a q-value as specified in RFC 7231 section 5.3.1.
    fn parse(s: &str) -> Option<Self> {
        let mut c = s.chars();
        // Parse "q=" (case-insensitively).
        match c.next() {
            Some('q' | 'Q') => (),
            _ => return None,
        };
        match c.next() {
            Some('=') => (),
            _ => return None,
        };

        // Parse leading digit. Since valid q-values are between 0.000 and 1.000, only "0" and "1"
        // are allowed.
        let mut value = match c.next() {
            Some('0') => 0,
            Some('1') => 1000,
            _ => return None,
        };

        // Parse optional decimal point.
        match c.next() {
            Some('.') => (),
            None => return Some(Self(value)),
            _ => return None,
        };

        // Parse optional fractional digits. The value of each digit is multiplied by `factor`.
        // Since the q-value is represented as an integer between 0 and 1000, `factor` is `100` for
        // the first digit, `10` for the next, and `1` for the digit after that.
        let mut factor = 100;
        loop {
            match c.next() {
                Some(n @ '0'..='9') => {
                    // If `factor` is less than `1`, three digits have already been parsed. A
                    // q-value having more than 3 fractional digits is invalid.
                    if factor < 1 {
                        return None;
                    }
                    // Add the digit's value multiplied by `factor` to `value`.
                    value += factor * (n as u16 - '0' as u16);
                }
                None => {
                    // No more characters to parse. Check that the value representing the q-value is
                    // in the valid range.
                    return if value <= 1000 {
                        Some(Self(value))
                    } else {
                        None
                    };
                }
                _ => return None,
            };
            factor /= 10;
        }
    }
}

#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-zstd",
    feature = "compression-deflate",
    feature = "fs",
))]
// based on https://github.com/http-rs/accept-encoding
pub(crate) fn encodings<'a>(
    headers: &'a http::HeaderMap,
    supported_encoding: impl SupportedEncodings + 'a,
) -> impl Iterator<Item = (Encoding, QValue)> + 'a {
    headers
        .get_all(http::header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|hval| hval.to_str().ok())
        .flat_map(|s| s.split(','))
        .filter_map(move |v| {
            let mut v = v.splitn(2, ';');

            let encoding = match Encoding::parse(v.next().unwrap().trim(), supported_encoding) {
                Some(encoding) => encoding,
                None => return None, // ignore unknown encodings
            };

            let qval = if let Some(qval) = v.next() {
                QValue::parse(qval.trim())?
            } else {
                QValue::one()
            };

            Some((encoding, qval))
        })
}

/// Extracts the q-value for the `*` wildcard from Accept-Encoding headers.
/// Returns `None` if no wildcard is present.
#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-zstd",
    feature = "compression-deflate",
))]
fn wildcard_qvalue(headers: &http::HeaderMap) -> Option<QValue> {
    headers
        .get_all(http::header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|hval| hval.to_str().ok())
        .flat_map(|s| s.split(','))
        .find_map(|v| {
            let mut v = v.splitn(2, ';');
            let coding = v.next().unwrap().trim();
            if coding != "*" {
                return None;
            }
            let qval = if let Some(qval) = v.next() {
                QValue::parse(qval.trim())?
            } else {
                QValue::one()
            };
            Some(qval)
        })
}

/// Selects the preferred encoding considering the `*` wildcard per RFC 9110 §12.5.3.
///
/// The wildcard applies its q-value to any encoding not explicitly listed. If all acceptable
/// encodings (including identity) are excluded, returns `None` to signal 406 Not Acceptable.
#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-zstd",
    feature = "compression-deflate",
))]
fn preferred_encoding_with_wildcard(
    headers: &http::HeaderMap,
    supported_encoding: impl SupportedEncodings,
) -> Option<Encoding> {
    let explicit: Vec<(Encoding, QValue)> = encodings(headers, supported_encoding).collect();
    let wildcard_q = wildcard_qvalue(headers);

    // If there is no wildcard, use only the explicitly listed encodings.
    // Per RFC 9110 §12.5.3, if identity is excluded (q=0) and no other encoding is
    // acceptable, the server SHOULD respond with 406.
    let Some(wildcard_q) = wildcard_q else {
        let identity_rejected = explicit
            .iter()
            .any(|(enc, q)| *enc == Encoding::Identity && q.0 == 0);
        return match Encoding::preferred_encoding(explicit.into_iter()) {
            Some(enc) => Some(enc),
            None => {
                if identity_rejected {
                    None
                } else {
                    Some(Encoding::Identity)
                }
            }
        };
    };

    // Build the effective set of (encoding, qvalue) for all supported encodings.
    // For each supported encoding, use its explicit q-value if listed, otherwise the wildcard
    // q-value.
    let all_supported = all_supported_encodings(supported_encoding);

    let effective = all_supported.iter().filter_map(|e| *e).map(|enc| {
        let q = explicit
            .iter()
            .find(|(e, _)| *e == enc)
            .map(|(_, q)| *q)
            .unwrap_or(wildcard_q);
        (enc, q)
    });

    Encoding::preferred_encoding(effective)
}

/// Returns all encodings the server supports (including Identity) in a fixed-capacity array.
#[cfg(any(
    feature = "compression-gzip",
    feature = "compression-br",
    feature = "compression-zstd",
    feature = "compression-deflate",
))]
fn all_supported_encodings(supported_encoding: impl SupportedEncodings) -> [Option<Encoding>; 5] {
    let mut out: [Option<Encoding>; 5] = [None; 5];
    let mut n = 0;

    macro_rules! push {
        ($enc:expr) => {
            out[n] = Some($enc);
            n += 1;
        };
    }

    push!(Encoding::Identity);

    #[cfg(any(feature = "fs", feature = "compression-gzip"))]
    if supported_encoding.gzip() {
        push!(Encoding::Gzip);
    }

    #[cfg(any(feature = "fs", feature = "compression-deflate"))]
    if supported_encoding.deflate() {
        push!(Encoding::Deflate);
    }

    #[cfg(any(feature = "fs", feature = "compression-br"))]
    if supported_encoding.br() {
        push!(Encoding::Brotli);
    }

    #[cfg(any(feature = "fs", feature = "compression-zstd"))]
    if supported_encoding.zstd() {
        push!(Encoding::Zstd);
    }

    let _ = n;
    out
}

#[cfg(all(
    test,
    feature = "compression-gzip",
    feature = "compression-deflate",
    feature = "compression-br",
    feature = "compression-zstd",
))]
mod tests {
    use super::*;

    #[derive(Copy, Clone, Default)]
    struct SupportedEncodingsAll;

    impl SupportedEncodings for SupportedEncodingsAll {
        fn gzip(&self) -> bool {
            true
        }

        fn deflate(&self) -> bool {
            true
        }

        fn br(&self) -> bool {
            true
        }

        fn zstd(&self) -> bool {
            true
        }
    }

    #[test]
    fn no_accept_encoding_header() {
        let encoding = Encoding::from_headers(&http::HeaderMap::new(), SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);
    }

    #[test]
    fn accept_encoding_header_single_encoding() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Gzip), encoding);
    }

    #[test]
    fn accept_encoding_header_two_encodings() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip,br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn accept_encoding_header_gzip_x_gzip() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip,x-gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Gzip), encoding);
    }

    #[test]
    fn accept_encoding_header_x_gzip_deflate() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("deflate,x-gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Gzip), encoding);
    }

    #[test]
    fn accept_encoding_header_three_encodings() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip,deflate,br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn accept_encoding_header_two_encodings_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn accept_encoding_header_three_encodings_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,deflate,br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn two_accept_encoding_headers_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5"),
        );
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn two_accept_encoding_headers_three_encodings_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,deflate"),
        );
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn three_accept_encoding_headers_with_one_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5"),
        );
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("deflate"),
        );
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn accept_encoding_header_two_encodings_with_two_qvalues() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,br;q=0.8"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.8,br;q=0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Gzip), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.995,br;q=0.999"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn accept_encoding_header_three_encodings_with_three_qvalues() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,deflate;q=0.6,br;q=0.8"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.8,deflate;q=0.6,br;q=0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Gzip), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.6,deflate;q=0.8,br;q=0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Deflate), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.995,deflate;q=0.997,br;q=0.999"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn accept_encoding_header_invalid_encdoing() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("invalid,gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Gzip), encoding);
    }

    #[test]
    fn accept_encoding_header_with_qvalue_zero() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0."),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0,br;q=0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn accept_encoding_header_with_uppercase_letters() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gZiP"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Gzip), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5,br;Q=0.8"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn accept_encoding_header_with_allowed_spaces() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static(" gzip\t; q=0.5 ,\tbr ;\tq=0.8\t"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Brotli), encoding);
    }

    #[test]
    fn accept_encoding_header_with_invalid_spaces() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q =0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q= 0.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);
    }

    #[test]
    fn accept_encoding_header_with_invalid_quvalues() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=-0.1"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=00.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=0.5000"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=.5"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=1.01"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);

        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("gzip;q=1.001"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Identity), encoding);
    }

    #[test]
    fn wildcard_alone_picks_best_supported() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("*"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        // * with q=1 means all encodings are acceptable; picks the highest-priority supported
        assert_eq!(Some(Encoding::Zstd), encoding);
    }

    #[test]
    fn wildcard_q_zero_with_nothing_else_returns_not_satisfiable() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("*;q=0"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        // *;q=0 rejects everything, including identity
        assert_eq!(None, encoding);
    }

    #[test]
    fn wildcard_q_zero_with_gzip_picks_gzip() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("*;q=0,gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Gzip), encoding);
    }

    #[test]
    fn identity_q_zero_alone_returns_not_satisfiable() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("identity;q=0"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        // identity;q=0 with no other encoding explicitly listed: the server cannot
        // determine what the client accepts, so 406 per RFC 9110 §12.5.3
        assert_eq!(None, encoding);
    }

    #[test]
    fn identity_q_zero_with_gzip_picks_gzip() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("identity;q=0,gzip"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        assert_eq!(Some(Encoding::Gzip), encoding);
    }

    #[test]
    fn wildcard_q_zero_identity_q_zero_no_compression_returns_not_satisfiable() {
        // *;q=0,identity;q=0 with no explicit compression listed
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("*;q=0,identity;q=0"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        // Both wildcard and identity are q=0, and no explicit encoding is listed with q>0
        assert_eq!(None, encoding);
    }

    #[test]
    fn wildcard_with_low_qvalue() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("*;q=0.5,gzip;q=1"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        // gzip is explicitly q=1, everything else gets q=0.5 from wildcard
        assert_eq!(Some(Encoding::Gzip), encoding);
    }

    #[test]
    fn wildcard_q_zero_with_identity_picks_identity() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("*;q=0,identity"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedEncodingsAll);
        // *;q=0 rejects all, but identity is explicitly listed with q=1
        assert_eq!(Some(Encoding::Identity), encoding);
    }

    #[derive(Copy, Clone)]
    struct SupportedGzipOnly;

    impl SupportedEncodings for SupportedGzipOnly {
        fn gzip(&self) -> bool {
            true
        }
        fn deflate(&self) -> bool {
            false
        }
        fn br(&self) -> bool {
            false
        }
        fn zstd(&self) -> bool {
            false
        }
    }

    #[test]
    fn wildcard_with_partial_server_support_picks_best_available() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("*"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedGzipOnly);
        // Server only supports gzip, so * should pick gzip (not zstd/br)
        assert_eq!(Some(Encoding::Gzip), encoding);
    }

    #[test]
    fn wildcard_q_zero_with_unsupported_encoding_returns_not_satisfiable() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::ACCEPT_ENCODING,
            http::HeaderValue::from_static("*;q=0,br"),
        );
        let encoding = Encoding::from_headers(&headers, SupportedGzipOnly);
        // Client wants br, but server only supports gzip. br is not in the
        // supported set so it's ignored by encodings(). Wildcard rejects
        // everything else. Result: 406.
        assert_eq!(None, encoding);
    }
}
