use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;

use http::{Method, Uri};
use tower_layer::Layer;

use super::service::Csrf;
use super::url::UriExt;
use super::{BypassFn, ConfigError, DebugFn, DefaultResponseForProtectionError, Origins};

/// Layer that applies the [`Csrf`] middleware.
///
/// See the [module docs](crate::csrf) for an example.
#[derive(Clone)]
#[must_use]
pub struct CsrfLayer<T = DefaultResponseForProtectionError> {
    insecure_bypass: Option<Arc<BypassFn>>,
    rejection_response: T,
    trusted_origins: Origins,
}

impl Default for CsrfLayer {
    fn default() -> Self {
        Self {
            insecure_bypass: None,
            rejection_response: DefaultResponseForProtectionError,
            trusted_origins: Origins::default(),
        }
    }
}

impl<T> Debug for CsrfLayer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("CsrfLayer")
            .field(
                "insecure_bypass",
                &self.insecure_bypass.as_ref().map(|_| DebugFn),
            )
            .field("trusted_origins", &self.trusted_origins)
            .field("rejection_response", &DebugFn)
            .finish()
    }
}

impl CsrfLayer {
    /// Creates a new `CsrfLayer` with no trusted origins, no bypass, and the
    /// default rejection response.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T> CsrfLayer<T> {
    /// Adds a trusted origin that allows all requests whose `Origin` header
    /// matches the given value.
    ///
    /// The value is matched **byte-for-byte** against the request's `Origin`
    /// header — there is no normalization (this mirrors the Go reference). It
    /// must therefore be written exactly as a browser sends it:
    ///
    /// - form `scheme://host[:port]`, where `scheme` is `http` or `https`;
    /// - the host lowercased (browsers lowercase it; IDN hosts must be given in
    ///   punycode, e.g. `xn--exmple-cua.com`);
    /// - **default ports omitted** — browsers drop `:80`/`:443`, so an explicit
    ///   default port (e.g. `https://example.com:443`) will never match;
    /// - **no trailing slash**, path, query, or fragment.
    ///
    /// Inputs that can't represent a browser `Origin` are rejected with a
    /// [`ConfigError`]; inputs that parse but aren't in the canonical browser
    /// form above are accepted but will silently never match.
    ///
    /// ```
    /// # use tower_http::csrf::CsrfLayer;
    /// // Matches `Origin: https://example.com`:
    /// let layer = CsrfLayer::new().add_trusted_origin("https://example.com")?;
    ///
    /// // Accepted, but never matches a browser Origin (explicit default port):
    /// let layer = CsrfLayer::new().add_trusted_origin("https://example.com:443")?;
    /// # Ok::<_, tower_http::csrf::ConfigError>(())
    /// ```
    pub fn add_trusted_origin<S: AsRef<str>>(mut self, origin: S) -> Result<Self, ConfigError> {
        let origin = origin.as_ref();

        // validate the form; the origin is stored and matched verbatim.
        Uri::parse_origin(origin)?;

        #[cfg(feature = "tracing")]
        tracing::debug!(origin = %origin, "added trusted origin");

        self.trusted_origins.insert(origin.to_owned());

        Ok(self)
    }

    /// Adds a bypass predicate that returns `true` for requests which should
    /// skip CSRF protection.
    ///
    /// This is an escape hatch for endpoints that legitimately need to accept
    /// cross-origin POSTs (e.g. webhook receivers). Bypassed endpoints must
    /// have their own protection (signed payloads, authentication tokens,
    /// etc.) — otherwise they are CSRF-vulnerable.
    pub fn with_insecure_bypass<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&Method, &Uri) -> bool + Send + Sync + 'static,
    {
        #[cfg(feature = "tracing")]
        tracing::debug!("added insecure bypass");

        self.insecure_bypass = Some(Arc::new(predicate));
        self
    }

    /// Replaces the response builder used when a request is rejected.
    ///
    /// Accepts any type that implements [`ResponseForProtectionError`](super::ResponseForProtectionError),
    /// including a `FnMut(ProtectionError) -> Response<B> + Clone` closure.
    /// The default builder returns a `403 Forbidden` with an empty body and
    /// the [`ProtectionError`](super::ProtectionError) attached to the
    /// response's extensions.
    pub fn with_rejection_response<R>(self, rejection_response: R) -> CsrfLayer<R> {
        CsrfLayer {
            insecure_bypass: self.insecure_bypass,
            trusted_origins: self.trusted_origins,
            rejection_response,
        }
    }
}

impl<S, T> Layer<S> for CsrfLayer<T>
where
    T: Clone,
{
    type Service = Csrf<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        Csrf::new(
            inner,
            self.insecure_bypass.clone(),
            self.rejection_response.clone(),
            self.trusted_origins.clone(),
        )
    }
}
