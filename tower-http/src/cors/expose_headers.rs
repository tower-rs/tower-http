use std::{array, fmt};

use http::{
    header::{HeaderName, HeaderValue},
    request::Parts as RequestParts,
};

use super::{separated_by_commas, Any, WILDCARD};

/// Holds configuration for how to set the [`Access-Control-Expose-Headers`][mdn] header.
///
/// See [`CorsLayer::expose_headers`] for more details.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Expose-Headers
/// [`CorsLayer::expose_headers`]: super::CorsLayer::expose_headers
#[derive(Clone, Default)]
pub struct ExposeHeaders(ExposeHeadersInner);

impl ExposeHeaders {
    /// Expose any / all headers by sending a wildcard (`*`)
    ///
    /// See [`CorsLayer::expose_headers`] for more details.
    ///
    /// [`CorsLayer::expose_headers`]: super::CorsLayer::expose_headers
    pub fn any() -> Self {
        Self(ExposeHeadersInner::Const(Some(WILDCARD)))
    }

    /// Set multiple exposed header names
    ///
    /// See [`CorsLayer::expose_headers`] for more details.
    ///
    /// [`CorsLayer::expose_headers`]: super::CorsLayer::expose_headers
    pub fn list<I>(headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        Self(ExposeHeadersInner::Const(separated_by_commas(
            headers.into_iter().map(Into::into),
        )))
    }

    pub(super) fn to_header_val(&self, _parts: &RequestParts) -> Option<HeaderValue> {
        match &self.0 {
            ExposeHeadersInner::Const(v) => v.clone(),
        }
    }
}

impl fmt::Debug for ExposeHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            ExposeHeadersInner::Const(inner) => f.debug_tuple("Const").field(inner).finish(),
        }
    }
}

impl From<Any> for ExposeHeaders {
    fn from(_: Any) -> Self {
        Self::any()
    }
}

impl<const N: usize> From<[HeaderName; N]> for ExposeHeaders {
    fn from(arr: [HeaderName; N]) -> Self {
        #[allow(deprecated)] // Can be changed when MSRV >= 1.53
        Self::list(array::IntoIter::new(arr))
    }
}

impl From<Vec<HeaderName>> for ExposeHeaders {
    fn from(vec: Vec<HeaderName>) -> Self {
        Self::list(vec)
    }
}

#[derive(Clone)]
enum ExposeHeadersInner {
    Const(Option<HeaderValue>),
}

impl Default for ExposeHeadersInner {
    fn default() -> Self {
        ExposeHeadersInner::Const(None)
    }
}
