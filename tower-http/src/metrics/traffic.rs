#![allow(missing_docs)]

use crate::classify::{ClassifiedResponse, ClassifyResponse, MakeClassifier};
use futures_core::ready;
use http::{Request, Response};
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
pub struct TrafficLayer<M, OnRequest = (), OnResponse = ()> {
    make_classifier: M,
    on_request: OnRequest,
    on_response: OnResponse,
}

impl<M> TrafficLayer<M> {
    pub fn new(make_classifier: M) -> Self {
        TrafficLayer {
            make_classifier,
            on_request: (),
            on_response: (),
        }
    }
}

impl<M, OnRequest, OnResponse> TrafficLayer<M, OnRequest, OnResponse> {
    pub fn on_request<NewOnRequest>(
        self,
        new_on_request: NewOnRequest,
    ) -> TrafficLayer<M, NewOnRequest, OnResponse> {
        TrafficLayer {
            make_classifier: self.make_classifier,
            on_request: new_on_request,
            on_response: self.on_response,
        }
    }

    pub fn on_response<NewOnResponse>(
        self,
        new_on_response: NewOnResponse,
    ) -> TrafficLayer<M, OnRequest, NewOnResponse> {
        TrafficLayer {
            make_classifier: self.make_classifier,
            on_request: self.on_request,
            on_response: new_on_response,
        }
    }
}

impl<S, M, OnRequest, OnResponse> Layer<S> for TrafficLayer<M, OnRequest, OnResponse>
where
    M: Clone,
    OnRequest: Clone,
    OnResponse: Clone,
{
    type Service = Traffic<S, M, OnRequest, OnResponse>;

    fn layer(&self, inner: S) -> Self::Service {
        Traffic {
            inner,
            make_classifier: self.make_classifier.clone(),
            on_request: self.on_request.clone(),
            on_response: self.on_response.clone(),
        }
    }
}

// ===== service =====

#[derive(Debug, Clone)]
pub struct Traffic<S, M, OnRequest = (), OnResponse = ()> {
    inner: S,
    make_classifier: M,
    on_request: OnRequest,
    on_response: OnResponse,
}

impl<S, M> Traffic<S, M> {
    pub fn new(inner: S, make_classifier: M) -> Self {
        Self {
            inner,
            make_classifier,
            on_request: (),
            on_response: (),
        }
    }

    pub fn layer(make_classifier: M) -> TrafficLayer<M> {
        TrafficLayer::new(make_classifier)
    }
}

impl<S, M, OnRequest, OnResponse> Traffic<S, M, OnRequest, OnResponse> {
    define_inner_service_accessors!();

    pub fn on_request<NewOnRequest>(
        self,
        new_on_request: NewOnRequest,
    ) -> Traffic<S, M, NewOnRequest, OnResponse> {
        Traffic {
            inner: self.inner,
            make_classifier: self.make_classifier,
            on_request: new_on_request,
            on_response: self.on_response,
        }
    }

    pub fn on_response<NewOnResponse>(
        self,
        new_on_response: NewOnResponse,
    ) -> Traffic<S, M, OnRequest, NewOnResponse> {
        Traffic {
            inner: self.inner,
            make_classifier: self.make_classifier,
            on_request: self.on_request,
            on_response: new_on_response,
        }
    }
}

impl<S, M, OnRequestT, OnResponseT, ReqBody, ResBody> Service<Request<ReqBody>>
    for Traffic<S, M, OnRequestT, OnResponseT>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    OnRequestT: OnRequest<ReqBody>,
    M: MakeClassifier<S::Error>,
    OnResponseT: OnResponse<ResBody, M::FailureClass> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M::Classifier, OnResponseT>;

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
            on_response: Some(self.on_response.clone()),
            request_received_at,
        }
    }
}

// ===== future =====

#[pin_project]
pub struct ResponseFuture<F, C, OnResponse> {
    #[pin]
    inner: F,
    classifier: Option<C>,
    on_response: Option<OnResponse>,
    request_received_at: Instant,
}

impl<F, C, ResBody, E, OnResponseT> Future for ResponseFuture<F, C, OnResponseT>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    C: ClassifyResponse<E>,
    OnResponseT: OnResponse<ResBody, C::FailureClass>,
{
    type Output = F::Output;

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

                        // TODO(david): separate response, body, and trailers failure callbacks

                        // let span = this.span.clone();
                        // let res = res.map(|body| ResponseBody {
                        //     inner: body,
                        //     classify_eos: None,
                        //     on_eos: None,
                        //     on_body_chunk,
                        //     on_failure: Some(on_failure),
                        //     start,
                        //     span,
                        // });

                        Poll::Ready(Ok(res))
                    }
                    ClassifiedResponse::RequiresEos(classify_eos) => {
                        // let span = this.span.clone();
                        // let res = res.map(|body| ResponseBody {
                        //     inner: body,
                        //     classify_eos: Some(classify_eos),
                        //     on_eos: on_eos.zip(Some(Instant::now())),
                        //     on_body_chunk,
                        //     on_failure: Some(on_failure),
                        //     start,
                        //     span,
                        // });

                        Poll::Ready(Ok(res))
                    }
                }
            }
            Err(err) => Poll::Ready(Err(err)),
        }
    }
}

// ===== body =====

// ===== callbacks =====

pub trait OnRequest<B> {
    fn on_request(&mut self, request: &Request<B>);
}

impl<B, F> OnRequest<B> for F
where
    F: FnMut(&Request<B>),
{
    fn on_request(&mut self, request: &Request<B>) {
        self(request)
    }
}

impl<B> OnRequest<B> for () {
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

impl<B, C, F> OnResponse<B, C> for F
where
    F: FnOnce(&Response<B>, Result<(), C>, Duration),
{
    fn on_response(self, response: &Response<B>, classification: Result<(), C>, latency: Duration) {
        self(response, classification, latency)
    }
}

impl<B, C> OnResponse<B, C> for () {
    #[inline]
    fn on_response(self, _: &Response<B>, _: Result<(), C>, _: Duration) {}
}
