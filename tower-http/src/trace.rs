//! Middleware that add high level [tracing] to a [`Service`].
//!
//! [tracing]: https://crates.io/crates/tracing
//! [`Service`]: tower_service::Service

use crate::classify::{
    ClassifiedNowOrLater, ClassifyEos, ClassifyResponse, GrpcErrorsAsFailures, MakeClassifier,
    ServerErrorsAsFailures, SharedClassifier,
};
use http::{HeaderMap, Request, Response};
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// [`Layer`] that adds high level [tracing] to a [`Service`].
///
/// [`Layer`]: tower_layer::Layer
/// [tracing]: https://crates.io/crates/tracing
/// [`Service`]: tower_service::Service
#[derive(Clone, Default)]
pub struct TraceLayer<C> {
    make_classifier: C,
}

impl TraceLayer<SharedClassifier<ServerErrorsAsFailures>> {
    /// Create a new [`TraceLayer`] using [`ServerErrorsAsFailures`] which supports classifying
    /// regular HTTP responses based on the status code.
    pub fn new<E>() -> Self {
        Self {
            make_classifier: SharedClassifier::new::<E>(ServerErrorsAsFailures::default()),
        }
    }
}

impl TraceLayer<SharedClassifier<GrpcErrorsAsFailures>> {
    /// Create a new [`TraceLayer`] using [`GrpcErrorsAsFailures`] which supports classifying
    /// gRPC responses and streams based on the `grpc-status` header.
    pub fn new_for_grpc<E>() -> Self {
        Self {
            make_classifier: SharedClassifier::new::<E>(GrpcErrorsAsFailures::default()),
        }
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
            make_classifier: self.make_classifier.clone(),
        }
    }
}

/// Middleware that add high level [tracing] to a [`Service`].
///
/// [tracing]: https://crates.io/crates/tracing
/// [`Service`]: tower_service::Service
#[derive(Clone, Default)]
pub struct Trace<S, C> {
    inner: S,
    make_classifier: C,
}

impl<S, ReqBody, ResBody, C> Service<Request<ReqBody>> for Trace<S, C>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ReqBody: Body,
    ResBody: Body,
    C: MakeClassifier<S::Error>,
    C::Classifier: Clone,
{
    type Response = Response<TraceBody<ResBody, C::ClassifyEos>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, C::Classifier>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let classifier = self.make_classifier.make_classify(&req);

        ResponseFuture {
            inner: self.inner.call(req),
            classifier: Some(classifier),
        }
    }
}

/// Response future for [`Trace`].
#[pin_project]
pub struct ResponseFuture<F, C> {
    #[pin]
    inner: F,
    classifier: Option<C>,
}

impl<F, ResBody, E, C> Future for ResponseFuture<F, C>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Body,
    C: ClassifyResponse<E> + Clone,
{
    type Output = Result<Response<TraceBody<ResBody, C::ClassifyEos>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = futures_util::ready!(this.inner.poll(cx));

        match result {
            Ok(res) => match this.classifier.take().unwrap().classify_response(&res) {
                ClassifiedNowOrLater::Ready(classification) => {
                    let res = res.map(|body| TraceBody {
                        inner: body,
                        classify_eos: None,
                    });
                    Poll::Ready(Ok(res))
                }
                ClassifiedNowOrLater::RequiresEos(classify_eos) => {
                    let res = res.map(|body| TraceBody {
                        inner: body,
                        classify_eos: Some(classify_eos),
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

        if let Some(classify_eos) = this.classify_eos.take() {
            let classification = classify_eos.classify_eos(trailers.as_ref());
        }

        Poll::Ready(Ok(trailers))
    }
}

#[allow(warnings)]
#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use hyper::{Body, Error, Request, Response};
    use tower::{
        retry::{Policy, Retry},
        service_fn, ServiceBuilder, ServiceExt,
    };

    // just checking this actually compiles
    async fn http() {
        let svc = ServiceBuilder::new()
            .layer(TraceLayer::new::<Error>())
            .service(service_fn(handle));

        svc.oneshot(Request::new(Body::empty())).await.unwrap();
    }

    // just checking this actually compiles
    async fn grpc() {
        let svc = ServiceBuilder::new()
            .layer(TraceLayer::new_for_grpc::<Error>())
            .service(service_fn(handle));

        svc.oneshot(Request::new(Body::empty())).await.unwrap();
    }

    #[allow(warnings)]
    async fn handle(_req: Request<Body>) -> Result<Response<Body>, Error> {
        todo!()
    }

    trait IsRetryable {
        fn is_retryable(&self) -> bool;
    }

    #[derive(Clone)]
    struct RetryBasedOnClassification<C> {
        classifier: C,
        // ...
    }

    impl<ReqB, ResB, E, C> Policy<Request<ReqB>, Response<ResB>, E> for RetryBasedOnClassification<C>
    where
        C: ClassifyResponse<E> + Clone,
        C::FailureClass: IsRetryable,
        ResB: http_body::Body,
        Request<ReqB>: Clone,
        E: std::error::Error + 'static,
    {
        type Future = futures::future::Ready<RetryBasedOnClassification<C>>;

        fn retry(
            &self,
            req: &Request<ReqB>,
            res: Result<&Response<ResB>, &E>,
        ) -> Option<Self::Future> {
            match res {
                Ok(res) => {
                    if let ClassifiedNowOrLater::Ready(class) =
                        self.classifier.clone().classify_response(res)
                    {
                        if class.err()?.is_retryable() {
                            return Some(futures::future::ready(self.clone()));
                        }
                    }

                    None
                }
                Err(err) => self
                    .classifier
                    .clone()
                    .classify_error(err)
                    .is_retryable()
                    .then(|| futures::future::ready(self.clone())),
            }
        }

        fn clone_request(&self, req: &Request<ReqB>) -> Option<Request<ReqB>> {
            Some(req.clone())
        }
    }
}
