//! Middleware that add high level [tracing] to a [`Service`].
//!
//! [tracing]: https://crates.io/crates/tracing
//! [`Service`]: tower_service::Service

use crate::classify::{
    ClassifiedNowOrLater, ClassifyEos, ClassifyResponse, ServerErrorsAsFailures,
};
use http::{HeaderMap, Request, Response};
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};
use tower_layer::Layer;
use tower_service::Service;

/// [`Layer`] that adds high level [tracing] to a [`Service`].
///
/// [`Layer`]: tower_layer::Layer
/// [tracing]: https://crates.io/crates/tracing
/// [`Service`]: tower_service::Service
#[derive(Clone, Default)]
pub struct TraceLayer<C = ServerErrorsAsFailures> {
    classifier: C,
}

impl TraceLayer<ServerErrorsAsFailures> {
    /// Create a new [`TraceLayer`] using [`ServerErrorsAsFailures`] as the response classifier.
    pub fn new() -> Self {
        Self {
            classifier: ServerErrorsAsFailures::new(),
        }
    }
}

impl<C> TraceLayer<C> {
    /// Provider another response classifier to use.
    pub fn with_classifier<NewClassifier>(
        self,
        classifier: NewClassifier,
    ) -> TraceLayer<NewClassifier> {
        TraceLayer { classifier }
    }
}

impl<S, C> Layer<S> for TraceLayer<C>
where
    C: Clone,
{
    type Service = Trace<S, C>;

    fn layer(&self, inner: S) -> Self::Service {
        Trace {
            inner,
            classifier: self.classifier.clone(),
        }
    }
}

/// Middleware that add high level [tracing] to a [`Service`].
///
/// [tracing]: https://crates.io/crates/tracing
/// [`Service`]: tower_service::Service
pub struct Trace<S, C> {
    inner: S,
    classifier: C,
}

impl<S, ReqBody, ResBody, C> Service<Request<ReqBody>> for Trace<S, C>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
    C: ClassifyResponse + Clone,
{
    type Response = Response<TraceBody<ResBody, C::ClassifyEos>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, C>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let start = Instant::now();

        // TODO(david): log that we received a request

        ResponseFuture {
            inner: self.inner.call(req),
            start,
            classifier: self.classifier.clone(),
        }
    }
}

/// Response future for [`Trace`].
#[pin_project]
pub struct ResponseFuture<F, C> {
    #[pin]
    inner: F,
    start: Instant,
    classifier: C,
}

impl<F, ResBody, E, C> Future for ResponseFuture<F, C>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Body,
    C: ClassifyResponse + Clone,
{
    type Output = Result<Response<TraceBody<ResBody, C::ClassifyEos>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = futures_util::ready!(this.inner.poll(cx));
        let latency = this.start.elapsed();
        let start = *this.start;

        match result {
            Ok(res) => match this.classifier.classify_response(&res) {
                ClassifiedNowOrLater::Ready(classification) => {
                    // TODO(david): use classification and latency

                    let res = res.map(|body| TraceBody {
                        inner: body,
                        classify_eos: None,
                        start,
                    });
                    Poll::Ready(Ok(res))
                }
                ClassifiedNowOrLater::RequiresEos(classify_eos) => {
                    let res = res.map(|body| TraceBody {
                        inner: body,
                        classify_eos: Some(classify_eos),
                        start,
                    });
                    Poll::Ready(Ok(res))
                }
            },
            Err(err) => {
                // TODO(david): log error
                Poll::Ready(Err(err))
            }
        }
    }
}

/// Response body for [`Trace`].
#[pin_project]
pub struct TraceBody<B, C> {
    #[pin]
    inner: B,
    classify_eos: Option<C>,
    start: Instant,
}

impl<B, C> Body for TraceBody<B, C>
where
    B: Body,
    C: ClassifyEos,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        self.project().inner.poll_data(cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        let this = self.project();
        let trailers = futures_util::ready!(this.inner.poll_trailers(cx)?);

        if let Some(mut classify_eos) = this.classify_eos.take() {
            let classification = classify_eos.classify_eos(trailers.as_ref());
            // TODO(david): use classification and start
        }

        Poll::Ready(Ok(trailers))
    }
}
