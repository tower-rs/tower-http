use http::Uri;

use super::ConfigError;

/// Internal extension methods on [`http::Uri`] used by the CSRF middleware to
/// validate trusted-origin strings.
pub(crate) trait UriExt: Sized {
    /// Parses a trusted-origin string of the form `scheme://host[:port]`.
    ///
    /// Rejects inputs that can't represent a browser `Origin`:
    ///
    /// - unparseable URIs ([`ConfigError::InvalidOriginUrl`]);
    /// - non-`http`/`https` schemes or missing host ([`ConfigError::OpaqueOrigin`]);
    /// - any path, query, or fragment component
    ///   ([`ConfigError::InvalidOriginUrlComponents`] — including a bare trailing
    ///   `/` and fragments that `http::Uri` would otherwise silently strip);
    /// - non-ASCII hostnames ([`ConfigError::NonAsciiHostname`] — IDN hosts
    ///   must be supplied in punycode, since that's what browsers send).
    ///
    /// The returned [`Uri`] is parsed but not normalized; the origin is matched
    /// against the request's `Origin` header byte-for-byte.
    fn parse_origin(input: &str) -> Result<Self, ConfigError>;
}

impl UriExt for Uri {
    fn parse_origin(input: &str) -> Result<Self, ConfigError> {
        if input.contains('#') {
            return Err(ConfigError::InvalidOriginUrlComponents {
                origin: input.to_owned(),
            });
        }

        // browsers will send punycode anyways
        if !input.is_ascii() {
            return Err(ConfigError::NonAsciiHostname {
                origin: input.to_owned(),
            });
        }

        let uri: Uri =
            input
                .parse()
                .map_err(|e: http::uri::InvalidUri| ConfigError::InvalidOriginUrl {
                    origin: input.to_owned(),
                    message: e.to_string(),
                })?;

        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.host().map_or(true, |h| h.is_empty())
        {
            return Err(ConfigError::OpaqueOrigin {
                origin: input.to_owned(),
            });
        }

        // Reject any path/query (fragments are rejected above). `http::Uri`
        // reports `path()` as "/" for both `scheme://host` and `scheme://host/`,
        // so detect a path from the raw input (everything after "://") to reach
        // parity with Go, which rejects a non-empty path — including a bare "/".
        let after_scheme = input.split_once("://").map_or("", |(_, rest)| rest);

        if after_scheme.contains('/') || uri.query().is_some() {
            return Err(ConfigError::InvalidOriginUrlComponents {
                origin: input.to_owned(),
            });
        }

        Ok(uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_origin_accepts() {
        for input in [
            "https://example.com",
            "http://example.com",
            "https://example.com:8443",
            "HTTPS://Example.COM",
        ] {
            assert!(
                Uri::parse_origin(input).is_ok(),
                "expected Ok for {input:?}, got {:?}",
                Uri::parse_origin(input)
            );
        }
    }

    #[test]
    fn test_parse_origin_rejects() {
        // Each row maps an input to the expected ConfigError variant.
        // Marker functions over closures because PartialEq on the enum already
        // makes equality the easy assertion shape.
        type Check = fn(&ConfigError) -> bool;
        let cases: &[(&str, Check)] = &[
            // http::Uri rejects these outright at parse time.
            ("not a valid url", |e| {
                matches!(e, ConfigError::InvalidOriginUrl { .. })
            }),
            ("https://", |e| {
                matches!(e, ConfigError::InvalidOriginUrl { .. })
            }),
            ("file:///", |e| {
                matches!(e, ConfigError::InvalidOriginUrl { .. })
            }),
            // Parse OK but scheme is not http/https (or absent).
            ("example.com", |e| {
                matches!(e, ConfigError::OpaqueOrigin { .. })
            }),
            ("file://host/path", |e| {
                matches!(e, ConfigError::OpaqueOrigin { .. })
            }),
            ("mailto:x@y.z", |e| {
                matches!(e, ConfigError::OpaqueOrigin { .. })
            }),
            ("javascript:alert(1)", |e| {
                matches!(e, ConfigError::OpaqueOrigin { .. })
            }),
            // Path/query/fragment not allowed on a trusted origin. A bare
            // trailing slash is a (non-empty) path too — rejected, matching Go.
            ("https://example.com/", |e| {
                matches!(e, ConfigError::InvalidOriginUrlComponents { .. })
            }),
            ("https://example.com/path", |e| {
                matches!(e, ConfigError::InvalidOriginUrlComponents { .. })
            }),
            ("https://example.com/path?query=value", |e| {
                matches!(e, ConfigError::InvalidOriginUrlComponents { .. })
            }),
            ("https://example.com/path#fragment", |e| {
                matches!(e, ConfigError::InvalidOriginUrlComponents { .. })
            }),
            // http::Uri silently strips fragments; the `contains('#')` pre-check
            // surfaces these as component errors instead of letting them slip in.
            ("https://example.com#fragment", |e| {
                matches!(e, ConfigError::InvalidOriginUrlComponents { .. })
            }),
            ("https://example.com/#fragment", |e| {
                matches!(e, ConfigError::InvalidOriginUrlComponents { .. })
            }),
            // IDN hosts must be supplied in punycode.
            ("https://ümlaut.de", |e| {
                matches!(e, ConfigError::NonAsciiHostname { .. })
            }),
            ("https://日本.jp", |e| {
                matches!(e, ConfigError::NonAsciiHostname { .. })
            }),
        ];

        for (input, predicate) in cases {
            match Uri::parse_origin(input) {
                Err(e) if predicate(&e) => {}
                other => panic!("unexpected result for {:?}: {:?}", input, other),
            }
        }
    }
}
