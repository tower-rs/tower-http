use crate::common::*;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct DebugEnterLeaveLayer<L, S> {
    inner: L,
    name: String,
    _marker: PhantomData<fn() -> S>,
}

impl<L, S> DebugEnterLeaveLayer<L, S> {
    pub(crate) fn new(inner: L, name: &str) -> Self {
        Self {
            inner,
            name: name.to_string(),
            _marker: PhantomData,
        }
    }
}

impl<S, L> Layer<S> for DebugEnterLeaveLayer<L, S>
where
    L: Layer<S>,
{
    type Service = DebugEnterLeave<L::Service>;

    fn layer(&self, inner: S) -> Self::Service {
        DebugEnterLeave {
            inner: self.inner.layer(inner),
            name: self.name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DebugEnterLeave<S> {
    inner: S,
    name: String,
}

impl<R, S> Service<R> for DebugEnterLeave<S>
where
    S: Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = DebugEnterLeaveResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        let start = Instant::now();
        tracing::debug!(message = %format!("entering {}", self.name));

        DebugEnterLeaveResponseFuture {
            future: self.inner.call(req),
            name: self.name.clone(),
            start,
        }
    }
}

#[pin_project]
#[derive(Debug)]
pub struct DebugEnterLeaveResponseFuture<F> {
    #[pin]
    future: F,
    name: String,
    start: Instant,
}

impl<F> Future for DebugEnterLeaveResponseFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = ready!(this.future.poll(cx));
        tracing::debug!(
            message = %format!("leaving {}", this.name),
            duration = ?this.start.elapsed(),
        );
        Poll::Ready(result)
    }
}
