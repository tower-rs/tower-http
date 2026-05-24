use http::Uri;

use super::ConfigError;

pub(crate) struct TrustedOrigin(String);

impl TrustedOrigin {
    pub(crate) fn parse(input: &str) -> Result<Self, ConfigError> {
        let uri = Uri::parse_origin(input)?;

        Ok(Self(
            uri.canonical()
                .expect("parse_origin validates scheme and host"),
        ))
    }

    pub(crate) fn into_canonical(self) -> String {
        self.0
    }
}

pub(crate) trait UriExt: Sized {
    fn canonical(&self) -> Option<String>;

    fn effective_port(&self) -> Option<u16>;

    fn scheme_default_port(&self) -> Option<u16>;

    fn parse_origin(input: &str) -> Result<Self, ConfigError>;
}

impl UriExt for Uri {
    fn canonical(&self) -> Option<String> {
        let scheme = match self.scheme_str()? {
            s @ ("http" | "https") => s,
            _ => return None,
        };
        let host = self.host().filter(|h| !h.is_empty())?.to_ascii_lowercase();
        let default: u16 = if scheme == "https" { 443 } else { 80 };
        Some(match self.port_u16() {
            Some(p) if p != default => format!("{scheme}://{host}:{p}"),
            _ => format!("{scheme}://{host}"),
        })
    }

    fn effective_port(&self) -> Option<u16> {
        self.port_u16().or_else(|| self.scheme_default_port())
    }

    fn scheme_default_port(&self) -> Option<u16> {
        match self.scheme_str() {
            Some("https") => Some(443),
            Some("http") => Some(80),
            _ => None,
        }
    }

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

        let path = uri.path();

        if (path != "/" && !path.is_empty()) || uri.query().is_some() {
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

    fn uri(s: &str) -> Uri {
        s.parse()
            .unwrap_or_else(|e| panic!("{:?} failed to parse: {}", s, e))
    }

    #[test]
    fn test_canonical() {
        struct Test {
            input: &'static str,
            expected: Option<&'static str>,
        }

        let tests = [
            Test {
                input: "https://example.com",
                expected: Some("https://example.com"),
            },
            Test {
                input: "http://example.com",
                expected: Some("http://example.com"),
            },
            Test {
                input: "HTTPS://Example.COM",
                expected: Some("https://example.com"),
            },
            Test {
                input: "https://example.com:443",
                expected: Some("https://example.com"),
            },
            Test {
                input: "http://example.com:80",
                expected: Some("http://example.com"),
            },
            Test {
                input: "https://example.com:8443",
                expected: Some("https://example.com:8443"),
            },
            Test {
                input: "ftp://example.com",
                expected: None,
            },
        ];

        for test in tests {
            assert_eq!(
                uri(test.input).canonical().as_deref(),
                test.expected,
                "{}",
                test.input
            );
        }
    }

    #[test]
    fn test_effective_port() {
        struct Test {
            input: &'static str,
            expected: Option<u16>,
        }

        let tests = [
            Test {
                input: "https://example.com",
                expected: Some(443),
            },
            Test {
                input: "http://example.com",
                expected: Some(80),
            },
            Test {
                input: "https://example.com:443",
                expected: Some(443),
            },
            Test {
                input: "https://example.com:8443",
                expected: Some(8443),
            },
            Test {
                input: "ftp://example.com",
                expected: None,
            },
        ];

        for test in tests {
            assert_eq!(
                uri(test.input).effective_port(),
                test.expected,
                "{}",
                test.input
            );
        }
    }

    #[test]
    fn test_parse_origin_accepts() {
        for input in [
            "https://example.com",
            "http://example.com",
            "https://example.com:8443",
            "HTTPS://Example.COM",
            "https://example.com/",
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
            // Path/query/fragment not allowed on a trusted origin.
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
