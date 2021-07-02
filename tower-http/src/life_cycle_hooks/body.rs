use super::{Callbacks, FailedAt};
use crate::classify::ClassifyEos;
use futures_core::ready;
use http_body::Body;
use pin_project::pin_project;
use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

/// Response body for [`LifeCycleHooks`].
///
/// [`LifeCycleHooks`]: crate::life_cycle_hooks::LifeCycleHooks
#[pin_project]
pub struct ResponseBody<B, C, Callbacks, CallbacksData> {
    #[pin]
    pub(super) inner: B,
    pub(super) parts: Option<(C, Callbacks, CallbacksData)>,
}

impl<B, C, CallbacksT, CallbacksData> Body for ResponseBody<B, C, CallbacksT, CallbacksData>
where
    B: Body,
    B::Error: fmt::Display + 'static,
    C: ClassifyEos,
    CallbacksT: Callbacks<C::FailureClass, Data = CallbacksData>,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();

        let result = ready!(this.inner.poll_data(cx));

        match result {
            None => Poll::Ready(None),
            Some(Ok(chunk)) => {
                if let Some((_, callbacks, callbacks_data)) = &this.parts {
                    callbacks.on_body_chunk(&chunk, callbacks_data);
                }

                Poll::Ready(Some(Ok(chunk)))
            }
            Some(Err(err)) => {
                if let Some((classify_eos, callbacks, callbacks_data)) = this.parts.take() {
                    let classification = classify_eos.classify_error(&err);
                    callbacks.on_failure(FailedAt::Body, classification, callbacks_data);
                }

                Poll::Ready(Some(Err(err)))
            }
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let this = self.project();

        let result = ready!(this.inner.poll_trailers(cx));

        match result {
            Ok(trailers) => {
                if let Some((classify_eos, callbacks, callbacks_data)) = this.parts.take() {
                    let trailers = trailers.as_ref();
                    let classification = classify_eos.classify_eos(trailers);
                    callbacks.on_eos(trailers, classification, callbacks_data);
                }

                Poll::Ready(Ok(trailers))
            }
            Err(err) => {
                if let Some((classify_eos, callbacks, callbacks_data)) = this.parts.take() {
                    let classification = classify_eos.classify_error(&err);
                    callbacks.on_failure(FailedAt::Trailers, classification, callbacks_data);
                }

                Poll::Ready(Err(err))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}
