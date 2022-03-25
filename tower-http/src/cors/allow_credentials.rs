use std::{fmt, sync::Arc};

use http::{request::Parts as RequestParts, HeaderValue};

/// Holds configuration for how to set the [`Access-Control-Allow-Credentials`][mdn] header.
///
/// See [`CorsLayer::allow_credentials`] for more details.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Credentials
/// [`CorsLayer::allow_credentials`]: super::CorsLayer::allow_credentials
#[derive(Clone, Default)]
pub struct AllowCredentials(AllowCredentialsInner);

impl AllowCredentials {
    /// Allow credentials for all requests
    ///
    /// See [`CorsLayer::allow_credentials`] for more details.
    ///
    /// [`CorsLayer::allow_credentials`]: super::CorsLayer::allow_credentials
    pub fn yes() -> Self {
        Self(AllowCredentialsInner::Yes)
    }

    /// Allow credentials for some requests, based on a given predicate
    ///
    /// See [`CorsLayer::allow_credentials`] for more details.
    ///
    /// [`CorsLayer::allow_credentials`]: super::CorsLayer::allow_credentials
    pub fn predicate<F>(f: F) -> Self
    where
        F: Fn(&HeaderValue, &RequestParts) -> bool + Send + Sync + 'static,
    {
        Self(AllowCredentialsInner::Predicate(Arc::new(f)))
    }

    pub(super) fn to_header_val(
        &self,
        origin: &HeaderValue,
        parts: &RequestParts,
    ) -> Option<HeaderValue> {
        #[allow(clippy::declare_interior_mutable_const)]
        const TRUE: HeaderValue = HeaderValue::from_static("true");

        let allow_creds = match &self.0 {
            AllowCredentialsInner::Yes => true,
            AllowCredentialsInner::No => false,
            AllowCredentialsInner::Predicate(c) => c(origin, parts),
        };

        allow_creds.then(|| TRUE)
    }
}

impl From<bool> for AllowCredentials {
    fn from(v: bool) -> Self {
        match v {
            true => Self(AllowCredentialsInner::Yes),
            false => Self(AllowCredentialsInner::No),
        }
    }
}

impl fmt::Debug for AllowCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            AllowCredentialsInner::Yes => f.debug_tuple("Yes").finish(),
            AllowCredentialsInner::No => f.debug_tuple("No").finish(),
            AllowCredentialsInner::Predicate(_) => f.debug_tuple("Predicate").finish(),
        }
    }
}

#[derive(Clone)]
enum AllowCredentialsInner {
    Yes,
    No,
    Predicate(
        Arc<dyn for<'a> Fn(&'a HeaderValue, &'a RequestParts) -> bool + Send + Sync + 'static>,
    ),
}

impl Default for AllowCredentialsInner {
    fn default() -> Self {
        Self::No
    }
}
