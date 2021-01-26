use crate::common::*;
use tracing::{
    instrument::{Instrument, Instrumented},
    Span,
};

#[derive(Clone, Debug)]
pub struct WrapInSpanLayer {
    span: Span,
}

impl WrapInSpanLayer {
    pub fn new(span: Span) -> Self {
        Self { span }
    }
}

impl<S> Layer<S> for WrapInSpanLayer {
    type Service = WrapInSpan<S>;

    fn layer(&self, inner: S) -> Self::Service {
        WrapInSpan {
            inner,
            span: self.span.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct WrapInSpan<S> {
    inner: S,
    span: Span,
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for WrapInSpan<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Instrumented<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let future = {
            let _guard = self.span.enter();
            self.inner.call(req)
        };
        future.instrument(self.span.clone())
    }
}
