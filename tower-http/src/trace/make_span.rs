use http::Request;
use tracing::{field::Empty, Span};

pub trait MakeSpan<B> {
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

#[derive(Debug, Clone, Default)]
pub struct DefaultMakeSpan {
    _priv: (),
}

impl DefaultMakeSpan {
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl<B> MakeSpan<B> for DefaultMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        tracing::debug_span!(
            "request",
            method = %request.method(),
            path = request.uri().path(),
            headers = Empty,
        )
    }
}
