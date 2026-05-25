//! Modern protection against [cross-site request forgery] (CSRF) attacks.
//!
//! This middleware implements the CSRF protection scheme [introduced in Go 1.25][go]
//! and described in [Filippo Valsorda's blog post][filippo]. It relies on the
//! [`Sec-Fetch-Site`] and [`Origin`] request headers and requires no
//! per-request token state.
//!
//! Requests are allowed if any of the following hold:
//!
//! 1. The method is `GET`, `HEAD`, or `OPTIONS`.
//! 2. The `Origin` header matches an allow-listed trusted origin.
//! 3. `Sec-Fetch-Site` is `same-origin` or `none`.
//! 4. Neither `Sec-Fetch-Site` nor `Origin` is present.
//! 5. The `Origin` host (with effective port) matches the `Host` header.
//!
//! Rejected requests receive a `403 Forbidden` response. The originating
//! [`ProtectionError`] is attached to the response's extensions so handlers can
//! distinguish between explicit cross-origin rejections and conservative
//! fallback rejections (e.g. requests from old browsers without
//! `Sec-Fetch-Site`). Use
//! [`CsrfLayer::with_rejection_response`](CsrfLayer::with_rejection_response)
//! to replace the rejection response with a custom builder.
//!
//! # Deployment caveat
//!
//! The middleware trusts whatever `Origin` and `Host` reach it. Reverse proxies
//! and load balancers that rewrite `Host` (e.g. to an internal hostname) or
//! strip `Origin` silently degrade the protection: the `Origin`/`Host`
//! fallback can no longer match, and `Sec-Fetch-Site` becomes the only
//! remaining line of defense. Configure intermediaries to forward both headers
//! unchanged.
//!
//! # Example
//!
//! ```
//! use bytes::Bytes;
//! use http::{Request, Response, StatusCode};
//! use http_body_util::Full;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn, BoxError};
//! use tower_http::csrf::CsrfLayer;
//!
//! async fn handle(_: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, BoxError> {
//!     Ok(Response::new(Full::default()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! let layer = CsrfLayer::new()
//!     .add_trusted_origin("https://example.com")?;
//!
//! let mut service = ServiceBuilder::new()
//!     .layer(layer)
//!     .service_fn(handle);
//!
//! // Safe methods always pass.
//! let request = Request::builder()
//!     .method("GET")
//!     .uri("/")
//!     .body(Full::default())
//!     .unwrap();
//!
//! let response = service.ready().await?.call(request).await?;
//!
//! assert_eq!(response.status(), StatusCode::OK);
//!
//! // Cross-site POSTs are blocked.
//! let request = Request::builder()
//!     .method("POST")
//!     .uri("/")
//!     .header("host", "example.com")
//!     .header("sec-fetch-site", "cross-site")
//!     .body(Full::default())
//!     .unwrap();
//!
//! let response = service.ready().await?.call(request).await?;
//!
//! assert_eq!(response.status(), StatusCode::FORBIDDEN);
//!
//! # Ok(())
//! # }
//! ```
//!
//! [cross-site request forgery]: https://developer.mozilla.org/en-US/docs/Glossary/CSRF
//! [filippo]: https://words.filippo.io/csrf/
//! [go]: https://pkg.go.dev/net/http#CrossOriginProtection
//! [`Sec-Fetch-Site`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Sec-Fetch-Site
//! [`Origin`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Origin

use std::collections::HashSet;
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;

use http::{Method, Uri};

mod future;
mod layer;
mod response;
mod service;
mod url;

pub use self::future::ResponseFuture;
pub use self::layer::CsrfLayer;
pub use self::response::{DefaultResponseForProtectionError, ResponseForProtectionError};
pub use self::service::Csrf;

/// Errors that can occur while configuring [`CsrfLayer`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ConfigError {
    /// The origin string could not be parsed as a URI.
    InvalidOriginUrl {
        /// The offending origin string.
        origin: String,
        /// The parser error message.
        message: String,
    },

    /// An origin URL containing a path, query, or fragment was added as a
    /// trusted origin.
    InvalidOriginUrlComponents {
        /// The offending origin string.
        origin: String,
    },

    /// An origin with a scheme other than `http` or `https` (e.g. `file://`,
    /// `mailto:`, or a bare host with no scheme) was added as a trusted
    /// origin. Such origins can never match a browser-supplied request
    /// `Origin`.
    OpaqueOrigin {
        /// The offending origin string.
        origin: String,
    },

    /// A trusted origin contained non-ASCII characters. Browsers send IDN
    /// hostnames in punycode form, so the configured value must use the
    /// punycode form (e.g. `xn--exmple-cua.com`) to ever match.
    NonAsciiHostname {
        /// The offending origin string.
        origin: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidOriginUrl { origin, message } => {
                write!(f, "invalid origin {origin:?}: {message}")
            }
            ConfigError::InvalidOriginUrlComponents { origin } => write!(
                f,
                "invalid origin {origin:?}: path, query, and fragment are not allowed"
            ),
            ConfigError::OpaqueOrigin { origin } => write!(
                f,
                "invalid origin {origin:?}: scheme must be http or https"
            ),
            ConfigError::NonAsciiHostname { origin } => write!(
                f,
                "invalid origin {origin:?}: non-ASCII hostnames must be supplied in punycode (xn--…)"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Reason a request was rejected by [`Csrf`].
///
/// Attached to the `403 Forbidden` response's extensions so handlers can
/// distinguish between explicit cross-origin rejections and conservative
/// fallback rejections.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ProtectionError {
    /// A cross-origin request was detected via `Sec-Fetch-Site`.
    CrossOriginRequest,

    /// A request without `Sec-Fetch-Site` failed the `Origin`/`Host` fallback
    /// check. Modern browsers always send `Sec-Fetch-Site`, so this typically
    /// means the request came from an old browser or non-browser client.
    CrossOriginRequestFromOldBrowser,
}

impl fmt::Display for ProtectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ProtectionError::CrossOriginRequest => f.write_str("Cross-Origin request detected"),
            ProtectionError::CrossOriginRequestFromOldBrowser => {
                f.write_str("Cross-Origin request from old browser detected")
            }
        }
    }
}

impl std::error::Error for ProtectionError {}

type BypassFn = dyn Fn(&Method, &Uri) -> bool + Send + Sync + 'static;

struct DebugFn;

impl Debug for DebugFn {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("<fn>")
    }
}

#[derive(Clone, Debug, Default)]
struct Origins(Arc<HashSet<String>>);

impl Origins {
    fn contains(&self, origin: &str) -> bool {
        self.0.contains(origin)
    }

    fn insert(&mut self, origin: impl Into<String>) {
        Arc::make_mut(&mut self.0).insert(origin.into());
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use http::{Request, Response, StatusCode};
    use tower::{service_fn, ServiceExt};
    use tower_layer::Layer;

    use super::*;
    use crate::test_helpers::{to_bytes, Body};

    fn echo_service() -> impl tower::Service<
        Request<Body>,
        Response = Response<Body>,
        Error = Infallible,
        Future = impl std::future::Future<Output = Result<Response<Body>, Infallible>>,
    > + Clone {
        service_fn(|req: Request<Body>| async move {
            let body: Body = match req.uri().path() {
                "/foo" => "foo".into(),
                "/bar" => "bar".into(),
                _ => Body::empty(),
            };
            Ok::<_, Infallible>(Response::new(body))
        })
    }

    #[tokio::test]
    async fn test_service_allows_safe_method() {
        let svc = CsrfLayer::new()
            .add_trusted_origin("https://example.com")
            .unwrap()
            .layer(echo_service());

        let req = Request::builder()
            .method("GET")
            .uri("/foo")
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);

        let body = to_bytes(res.into_body()).await.unwrap();
        assert_eq!(&body[..], b"foo");
    }

    #[tokio::test]
    async fn test_service_allows_post_from_trusted_origin() {
        let svc = CsrfLayer::new()
            .add_trusted_origin("https://example.com")
            .unwrap()
            .layer(echo_service());

        let req = Request::builder()
            .method("POST")
            .uri("/bar")
            .header("origin", "https://example.com")
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);

        let body = to_bytes(res.into_body()).await.unwrap();
        assert_eq!(&body[..], b"bar");
    }

    #[tokio::test]
    async fn test_service_rejects_post_from_untrusted_origin() {
        let svc = CsrfLayer::new()
            .add_trusted_origin("https://example.com")
            .unwrap()
            .layer(echo_service());

        let req = Request::builder()
            .method("POST")
            .uri("/bar")
            .header("origin", "https://malicious.example")
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            res.extensions().get::<ProtectionError>(),
            Some(&ProtectionError::CrossOriginRequestFromOldBrowser),
        );
    }

    #[tokio::test]
    async fn test_service_uses_custom_rejection_response() {
        let svc = CsrfLayer::new()
            .with_rejection_response(|_err: ProtectionError| {
                let mut res = Response::new(Body::from("denied"));
                *res.status_mut() = StatusCode::IM_A_TEAPOT;
                res
            })
            .layer(echo_service());

        let req = Request::builder()
            .method("POST")
            .uri("/bar")
            .header("origin", "https://malicious.example")
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::IM_A_TEAPOT);
        assert!(res.extensions().get::<ProtectionError>().is_none());

        let body = to_bytes(res.into_body()).await.unwrap();
        assert_eq!(&body[..], b"denied");
    }

    #[test]
    fn test_layer_add_trusted_origin() {
        // Smoke check that the layer threads parse_origin's Ok and Err
        // through; the full validation matrix lives in url.rs.
        assert!(CsrfLayer::new()
            .add_trusted_origin("https://example.com")
            .is_ok());
        assert!(matches!(
            CsrfLayer::new().add_trusted_origin("not a valid url"),
            Err(ConfigError::InvalidOriginUrl { .. })
        ));
    }

    #[test]
    fn test_middleware_bypass() {
        let layer = CsrfLayer::new()
            .with_insecure_bypass(|_method, uri| -> bool { uri.path() == "/bypass" });

        let middleware = layer.layer(());

        struct Test {
            name: &'static str,
            path: &'static str,
            sec_fetch_site: Option<&'static str>,
            result: Result<(), ProtectionError>,
        }

        let tests = [
            Test {
                name: "bypass path without sec-fetch-site",
                path: "/bypass",
                sec_fetch_site: None,
                result: Ok(()),
            },
            Test {
                name: "bypass path with cross-site",
                path: "/bypass",
                sec_fetch_site: Some("cross-site"),
                result: Ok(()),
            },
            Test {
                name: "non-bypass path without sec-fetch-site",
                path: "/api",
                sec_fetch_site: None,
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "non-bypass path with cross-site",
                path: "/api",
                sec_fetch_site: Some("cross-site"),
                result: Err(ProtectionError::CrossOriginRequest),
            },
        ];

        for test in tests {
            let mut req = Request::builder()
                .method("POST")
                .header("host", "example.com")
                .header("origin", "https://attacker.example")
                .uri(format!("https://example.com{}", test.path));

            if let Some(sec_fetch_site) = test.sec_fetch_site {
                req = req.header("sec-fetch-site", sec_fetch_site);
            }

            let req = req.body(()).unwrap();

            assert_eq!(middleware.verify(&req), test.result, "{}", test.name);
        }
    }

    #[test]
    fn test_middleware_bypass_applies_when_origin_unparseable() {
        let middleware = CsrfLayer::new()
            .with_insecure_bypass(|_method, uri| uri.path() == "/bypass")
            .layer(());

        let req = Request::builder()
            .method("POST")
            .uri("https://example.com/bypass")
            .header("host", "example.com")
            .header(
                "origin",
                http::HeaderValue::from_bytes(&[0xFF, 0xFE]).unwrap(),
            )
            .body(())
            .unwrap();

        assert_eq!(middleware.verify(&req), Ok(()));
    }

    #[test]
    fn test_middleware_debug_trait() {
        let layer = CsrfLayer::new();

        let middleware = layer
            .clone()
            .with_insecure_bypass(|method, uri| method == Method::POST && uri.path() == "/bypass")
            .layer(());

        assert_eq!(
            format!("{:?}", middleware),
            "Csrf { inner: (), insecure_bypass: Some(<fn>), trusted_origins: Origins({}), rejection_response: <fn> }"
        );

        let middleware = layer.layer(());

        assert_eq!(
            format!("{:?}", middleware),
            "Csrf { inner: (), insecure_bypass: None, trusted_origins: Origins({}), rejection_response: <fn> }"
        );
    }

    #[test]
    fn test_middleware_origin_host_port_match() {
        let middleware: Csrf<()> = Default::default();

        struct Test {
            name: &'static str,
            uri: &'static str,
            host: Option<&'static str>,
            origin: &'static str,
            result: Result<(), ProtectionError>,
        }

        let tests = [
            Test {
                name: "default port both sides",
                uri: "/",
                host: Some("example.com"),
                origin: "https://example.com",
                result: Ok(()),
            },
            Test {
                name: "same non-default port both sides",
                uri: "/",
                host: Some("example.com:8443"),
                origin: "https://example.com:8443",
                result: Ok(()),
            },
            Test {
                name: "explicit default port both sides",
                uri: "/",
                host: Some("example.com:443"),
                origin: "https://example.com:443",
                result: Ok(()),
            },
            Test {
                name: "mismatched non-default ports",
                uri: "/",
                host: Some("example.com:8443"),
                origin: "https://example.com:8444",
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "origin has explicit default, host implicit",
                uri: "/",
                host: Some("example.com"),
                origin: "https://example.com:443",
                result: Ok(()),
            },
            Test {
                name: "host has explicit default, origin implicit",
                uri: "/",
                host: Some("example.com:443"),
                origin: "https://example.com",
                result: Ok(()),
            },
            Test {
                name: "host implicit, origin explicit non-default",
                uri: "/",
                host: Some("example.com"),
                origin: "https://example.com:8443",
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "missing host, uri authority implicit, origin explicit non-default",
                uri: "https://example.com/path",
                host: None,
                origin: "https://example.com:8443",
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "unparseable host falls back to uri authority",
                uri: "https://example.com/path",
                host: Some("not a valid authority"),
                origin: "https://example.com",
                result: Ok(()),
            },
            Test {
                name: "missing host, uri carries authority (match)",
                uri: "https://example.com/path",
                host: None,
                origin: "https://example.com",
                result: Ok(()),
            },
            Test {
                name: "missing host, uri authority mismatch",
                uri: "https://other.example/path",
                host: None,
                origin: "https://example.com",
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "missing host and no uri authority",
                uri: "/path",
                host: None,
                origin: "https://example.com",
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "scheme-less origin does not match host even if bytes agree",
                uri: "/",
                host: Some("example.com:8443"),
                origin: "example.com:8443",
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "non-http origin scheme does not enter host fallback",
                uri: "/",
                host: Some("example.com:8443"),
                origin: "ftp://example.com:8443",
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
        ];

        for test in tests {
            let mut req = Request::builder().method(Method::POST).uri(test.uri);

            if let Some(host) = test.host {
                req = req.header("host", host);
            }

            let req = req.header("origin", test.origin).body(()).unwrap();

            assert_eq!(middleware.verify(&req), test.result, "{}", test.name);
        }
    }

    #[test]
    fn test_middleware_sec_fetch_site() {
        let middleware: Csrf<()> = Default::default();

        const NON_DECODABLE: &[u8] = &[0xFF, 0xFE];
        assert!(
            http::HeaderValue::from_bytes(NON_DECODABLE)
                .expect("NON_DECODABLE must be a valid HeaderValue")
                .to_str()
                .is_err(),
            "NON_DECODABLE must fail HeaderValue::to_str()"
        );

        struct Test {
            name: &'static str,
            method: http::Method,
            sec_fetch_site: Option<&'static [u8]>,
            origin: Option<&'static [u8]>,
            result: Result<(), ProtectionError>,
        }

        let tests = [
            Test {
                name: "same-origin allowed",
                method: Method::GET,
                sec_fetch_site: Some(b"same-origin"),
                origin: None,
                result: Ok(()),
            },
            Test {
                name: "none allowed",
                method: Method::POST,
                sec_fetch_site: Some(b"none"),
                origin: None,
                result: Ok(()),
            },
            Test {
                name: "cross-site blocked",
                method: Method::POST,
                sec_fetch_site: Some(b"cross-site"),
                origin: None,
                result: Err(ProtectionError::CrossOriginRequest),
            },
            Test {
                name: "same-site blocked",
                method: Method::POST,
                sec_fetch_site: Some(b"same-site"),
                origin: None,
                result: Err(ProtectionError::CrossOriginRequest),
            },
            Test {
                name: "no header with no origin",
                method: Method::POST,
                sec_fetch_site: None,
                origin: None,
                result: Ok(()),
            },
            Test {
                name: "no header with matching origin",
                method: Method::POST,
                sec_fetch_site: None,
                origin: Some(b"https://example.com"),
                result: Ok(()),
            },
            Test {
                name: "no header with mismatched origin",
                method: Method::POST,
                sec_fetch_site: None,
                origin: Some(b"https://attacker.example"),
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "no header with null origin",
                method: Method::POST,
                sec_fetch_site: None,
                origin: Some(b"null"),
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "GET allowed",
                method: Method::GET,
                sec_fetch_site: Some(b"cross-site"),
                origin: None,
                result: Ok(()),
            },
            Test {
                name: "HEAD allowed",
                method: Method::HEAD,
                sec_fetch_site: Some(b"cross-site"),
                origin: None,
                result: Ok(()),
            },
            Test {
                name: "OPTIONS allowed",
                method: Method::OPTIONS,
                sec_fetch_site: Some(b"cross-site"),
                origin: None,
                result: Ok(()),
            },
            Test {
                name: "PUT blocked",
                method: Method::PUT,
                sec_fetch_site: Some(b"cross-site"),
                origin: None,
                result: Err(ProtectionError::CrossOriginRequest),
            },
            Test {
                name: "non-decodable origin without sec-fetch-site rejected",
                method: Method::POST,
                sec_fetch_site: None,
                origin: Some(NON_DECODABLE),
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "non-decodable sec-fetch-site without origin rejected",
                method: Method::POST,
                sec_fetch_site: Some(NON_DECODABLE),
                origin: None,
                result: Err(ProtectionError::CrossOriginRequest),
            },
            Test {
                name: "empty sec-fetch-site without origin allowed",
                method: Method::POST,
                sec_fetch_site: Some(b""),
                origin: None,
                result: Ok(()),
            },
            Test {
                name: "empty origin without sec-fetch-site allowed",
                method: Method::POST,
                sec_fetch_site: None,
                origin: Some(b""),
                result: Ok(()),
            },
        ];

        for test in tests {
            let mut req = Request::builder()
                .method(test.method)
                .header("host", "example.com");

            if let Some(sec_fetch_site) = test.sec_fetch_site {
                req = req.header("sec-fetch-site", sec_fetch_site);
            }

            if let Some(origin) = test.origin {
                req = req.header("origin", origin);
            }

            let req = req.body(()).unwrap();

            assert_eq!(middleware.verify(&req), test.result, "{}", test.name);
        }
    }

    #[test]
    fn test_middleware_trusted_origin_bypass() {
        let layer = CsrfLayer::new()
            .add_trusted_origin("https://trusted.example")
            .unwrap();

        let middleware = layer.layer(());

        struct Test {
            name: &'static str,
            sec_fetch_site: Option<&'static str>,
            origin: Option<&'static str>,
            result: Result<(), ProtectionError>,
        }

        let tests = [
            Test {
                name: "trusted origin without sec-fetch-site",
                origin: Some("https://trusted.example"),
                sec_fetch_site: None,
                result: Ok(()),
            },
            Test {
                name: "trusted origin with cross-site",
                origin: Some("https://trusted.example"),
                sec_fetch_site: Some("cross-site"),
                result: Ok(()),
            },
            Test {
                name: "untrusted origin without sec-fetch-site",
                origin: Some("https://attacker.example"),
                sec_fetch_site: None,
                result: Err(ProtectionError::CrossOriginRequestFromOldBrowser),
            },
            Test {
                name: "untrusted origin with cross-site",
                origin: Some("https://attacker.example"),
                sec_fetch_site: Some("cross-site"),
                result: Err(ProtectionError::CrossOriginRequest),
            },
        ];

        for test in tests {
            let mut req = Request::builder()
                .method("POST")
                .header("host", "example.com");

            if let Some(sec_fetch_site) = test.sec_fetch_site {
                req = req.header("sec-fetch-site", sec_fetch_site);
            }

            if let Some(origin) = test.origin {
                req = req.header("origin", origin);
            }

            let req = req.body(()).unwrap();

            assert_eq!(middleware.verify(&req), test.result, "{}", test.name);
        }
    }

    #[test]
    fn test_middleware_trusted_origin_normalization() {
        // Each row inserts a non-canonical form of the trusted origin and
        // sends a request bearing the canonical browser-style Origin header;
        // the test fails if canonicalization is dropped on either side.
        struct Test {
            name: &'static str,
            trusted: &'static str,
            origin: &'static str,
        }

        let tests = [
            Test {
                name: "scheme and host case folded",
                trusted: "HTTPS://Example.COM",
                origin: "https://example.com",
            },
            Test {
                name: "trailing slash stripped",
                trusted: "https://example.com/",
                origin: "https://example.com",
            },
            Test {
                name: "default https port stripped",
                trusted: "https://example.com:443",
                origin: "https://example.com",
            },
            Test {
                name: "default http port stripped",
                trusted: "http://example.com:80",
                origin: "http://example.com",
            },
            Test {
                name: "non-default port preserved",
                trusted: "https://example.com:8443",
                origin: "https://example.com:8443",
            },
        ];

        for test in tests {
            let middleware = CsrfLayer::new()
                .add_trusted_origin(test.trusted)
                .unwrap_or_else(|e| panic!("{}: add_trusted_origin failed: {e}", test.name))
                .layer(());

            let req = Request::builder()
                .method("POST")
                .header("host", "other.example")
                .header("origin", test.origin)
                .header("sec-fetch-site", "cross-site")
                .body(())
                .unwrap();

            assert_eq!(middleware.verify(&req), Ok(()), "{}", test.name);
        }
    }
}
