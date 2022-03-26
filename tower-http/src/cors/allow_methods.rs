use std::{array, fmt};

use http::{header, request::Parts as RequestParts, HeaderValue, Method};

use super::{separated_by_commas, Any, WILDCARD};

/// Holds configuration for how to set the [`Access-Control-Allow-Methods`][mdn] header.
///
/// See [`CorsLayer::allow_methods`] for more details.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Methods
/// [`CorsLayer::allow_methods`]: super::CorsLayer::allow_methods
#[derive(Clone, Default)]
#[must_use]
pub struct AllowMethods(AllowMethodsInner);

impl AllowMethods {
    /// Allow any method by sending a wildcard (`*`)
    ///
    /// See [`CorsLayer::allow_methods`] for more details.
    ///
    /// [`CorsLayer::allow_methods`]: super::CorsLayer::allow_methods
    pub fn any() -> Self {
        Self(AllowMethodsInner::Const(Some(WILDCARD)))
    }

    /// Set a single allowed method
    ///
    /// See [`CorsLayer::allow_methods`] for more details.
    ///
    /// [`CorsLayer::allow_methods`]: super::CorsLayer::allow_methods
    pub fn exact(method: Method) -> Self {
        Self(AllowMethodsInner::Const(Some(
            HeaderValue::from_str(method.as_str()).unwrap(),
        )))
    }

    /// Set multiple allowed methods
    ///
    /// See [`CorsLayer::allow_methods`] for more details.
    ///
    /// [`CorsLayer::allow_methods`]: super::CorsLayer::allow_methods
    pub fn list<I>(methods: I) -> Self
    where
        I: IntoIterator<Item = Method>,
    {
        Self(AllowMethodsInner::Const(separated_by_commas(
            methods
                .into_iter()
                .map(|m| HeaderValue::from_str(m.as_str()).unwrap()),
        )))
    }

    /// Allow any method, by mirroring the preflight [`Access-Control-Request-Method`][mdn]
    /// header.
    ///
    /// See [`CorsLayer::allow_methods`] for more details.
    ///
    /// [`CorsLayer::allow_methods`]: super::CorsLayer::allow_methods
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Request-Method
    pub fn mirror_request() -> Self {
        Self(AllowMethodsInner::MirrorRequest)
    }

    pub(super) fn to_header_val(&self, parts: &RequestParts) -> Option<HeaderValue> {
        match &self.0 {
            AllowMethodsInner::Const(v) => v.clone(),
            AllowMethodsInner::MirrorRequest => parts
                .headers
                .get(header::ACCESS_CONTROL_REQUEST_METHOD)
                .cloned(),
        }
    }
}

impl fmt::Debug for AllowMethods {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            AllowMethodsInner::Const(inner) => f.debug_tuple("Const").field(inner).finish(),
            AllowMethodsInner::MirrorRequest => f.debug_tuple("MirrorRequest").finish(),
        }
    }
}

impl From<Any> for AllowMethods {
    fn from(_: Any) -> Self {
        Self::any()
    }
}

impl From<Method> for AllowMethods {
    fn from(method: Method) -> Self {
        Self::exact(method)
    }
}

impl<const N: usize> From<[Method; N]> for AllowMethods {
    fn from(arr: [Method; N]) -> Self {
        #[allow(deprecated)] // Can be changed when MSRV >= 1.53
        Self::list(array::IntoIter::new(arr))
    }
}

impl From<Vec<Method>> for AllowMethods {
    fn from(vec: Vec<Method>) -> Self {
        Self::list(vec)
    }
}

#[derive(Clone)]
enum AllowMethodsInner {
    Const(Option<HeaderValue>),
    MirrorRequest,
}

impl Default for AllowMethodsInner {
    fn default() -> Self {
        Self::Const(None)
    }
}
