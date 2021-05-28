use super::{FailedAt, MetricsSink, ResponseBody};
use crate::classify::{ClassifiedResponse, ClassifyResponse};
use futures_core::ready;
use http::Response;
use http_body::Body;
use pin_project::pin_project;
use std::{
    fmt,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

/// Response future for [`Traffic`].
///
/// [`Traffic`]: crate::metrics::Traffic
#[pin_project]
pub struct ResponseFuture<F, C, MetricsSink, SinkData> {
    #[pin]
    pub(super) inner: F,
    pub(super) classifier: Option<C>,
    pub(super) request_received_at: Instant,
    pub(super) sink: Option<MetricsSink>,
    pub(super) sink_data: Option<SinkData>,
}

impl<F, C, ResBody, E, MetricsSinkT, SinkData> Future
    for ResponseFuture<F, C, MetricsSinkT, SinkData>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Body,
    C: ClassifyResponse,
    MetricsSinkT: MetricsSink<C::FailureClass, Data = SinkData>,
    E: fmt::Display + 'static,
{
    type Output = Result<
        Response<ResponseBody<ResBody, C::ClassifyEos, MetricsSinkT, MetricsSinkT::Data>>,
        E,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = ready!(this.inner.poll(cx));

        let classifier = this.classifier.take().unwrap();

        match result {
            Ok(res) => {
                let classification = classifier.classify_response(&res);
                let mut sink: MetricsSinkT = this.sink.take().unwrap();
                let mut sink_data = this.sink_data.take().unwrap();

                match classification {
                    ClassifiedResponse::Ready(classification) => {
                        sink.on_response(
                            &res,
                            ClassifiedResponse::Ready(classification),
                            &mut sink_data,
                        );

                        let res = res.map(|body| ResponseBody {
                            inner: body,
                            parts: None,
                        });

                        Poll::Ready(Ok(res))
                    }
                    ClassifiedResponse::RequiresEos(classify_eos) => {
                        sink.on_response(&res, ClassifiedResponse::RequiresEos(()), &mut sink_data);

                        let res = res.map(|body| ResponseBody {
                            inner: body,
                            parts: Some((classify_eos, sink, sink_data)),
                        });

                        Poll::Ready(Ok(res))
                    }
                }
            }
            Err(err) => {
                let classification = classifier.classify_error(&err);
                this.sink.take().unwrap().on_failure(
                    FailedAt::Response,
                    classification,
                    this.sink_data.take().unwrap(),
                );

                Poll::Ready(Err(err))
            }
        }
    }
}
