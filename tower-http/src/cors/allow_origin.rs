use std::{array, fmt, sync::Arc};

use http::{request::Parts as RequestParts, HeaderValue};

use super::{separated_by_commas, Any, WILDCARD};

/// Holds configuration for how to set the [`Access-Control-Allow-Origin`][mdn] header.
///
/// See [`CorsLayer::allow_origin`] for more details.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin
/// [`CorsLayer::allow_origin`]: super::CorsLayer::allow_origin
#[derive(Clone, Default)]
pub struct AllowOrigin(OriginInner);

impl AllowOrigin {
    /// Allow any origin by sending a wildcard (`*`)
    ///
    /// See [`CorsLayer::allow_origin`] for more details.
    ///
    /// [`CorsLayer::allow_origin`]: super::CorsLayer::allow_origin
    pub fn any() -> Self {
        Self(OriginInner::Const(Some(WILDCARD)))
    }

    /// Set a single allowed origin
    ///
    /// See [`CorsLayer::allow_origin`] for more details.
    ///
    /// [`CorsLayer::allow_origin`]: super::CorsLayer::allow_origin
    pub fn exact(origin: HeaderValue) -> Self {
        Self(OriginInner::Const(Some(origin)))
    }

    /// Set multiple allowed origins
    ///
    /// See [`CorsLayer::allow_origin`] for more details.
    ///
    /// [`CorsLayer::allow_origin`]: super::CorsLayer::allow_origin
    pub fn list<I>(origins: I) -> Self
    where
        I: IntoIterator<Item = HeaderValue>,
    {
        Self(OriginInner::Const(separated_by_commas(
            origins.into_iter().map(Into::into),
        )))
    }

    /// Set the allowed origins from a predicate
    ///
    /// See [`CorsLayer::allow_origin`] for more details.
    ///
    /// [`CorsLayer::allow_origin`]: super::CorsLayer::allow_origin
    pub fn predicate<F>(f: F) -> Self
    where
        F: Fn(&HeaderValue, &RequestParts) -> bool + Send + Sync + 'static,
    {
        Self(OriginInner::Predicate(Arc::new(f)))
    }

    /// Allow any origin, by mirroring the request origin
    ///
    /// This is equivalent to
    /// [`AllowOrigin::predicate(|_, _| true)`][Self::predicate].
    ///
    /// See [`CorsLayer::allow_origin`] for more details.
    ///
    /// [`CorsLayer::allow_origin`]: super::CorsLayer::allow_origin
    pub fn mirror_request() -> Self {
        Self::predicate(|_, _| true)
    }

    pub(super) fn to_header_val(
        &self,
        origin: &HeaderValue,
        parts: &RequestParts,
    ) -> Option<HeaderValue> {
        match &self.0 {
            OriginInner::Const(v) => v.clone(),
            OriginInner::Predicate(c) => c(origin, parts).then(|| origin.to_owned()),
        }
    }
}

impl fmt::Debug for AllowOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            OriginInner::Const(inner) => f.debug_tuple("Const").field(inner).finish(),
            OriginInner::Predicate(_) => f.debug_tuple("Predicate").finish(),
        }
    }
}

impl From<Any> for AllowOrigin {
    fn from(_: Any) -> Self {
        Self::any()
    }
}

impl From<HeaderValue> for AllowOrigin {
    fn from(val: HeaderValue) -> Self {
        Self::exact(val)
    }
}

impl<const N: usize> From<[HeaderValue; N]> for AllowOrigin {
    fn from(arr: [HeaderValue; N]) -> Self {
        #[allow(deprecated)] // Can be changed when MSRV >= 1.53
        Self::list(array::IntoIter::new(arr))
    }
}

impl From<Vec<HeaderValue>> for AllowOrigin {
    fn from(vec: Vec<HeaderValue>) -> Self {
        Self::list(vec)
    }
}

#[derive(Clone)]
enum OriginInner {
    Const(Option<HeaderValue>),
    Predicate(
        Arc<dyn for<'a> Fn(&'a HeaderValue, &'a RequestParts) -> bool + Send + Sync + 'static>,
    ),
}

impl Default for OriginInner {
    fn default() -> Self {
        Self::Const(None)
    }
}
