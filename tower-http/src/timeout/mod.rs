//! Middleware that adds timeouts to request or response bodies.
//!
//! Note these middleware differ from [`tower::timeout::Timeout`] which only
//! adds a timeout to the response future and doesn't consider request bodies.

use tower::BoxError;
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::time::Sleep;

pub mod request_body;
pub mod response_body;

#[doc(inline)]
pub use self::{
    request_body::{RequestBodyTimeout, RequestBodyTimeoutLayer},
    response_body::{ResponseBodyTimeout, ResponseBodyTimeoutLayer},
};

/// An HTTP body with a timeout applied.
#[pin_project]
#[derive(Debug)]
pub struct TimeoutBody<B> {
    #[pin]
    inner: B,
    #[pin]
    state: State,
}

impl<B> TimeoutBody<B> {
    pub(crate) fn new(inner: B, timeout: Duration) -> Self {
        Self {
            inner,
            state: State::NotPolled(timeout),
        }
    }
}

// Only start the timeout after first poll of the body. This enum manages that.
#[allow(clippy::large_enum_variant)]
#[pin_project(project = StateProj)]
#[derive(Debug)]
enum State {
    NotPolled(Duration),
    SleepPending(#[pin] Sleep),
}

impl Future for State {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let new_state = match self.as_mut().project() {
                StateProj::NotPolled(timeout) => State::SleepPending(tokio::time::sleep(*timeout)),
                StateProj::SleepPending(sleep) => return sleep.poll(cx),
            };
            self.set(new_state);
        }
    }
}

impl<B> Body for TimeoutBody<B>
where
    B: Body,
    B::Error: Into<BoxError>,
{
    type Data = B::Data;
    type Error = BoxError;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();

        if let Poll::Ready(chunk) = this.inner.poll_data(cx) {
            let chunk = chunk.map(|chunk| chunk.map_err(Into::into));
            return Poll::Ready(chunk);
        }

        if this.state.poll(cx).is_ready() {
            let err = tower::timeout::error::Elapsed::new().into();
            return Poll::Ready(Some(Err(err)));
        }

        Poll::Pending
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let this = self.project();

        if let Poll::Ready(trailers) = this.inner.poll_trailers(cx) {
            let trailers = trailers.map_err(Into::into);
            return Poll::Ready(trailers);
        }

        if this.state.poll(cx).is_ready() {
            let err = tower::timeout::error::Elapsed::new().into();
            return Poll::Ready(Err(err));
        }

        Poll::Pending
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}
