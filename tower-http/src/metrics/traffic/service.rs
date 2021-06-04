use super::{Callbacks, ResponseBody, ResponseFuture, TrafficLayer};
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
pub struct Traffic<S, M, Callbacks> {
    pub(super) inner: S,
    pub(super) make_classifier: M,
    pub(super) callbacks: Callbacks,
}

impl<S, M, Callbacks> Traffic<S, M, Callbacks> {
    /// Create a new `Traffic`.
    pub fn new(inner: S, make_classifier: M, callbacks: Callbacks) -> Self {
        Self {
            inner,
            make_classifier,
            callbacks,
        }
    }

    /// Returns a new [`Layer`] that wraps services with a [`Traffic`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(make_classifier: M, callbacks: Callbacks) -> TrafficLayer<M, Callbacks> {
        TrafficLayer::new(make_classifier, callbacks)
    }

    define_inner_service_accessors!();
}

impl<S, M, ReqBody, ResBody, CallbacksT> Service<Request<ReqBody>> for Traffic<S, M, CallbacksT>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
    M: MakeClassifier,
    CallbacksT: Callbacks<M::FailureClass> + Clone,
    S::Error: fmt::Display + 'static,
{
    type Response = Response<ResponseBody<ResBody, M::ClassifyEos, CallbacksT, CallbacksT::Data>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M::Classifier, CallbacksT, CallbacksT::Data>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let request_received_at = Instant::now();

        let callbacks_data = self.callbacks.prepare(&req);

        let classifier = self.make_classifier.make_classifier(&req);

        ResponseFuture {
            inner: self.inner.call(req),
            classifier: Some(classifier),
            request_received_at,
            callbacks: Some(self.callbacks.clone()),
            callbacks_data: Some(callbacks_data),
        }
    }
}
