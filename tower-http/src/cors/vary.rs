use std::iter;

use http::header::{self, HeaderName, HeaderValue};

use crate::cors::preflight_request_headers;

/// Holds configuration for how to set the [`Vary`][mdn] header.
///
/// See [`CorsLayer::vary`] for more details.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Vary
/// [`CorsLayer::vary`]: super::CorsLayer::vary
#[derive(Clone, Debug)]
pub struct Vary(Option<HeaderValue>);

impl Vary {
    /// Set the list of header names to return as vary header values
    ///
    /// See [`CorsLayer::vary`] for more details.
    ///
    /// [`CorsLayer::vary`]: super::CorsLayer::vary
    pub fn list<I>(headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        // The list is joined once here rather than for every response.
        let mut headers = headers.into_iter();
        let value = headers.next().map(|first| match headers.next() {
            // A single name keeps its bytes, which are static for standard headers.
            None => first.into(),
            Some(second) => {
                let mut res = first.as_str().as_bytes().to_owned();
                for name in iter::once(second).chain(headers) {
                    res.extend_from_slice(b", ");
                    res.extend_from_slice(name.as_str().as_bytes());
                }
                HeaderValue::from_bytes(&res)
                    .expect("comma-separated list of HeaderNames is always a valid HeaderValue")
            }
        });
        Self(value)
    }

    pub(super) fn to_header(&self) -> Option<(HeaderName, HeaderValue)> {
        self.0.clone().map(|value| (header::VARY, value))
    }
}

impl Default for Vary {
    fn default() -> Self {
        Self::list(preflight_request_headers())
    }
}

impl<const N: usize> From<[HeaderName; N]> for Vary {
    fn from(arr: [HeaderName; N]) -> Self {
        Self::list(arr)
    }
}

impl From<Vec<HeaderName>> for Vary {
    fn from(vec: Vec<HeaderName>) -> Self {
        Self::list(vec)
    }
}
