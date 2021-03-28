use http::Request;
use tracing::Span;

/// Trait used to generate [`Span`]s from requests. [`Trace`] wraps all request handling in this
/// span.
///
/// [`Span`]: tracing::Span
/// [`Trace`]: super::Trace
pub trait MakeSpan<B> {
    /// Make a span from a request.
    fn make_span(&mut self, request: &Request<B>) -> Span;
}

impl<B> MakeSpan<B> for Span {
    fn make_span(&mut self, _request: &Request<B>) -> Span {
        self.clone()
    }
}

impl<F, B> MakeSpan<B> for F
where
    F: FnMut(&Request<B>) -> Span,
{
    fn make_span(&mut self, request: &Request<B>) -> Span {
        self(request)
    }
}

/// The default way [`Span`]s will be created for [`Trace`].
///
/// [`Span`]: tracing::Span
/// [`Trace`]: super::Trace
#[derive(Debug, Clone, Default)]
pub struct DefaultMakeSpan {
    include_headers: bool,
}

impl DefaultMakeSpan {
    /// Create a new `DefaultMakeSpan`.
    pub fn new() -> Self {
        Self {
            include_headers: false,
        }
    }

    /// Include request headers on the [`Span`].
    ///
    /// By default headers are not included.
    ///
    /// [`Span`]: tracing::Span
    pub fn include_headers(mut self, include_headers: bool) -> Self {
        self.include_headers = include_headers;
        self
    }
}

impl<B> MakeSpan<B> for DefaultMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        if self.include_headers {
            tracing::debug_span!(
                "request",
                method = %request.method(),
                uri = %request.uri(),
                version = ?request.version(),
                headers = ?request.headers(),
            )
        } else {
            tracing::debug_span!(
                "request",
                method = %request.method(),
                uri = %request.uri(),
                version = ?request.version(),
            )
        }
    }
}
