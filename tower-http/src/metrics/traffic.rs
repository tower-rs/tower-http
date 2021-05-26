#![allow(missing_docs)]

use crate::classify::{ClassifiedResponse, ClassifyEos, ClassifyResponse, MakeClassifier};
use futures_core::ready;
use http::{HeaderMap, Request, Response};
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tower_layer::Layer;
use tower_service::Service;

// ===== layer =====

#[derive(Debug, Clone)]
pub struct TrafficLayer<M, OnRequest = (), OnResponse = (), OnEos = (), OnFailure = ()> {
    make_classifier: M,
    on_request: OnRequest,
    on_response: OnResponse,
    on_eos: OnEos,
    on_failure: OnFailure,
}

impl<M> TrafficLayer<M> {
    pub fn new(make_classifier: M) -> Self {
        TrafficLayer {
            make_classifier,
            on_request: (),
            on_response: (),
            on_eos: (),
            on_failure: (),
        }
    }
}

impl<M, OnRequest, OnResponse, OnEos, OnFailure>
    TrafficLayer<M, OnRequest, OnResponse, OnEos, OnFailure>
{
    pub fn on_request<NewOnRequest>(
        self,
        new_on_request: NewOnRequest,
    ) -> TrafficLayer<M, NewOnRequest, OnResponse, OnEos, OnFailure> {
        TrafficLayer {
            make_classifier: self.make_classifier,
            on_request: new_on_request,
            on_response: self.on_response,
            on_eos: self.on_eos,
            on_failure: self.on_failure,
        }
    }

    pub fn on_response<NewOnResponse>(
        self,
        new_on_response: NewOnResponse,
    ) -> TrafficLayer<M, OnRequest, NewOnResponse, OnEos, OnFailure> {
        TrafficLayer {
            make_classifier: self.make_classifier,
            on_request: self.on_request,
            on_failure: self.on_failure,
            on_eos: self.on_eos,
            on_response: new_on_response,
        }
    }

    pub fn on_failure<NewOnFailure>(
        self,
        new_on_failure: NewOnFailure,
    ) -> TrafficLayer<M, OnRequest, OnResponse, OnEos, NewOnFailure> {
        TrafficLayer {
            make_classifier: self.make_classifier,
            on_request: self.on_request,
            on_failure: new_on_failure,
            on_response: self.on_response,
            on_eos: self.on_eos,
        }
    }

    pub fn on_eos<NewOnEos>(
        self,
        new_on_eos: NewOnEos,
    ) -> TrafficLayer<M, OnRequest, OnResponse, NewOnEos, OnFailure> {
        TrafficLayer {
            make_classifier: self.make_classifier,
            on_request: self.on_request,
            on_failure: self.on_failure,
            on_response: self.on_response,
            on_eos: new_on_eos,
        }
    }
}

impl<S, M, OnRequest, OnResponse, OnEos, OnFailure> Layer<S>
    for TrafficLayer<M, OnRequest, OnResponse, OnEos, OnFailure>
where
    M: Clone,
    OnRequest: Clone,
    OnResponse: Clone,
    OnEos: Clone,
    OnFailure: Clone,
{
    type Service = Traffic<S, M, OnRequest, OnResponse, OnEos, OnFailure>;

    fn layer(&self, inner: S) -> Self::Service {
        Traffic {
            inner,
            make_classifier: self.make_classifier.clone(),
            on_request: self.on_request.clone(),
            on_response: self.on_response.clone(),
            on_eos: self.on_eos.clone(),
            on_failure: self.on_failure.clone(),
        }
    }
}

// ===== service =====

#[derive(Debug, Clone)]
pub struct Traffic<S, M, OnRequest = (), OnResponse = (), OnEos = (), OnFailure = ()> {
    inner: S,
    make_classifier: M,
    on_request: OnRequest,
    on_response: OnResponse,
    on_eos: OnEos,
    on_failure: OnFailure,
}

impl<S, M> Traffic<S, M> {
    pub fn new(inner: S, make_classifier: M) -> Self {
        Self {
            inner,
            make_classifier,
            on_request: (),
            on_response: (),
            on_eos: (),
            on_failure: (),
        }
    }

    pub fn layer(make_classifier: M) -> TrafficLayer<M> {
        TrafficLayer::new(make_classifier)
    }
}

impl<S, M, OnRequest, OnResponse, OnEos, OnFailure>
    Traffic<S, M, OnRequest, OnResponse, OnEos, OnFailure>
{
    define_inner_service_accessors!();

    pub fn on_request<NewOnRequest>(
        self,
        new_on_request: NewOnRequest,
    ) -> Traffic<S, M, NewOnRequest, OnResponse, OnEos, OnFailure> {
        Traffic {
            inner: self.inner,
            make_classifier: self.make_classifier,
            on_request: new_on_request,
            on_response: self.on_response,
            on_failure: self.on_failure,
            on_eos: self.on_eos,
        }
    }

    pub fn on_response<NewOnResponse>(
        self,
        new_on_response: NewOnResponse,
    ) -> Traffic<S, M, OnRequest, NewOnResponse, OnEos, OnFailure> {
        Traffic {
            inner: self.inner,
            make_classifier: self.make_classifier,
            on_request: self.on_request,
            on_failure: self.on_failure,
            on_response: new_on_response,
            on_eos: self.on_eos,
        }
    }

    pub fn on_failure<NewOnFailure>(
        self,
        new_on_failure: NewOnFailure,
    ) -> Traffic<S, M, OnRequest, OnResponse, OnEos, NewOnFailure> {
        Traffic {
            inner: self.inner,
            make_classifier: self.make_classifier,
            on_request: self.on_request,
            on_failure: new_on_failure,
            on_response: self.on_response,
            on_eos: self.on_eos,
        }
    }

    pub fn on_eos<NewOnEos>(
        self,
        new_on_eos: NewOnEos,
    ) -> Traffic<S, M, OnRequest, OnResponse, NewOnEos, OnFailure> {
        Traffic {
            inner: self.inner,
            make_classifier: self.make_classifier,
            on_request: self.on_request,
            on_failure: self.on_failure,
            on_response: self.on_response,
            on_eos: new_on_eos,
        }
    }
}

impl<S, M, ReqBody, ResBody, OnRequestT, OnResponseT, OnEosT, OnFailureT> Service<Request<ReqBody>>
    for Traffic<S, M, OnRequestT, OnResponseT, OnEosT, OnFailureT>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
    M: MakeClassifier<S::Error>,
    OnRequestT: OnRequest<M::Classifier, ReqBody>,
    OnResponseT: OnResponse<ResBody, M::FailureClass> + Clone,
    OnFailureT: OnFailure<M::FailureClass> + Clone,
    OnEosT: OnEos<M::FailureClass> + Clone,
{
    type Response = Response<ResponseBody<ResBody, M::ClassifyEos, OnEosT, OnFailureT>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M::Classifier, OnResponseT, OnEosT, OnFailureT>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let request_received_at = Instant::now();

        self.on_request.on_request(&req);

        let classifier = self.make_classifier.make_classifier(&req);

        ResponseFuture {
            inner: self.inner.call(req),
            classifier: Some(classifier),
            request_received_at,
            on_response: Some(self.on_response.clone()),
            on_failure: Some(self.on_failure.clone()),
            on_eos: Some(self.on_eos.clone()),
        }
    }
}

// ===== future =====

#[pin_project]
pub struct ResponseFuture<F, C, OnResponse, OnEos, OnFailure> {
    #[pin]
    inner: F,
    classifier: Option<C>,
    request_received_at: Instant,
    on_response: Option<OnResponse>,
    on_eos: Option<OnEos>,
    on_failure: Option<OnFailure>,
}

impl<F, C, ResBody, E, OnResponseT, OnEosT, OnFailureT> Future
    for ResponseFuture<F, C, OnResponseT, OnEosT, OnFailureT>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Body,
    C: ClassifyResponse<E>,
    OnResponseT: OnResponse<ResBody, C::FailureClass>,
    OnFailureT: OnFailure<C::FailureClass>,
    OnEosT: OnEos<C::FailureClass>,
{
    type Output = Result<Response<ResponseBody<ResBody, C::ClassifyEos, OnEosT, OnFailureT>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = ready!(this.inner.poll(cx));
        let latency = this.request_received_at.elapsed();

        let classifier: C = this.classifier.take().unwrap();

        match result {
            Ok(res) => {
                let classification = classifier.classify_response(&res);

                match classification {
                    ClassifiedResponse::Ready(classification) => {
                        this.on_response
                            .take()
                            .unwrap()
                            .on_response(&res, classification, latency);

                        let on_failure = this.on_failure.take().unwrap();

                        let res = res.map(|body| ResponseBody {
                            inner: body,
                            classify_eos: None,
                            on_eos: None,
                            on_failure: Some(on_failure),
                            stream_start: Instant::now(),
                        });

                        Poll::Ready(Ok(res))
                    }
                    ClassifiedResponse::RequiresEos(classify_eos) => {
                        let on_failure = this.on_failure.take().unwrap();
                        let on_eos = this.on_eos.take().unwrap();

                        let res = res.map(|body| ResponseBody {
                            inner: body,
                            classify_eos: Some(classify_eos),
                            on_eos: Some(on_eos),
                            on_failure: Some(on_failure),
                            stream_start: Instant::now(),
                        });

                        Poll::Ready(Ok(res))
                    }
                }
            }
            Err(err) => {
                let classification = classifier.classify_error(&err);
                this.on_failure
                    .take()
                    .unwrap()
                    .on_failure(FailedAt::Response, classification);

                Poll::Ready(Err(err))
            }
        }
    }
}

// ===== body =====

#[pin_project]
pub struct ResponseBody<B, C, OnEos, OnFailure> {
    #[pin]
    inner: B,
    classify_eos: Option<C>,
    on_eos: Option<OnEos>,
    on_failure: Option<OnFailure>,
    stream_start: Instant,
}

impl<B, C, OnEosT, OnFailureT> Body for ResponseBody<B, C, OnEosT, OnFailureT>
where
    B: Body,
    C: ClassifyEos<B::Error>,
    OnEosT: OnEos<C::FailureClass>,
    OnFailureT: OnFailure<C::FailureClass>,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();

        let result = ready!(this.inner.poll_data(cx));
        let stream_duration = this.stream_start.elapsed();

        match result {
            None => Poll::Ready(None),
            Some(Ok(chunk)) => Poll::Ready(Some(Ok(chunk))),
            Some(Err(err)) => {
                let classify_eos = this.classify_eos.take().unwrap();
                let classification = classify_eos.classify_error(&err);
                this.on_failure.take().unwrap().on_failure(
                    FailedAt::Body(FailedAtBody { stream_duration }),
                    classification,
                );

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
        let stream_duration = this.stream_start.elapsed();

        let classify_eos = this.classify_eos.take().unwrap();

        match result {
            Ok(trailers) => {
                let classification = this
                    .classify_eos
                    .take()
                    .unwrap()
                    .classify_eos(trailers.as_ref());

                this.on_eos.take().unwrap().on_eos(
                    trailers.as_ref(),
                    classification,
                    stream_duration,
                );

                Poll::Ready(Ok(trailers))
            }
            Err(err) => {
                let classification = classify_eos.classify_error(&err);
                this.on_failure.take().unwrap().on_failure(
                    FailedAt::Body(FailedAtBody { stream_duration }),
                    classification,
                );

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

// ===== callbacks =====

pub trait OnRequest<C, B> {
    fn on_request(&mut self, request: &Request<B>);
}

impl<C, B, F> OnRequest<C, B> for F
where
    F: FnMut(&Request<B>),
{
    fn on_request(&mut self, request: &Request<B>) {
        self(request)
    }
}

impl<C, B> OnRequest<C, B> for () {
    #[inline]
    fn on_request(&mut self, _: &Request<B>) {}
}

pub trait OnResponse<ResBody, FailureClass> {
    fn on_response(
        self,
        response: &Response<ResBody>,
        classification: Result<(), FailureClass>,
        latency: Duration,
    );
}

impl<B, FailureClass, F> OnResponse<B, FailureClass> for F
where
    F: FnOnce(&Response<B>, Result<(), FailureClass>, Duration),
{
    fn on_response(
        self,
        response: &Response<B>,
        classification: Result<(), FailureClass>,
        latency: Duration,
    ) {
        self(response, classification, latency)
    }
}

impl<B, FailureClass> OnResponse<B, FailureClass> for () {
    #[inline]
    fn on_response(self, _: &Response<B>, _: Result<(), FailureClass>, _: Duration) {}
}

pub trait OnEos<FailureClass> {
    fn on_eos(
        self,
        trailers: Option<&HeaderMap>,
        classification: Result<(), FailureClass>,
        stream_duration: Duration,
    );
}

impl<FailureClass, F> OnEos<FailureClass> for F
where
    F: FnOnce(Option<&HeaderMap>, Result<(), FailureClass>, Duration),
{
    fn on_eos(
        self,
        trailers: Option<&HeaderMap>,
        classification: Result<(), FailureClass>,
        stream_duration: Duration,
    ) {
        self(trailers, classification, stream_duration)
    }
}

impl<FailureClass> OnEos<FailureClass> for () {
    #[inline]
    fn on_eos(self, _: Option<&HeaderMap>, _: Result<(), FailureClass>, _: Duration) {}
}

pub trait OnFailure<FailureClass> {
    fn on_failure(self, failed_at: FailedAt, failure_classification: FailureClass);
}

impl<FailureClass, F> OnFailure<FailureClass> for F
where
    F: FnOnce(FailedAt, FailureClass),
{
    fn on_failure(self, failed_at: FailedAt, failure_classification: FailureClass) {
        self(failed_at, failure_classification)
    }
}

impl<FailureClass> OnFailure<FailureClass> for () {
    #[inline]
    fn on_failure(self, _: FailedAt, _: FailureClass) {}
}

#[derive(Debug)]
pub enum FailedAt {
    Response,
    Body(FailedAtBody),
    Trailers(FailedAtBody),
}

#[derive(Debug)]
pub struct FailedAtBody {
    stream_duration: Duration,
}

impl FailedAtBody {
    pub fn stream_duration(&self) -> Duration {
        self.stream_duration
    }
}

#[cfg(test)]
mod tests {
    #![allow(warnings)]

    use super::*;
    use crate::classify::{GrpcEosErrorsAsFailures, GrpcErrorsAsFailures};
    use http::{Method, Request, Response, StatusCode, Uri, Version};
    use hyper::Body;
    use tower::{BoxError, Service, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn unary_request() {
        let mut svc = ServiceBuilder::new()
            .layer(
                TrafficLayer::new(MyMakeClassify)
                    .on_request(|_: &Request<Body>| {})
                    .on_response(
                        |_res: &Response<Body>,
                         class: Result<(), MyFailureClass>,
                         _latency: Duration| {
                            todo!();
                        },
                    )
                    .on_eos(
                        |_trailers: Option<&HeaderMap>,
                         class: Result<(), MyFailureClass>,
                         _stream_duration: Duration| {
                            todo!();
                        },
                    )
                    .on_failure(|_: FailedAt, class: MyFailureClass| {
                        todo!();
                    }),
            )
            .service_fn(echo);

        let res = svc
            .ready()
            .await
            .unwrap()
            .call(Request::new(Body::from("foobar")))
            .await
            .unwrap();

        hyper::body::to_bytes(res.into_body()).await.unwrap();
    }

    async fn echo(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        Ok(Response::new(req.into_body()))
    }

    #[derive(Clone)]
    struct MyMakeClassify;

    impl MakeClassifier<hyper::Error> for MyMakeClassify {
        type Classifier = MyClassifier;
        type FailureClass = MyFailureClass;
        type ClassifyEos = MyClassifyEos;

        fn make_classifier<B>(&self, req: &Request<B>) -> Self::Classifier {
            MyClassifier {
                uri: req.uri().clone(),
                method: req.method().clone(),
                version: req.version(),
                inner: GrpcErrorsAsFailures::new(),
            }
        }
    }

    struct MyClassifier {
        uri: Uri,
        method: Method,
        version: Version,
        inner: GrpcErrorsAsFailures,
    }

    impl ClassifyResponse<hyper::Error> for MyClassifier {
        type FailureClass = MyFailureClass;
        type ClassifyEos = MyClassifyEos;

        fn classify_response<B>(
            self,
            res: &Response<B>,
        ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos> {
            let uri = self.uri.clone();
            let method = self.method.clone();
            let version = self.version;

            match ClassifyResponse::<hyper::Error>::classify_response(self.inner, res) {
                ClassifiedResponse::Ready(result) => {
                    ClassifiedResponse::Ready(result.map_err(|grpc_code| MyFailureClass {
                        grpc_code,
                        uri,
                        method,
                        version,
                    }))
                }
                ClassifiedResponse::RequiresEos(classify_eos) => {
                    ClassifiedResponse::RequiresEos(MyClassifyEos {
                        inner: classify_eos,
                        uri,
                        method,
                        version,
                    })
                }
            }
        }

        fn classify_error(self, error: &hyper::Error) -> Self::FailureClass {
            MyFailureClass {
                uri: self.uri.clone(),
                method: self.method.clone(),
                version: self.version,
                grpc_code: self.inner.classify_error(error),
            }
        }
    }

    struct MyClassifyEos {
        inner: GrpcEosErrorsAsFailures,
        uri: Uri,
        method: Method,
        version: Version,
    }

    impl ClassifyEos<hyper::Error> for MyClassifyEos {
        type FailureClass = MyFailureClass;

        fn classify_eos(self, trailers: Option<&HeaderMap>) -> Result<(), Self::FailureClass> {
            let uri = self.uri.clone();
            let method = self.method.clone();
            let version = self.version;

            ClassifyEos::<hyper::Error>::classify_eos(self.inner, trailers).map_err(|grpc_code| {
                MyFailureClass {
                    grpc_code,
                    uri,
                    method,
                    version,
                }
            })
        }

        fn classify_error(self, error: &hyper::Error) -> Self::FailureClass {
            MyFailureClass {
                grpc_code: self.inner.classify_error(error),
                uri: self.uri.clone(),
                method: self.method.clone(),
                version: self.version,
            }
        }
    }

    struct MyFailureClass {
        uri: Uri,
        method: Method,
        version: Version,
        grpc_code: i32,
    }
}
