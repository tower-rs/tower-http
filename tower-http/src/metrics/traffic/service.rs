use super::{MetricsSink, ResponseBody, ResponseFuture, TrafficLayer};
use crate::classify::MakeClassifier;
use http::{Request, Response};
use http_body::Body;
use std::{
    fmt,
    task::{Context, Poll},
    time::Instant,
};
use tower_service::Service;

/// Middleware for adding high level traffic metrics to a [`Service`].
///
/// See the [module docs](crate::metrics::traffic) for more details.
///
/// [`Layer`]: tower_layer::Layer
/// [`Service`]: tower_service::Service
#[derive(Debug, Clone)]
pub struct Traffic<S, M, MetricsSink> {
    pub(super) inner: S,
    pub(super) make_classifier: M,
    pub(super) sink: MetricsSink,
}

impl<S, M, MetricsSink> Traffic<S, M, MetricsSink> {
    /// Create a new `Traffic`.
    pub fn new(inner: S, make_classifier: M, sink: MetricsSink) -> Self {
        Self {
            inner,
            make_classifier,
            sink,
        }
    }

    /// Returns a new [`Layer`] that wraps services with a [`Traffic`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(make_classifier: M, sink: MetricsSink) -> TrafficLayer<M, MetricsSink> {
        TrafficLayer::new(make_classifier, sink)
    }

    define_inner_service_accessors!();
}

impl<S, M, ReqBody, ResBody, MetricsSinkT> Service<Request<ReqBody>> for Traffic<S, M, MetricsSinkT>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
    M: MakeClassifier,
    MetricsSinkT: MetricsSink<M::FailureClass> + Clone,
    S::Error: fmt::Display + 'static,
{
    type Response =
        Response<ResponseBody<ResBody, M::ClassifyEos, MetricsSinkT, MetricsSinkT::Data>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M::Classifier, MetricsSinkT, MetricsSinkT::Data>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let request_received_at = Instant::now();

        let sink_data = self.sink.prepare(&req);

        let classifier = self.make_classifier.make_classifier(&req);

        ResponseFuture {
            inner: self.inner.call(req),
            classifier: Some(classifier),
            request_received_at,
            sink: Some(self.sink.clone()),
            sink_data: Some(sink_data),
        }
    }
}
