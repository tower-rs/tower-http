use super::{FailedAt, MetricsSink};
use crate::classify::ClassifyEos;
use futures_core::ready;
use http_body::Body;
use pin_project::pin_project;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

/// Response body for [`Traffic`].
#[pin_project]
pub struct ResponseBody<B, C, MetricsSink, SinkData> {
    #[pin]
    pub(super) inner: B,
    pub(super) parts: Option<(C, MetricsSink, SinkData)>,
}

impl<B, C, MetricsSinkT, SinkData> Body for ResponseBody<B, C, MetricsSinkT, SinkData>
where
    B: Body,
    C: ClassifyEos<B::Error>,
    MetricsSinkT: MetricsSink<C::FailureClass, Data = SinkData>,
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
            Some(Ok(chunk)) => Poll::Ready(Some(Ok(chunk))),
            Some(Err(err)) => {
                if let Some((classify_eos, sink, sink_data)) = this.parts.take() {
                    let classification = classify_eos.classify_error(&err);
                    sink.on_failure(FailedAt::Body, classification, sink_data);
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
                if let Some((classify_eos, sink, sink_data)) = this.parts.take() {
                    let trailers = trailers.as_ref();
                    let classification = classify_eos.classify_eos(trailers);
                    sink.on_eos(trailers, classification, sink_data);
                }

                Poll::Ready(Ok(trailers))
            }
            Err(err) => {
                if let Some((classify_eos, sink, sink_data)) = this.parts.take() {
                    let classification = classify_eos.classify_error(&err);
                    sink.on_failure(FailedAt::Body, classification, sink_data);
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
