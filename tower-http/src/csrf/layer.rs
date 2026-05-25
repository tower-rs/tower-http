use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;

use http::{Method, Uri};
use tower_layer::Layer;

use super::service::Csrf;
use super::url::TrustedOrigin;
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
    /// Adds a trusted origin that allows all requests with an `Origin` header
    /// exactly matching the given value.
    ///
    /// Origin values are of the form `scheme://host[:port]`. Default ports
    /// (`80` for `http`, `443` for `https`) and casing are normalized; paths,
    /// queries, and fragments are rejected.
    pub fn add_trusted_origin<S: AsRef<str>>(mut self, origin: S) -> Result<Self, ConfigError> {
        let normalized = TrustedOrigin::parse(origin.as_ref())?.into_canonical();

        #[cfg(feature = "tracing")]
        tracing::debug!(origin = %normalized, "added trusted origin");

        self.trusted_origins.insert(normalized);

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
