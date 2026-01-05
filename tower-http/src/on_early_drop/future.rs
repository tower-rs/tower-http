//! Future implementation for the OnEarlyDrop middleware.

use crate::on_early_drop::guard::OnEarlyDropGuard;
use pin_project_lite::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pin_project! {
    /// Response future for [`OnEarlyDropService`].
    ///
    /// This future wraps an inner service future and ensures that the
    /// early drop guard is  executed whenever the future does not
    /// complete successfully
    ///
    /// # Type Parameters
    ///
    /// * `Future` - The inner future type produced by the wrapped service
    /// * `Callback` - The callback type, a function that will be executed if a request is dropped early
    ///
    /// [`OnEarlyDropService`]: super::service::OnEarlyDropService
    pub struct OnEarlyDropFuture<Future, Callback: FnOnce()> {
        #[pin]
        inner: Future,
        guard: Option<OnEarlyDropGuard<Callback>>,
    }
}

impl<Future, Callback: FnOnce()> OnEarlyDropFuture<Future, Callback> {
    /// Creates a new `OnEarlyDropFuture` with the given inner future and guard.
    pub(crate) fn new(inner: Future, guard: OnEarlyDropGuard<Callback>) -> Self {
        Self {
            inner,
            guard: Some(guard),
        }
    }
}

/// Implementation of `Future` for `OnEarlyDropFuture`.
///
/// # Type Parameters
///
/// * `InnerFuture` - The inner future type produced by the wrapped service
/// * `Callback` - The callback type, a function that will be executed if a request is dropped early
/// * `Error` - The error type that might be returned by the inner future
/// * `Response` - The response type returned by the inner future
impl<InnerFuture, Callback, Error, Response> Future for OnEarlyDropFuture<InnerFuture, Callback>
where
    InnerFuture: Future<Output = Result<Response, Error>>,
    Callback: FnOnce(),
{
    type Output = Result<Response, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        // Poll the inner future
        let result = match this.inner.poll(cx) {
            Poll::Ready(result) => result,
            Poll::Pending => return Poll::Pending,
        };

        // Mark the guard as completed since we've successfully completed the future
        if let Some(guard) = this.guard.take() {
            let mut guard = guard;
            guard.completed();
            // Guard will be dropped here, but won't execute the callback
            // since we've marked it as completed
        }

        Poll::Ready(result)
    }
}
