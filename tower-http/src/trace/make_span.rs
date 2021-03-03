use http::Request;
use tracing::{field::Empty, Span};

pub trait MakeSpan {
    fn make_span<B>(&mut self, request: &Request<B>) -> Span;
}

impl MakeSpan for Span {
    fn make_span<B>(&mut self, _request: &Request<B>) -> Span {
        self.clone()
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

impl MakeSpan for DefaultMakeSpan {
    fn make_span<B>(&mut self, request: &Request<B>) -> Span {
        tracing::debug_span!(
            "request",
            method = %request.method(),
            path = request.uri().path(),
            headers = Empty,
        )
    }
}
