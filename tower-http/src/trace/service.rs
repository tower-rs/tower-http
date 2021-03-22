use super::{
    DefaultMakeSpan, DefaultOnBodyChunk, DefaultOnEos, DefaultOnFailure, DefaultOnRequest,
    DefaultOnResponse, MakeSpan, OnBodyChunk, OnEos, OnFailure, OnRequest, OnResponse,
    ResponseBody, ResponseFuture, TraceLayer,
};
use crate::classify::{
    GrpcErrorsAsFailures, MakeClassifier, ServerErrorsAsFailures, SharedClassifier,
};
use http::{Request, Response};
use http_body::Body;
use std::{
    fmt,
    marker::PhantomData,
    task::{Context, Poll},
    time::Instant,
};
use tower_service::Service;

/// Middleware that adds high level [tracing] to a [`Service`].
///
/// See the [module docs](crate::trace) for an example.
///
/// [tracing]: https://crates.io/crates/tracing
/// [`Service`]: tower_service::Service
pub struct Trace<
    S,
    M,
    E,
    MakeSpan = DefaultMakeSpan,
    OnRequest = DefaultOnRequest,
    OnResponse = DefaultOnResponse,
    OnBodyChunk = DefaultOnBodyChunk,
    OnEos = DefaultOnEos,
    OnFailure = DefaultOnFailure,
> {
    pub(crate) inner: S,
    pub(crate) make_classifier: M,
    pub(crate) make_span: MakeSpan,
    pub(crate) on_request: OnRequest,
    pub(crate) on_response: OnResponse,
    pub(crate) on_body_chunk: OnBodyChunk,
    pub(crate) on_eos: OnEos,
    pub(crate) on_failure: OnFailure,
    pub(crate) _error: PhantomData<fn() -> E>,
}

impl<S, M, E> Trace<S, M, E> {
    /// Create a new [`Trace`] using the given [`MakeClassifier`].
    pub fn new(inner: S, make_classifier: M) -> Self
    where
        M: MakeClassifier<E>,
    {
        Self {
            inner,
            make_classifier,
            make_span: DefaultMakeSpan::new(),
            on_request: DefaultOnRequest::default(),
            on_response: DefaultOnResponse::default(),
            on_body_chunk: DefaultOnBodyChunk::default(),
            on_eos: DefaultOnEos::default(),
            on_failure: DefaultOnFailure::default(),
            _error: PhantomData,
        }
    }

    /// Returns a new [`Layer`] that wraps services with a [`TraceLayer`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(make_classifier: M) -> TraceLayer<M, E>
    where
        M: MakeClassifier<E>,
    {
        TraceLayer::new(make_classifier)
    }
}

impl<S, M, E, MakeSpan, OnRequest, OnResponse, OnBodyChunk, OnEos, OnFailure>
    Trace<S, M, E, MakeSpan, OnRequest, OnResponse, OnBodyChunk, OnEos, OnFailure>
{
    define_inner_service_accessors!();

    pub fn on_request<NewOnRequest>(
        self,
        new_on_request: NewOnRequest,
    ) -> Trace<S, M, E, MakeSpan, NewOnRequest, OnResponse, OnBodyChunk, OnEos, OnFailure> {
        Trace {
            on_request: new_on_request,
            inner: self.inner,
            on_failure: self.on_failure,
            on_eos: self.on_eos,
            on_body_chunk: self.on_body_chunk,
            make_span: self.make_span,
            on_response: self.on_response,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn on_response<NewOnResponse>(
        self,
        new_on_response: NewOnResponse,
    ) -> Trace<S, M, E, MakeSpan, OnRequest, NewOnResponse, OnBodyChunk, OnEos, OnFailure> {
        Trace {
            on_response: new_on_response,
            inner: self.inner,
            on_request: self.on_request,
            on_failure: self.on_failure,
            on_body_chunk: self.on_body_chunk,
            on_eos: self.on_eos,
            make_span: self.make_span,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn on_failure<NewOnFailure>(
        self,
        new_on_failure: NewOnFailure,
    ) -> Trace<S, M, E, MakeSpan, OnRequest, OnResponse, OnBodyChunk, OnEos, NewOnFailure> {
        Trace {
            on_failure: new_on_failure,
            inner: self.inner,
            make_span: self.make_span,
            on_body_chunk: self.on_body_chunk,
            on_request: self.on_request,
            on_eos: self.on_eos,
            on_response: self.on_response,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn on_body_chunk<NewOnBodyChunk>(
        self,
        new_on_body_chunk: NewOnBodyChunk,
    ) -> Trace<S, M, E, MakeSpan, OnRequest, OnResponse, NewOnBodyChunk, OnEos, OnFailure> {
        Trace {
            on_body_chunk: new_on_body_chunk,
            on_eos: self.on_eos,
            make_span: self.make_span,
            inner: self.inner,
            on_failure: self.on_failure,
            on_request: self.on_request,
            on_response: self.on_response,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn on_eos<NewOnEos>(
        self,
        new_on_eos: NewOnEos,
    ) -> Trace<S, M, E, MakeSpan, OnRequest, OnResponse, OnBodyChunk, NewOnEos, OnFailure> {
        Trace {
            on_eos: new_on_eos,
            make_span: self.make_span,
            inner: self.inner,
            on_failure: self.on_failure,
            on_request: self.on_request,
            on_body_chunk: self.on_body_chunk,
            on_response: self.on_response,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn make_span_with<NewMakeSpan>(
        self,
        new_make_span: NewMakeSpan,
    ) -> Trace<S, M, E, NewMakeSpan, OnRequest, OnResponse, OnBodyChunk, OnEos, OnFailure> {
        Trace {
            make_span: new_make_span,
            inner: self.inner,
            on_failure: self.on_failure,
            on_request: self.on_request,
            on_body_chunk: self.on_body_chunk,
            on_response: self.on_response,
            on_eos: self.on_eos,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }
}

impl<S, E>
    Trace<
        S,
        SharedClassifier<ServerErrorsAsFailures>,
        E,
        DefaultMakeSpan,
        DefaultOnRequest,
        DefaultOnResponse,
        DefaultOnBodyChunk,
        DefaultOnEos,
        DefaultOnFailure,
    >
{
    /// Create a new [`Trace`] using [`ServerErrorsAsFailures`] which supports classifying
    /// regular HTTP responses based on the status code.
    pub fn new_for_http(inner: S) -> Self {
        Self {
            inner,
            make_classifier: SharedClassifier::new::<E>(ServerErrorsAsFailures::default()),
            make_span: DefaultMakeSpan::new(),
            on_request: DefaultOnRequest::default(),
            on_response: DefaultOnResponse::default(),
            on_body_chunk: DefaultOnBodyChunk::default(),
            on_eos: DefaultOnEos::default(),
            on_failure: DefaultOnFailure::default(),
            _error: PhantomData,
        }
    }
}

impl<S, E>
    Trace<
        S,
        SharedClassifier<GrpcErrorsAsFailures>,
        E,
        DefaultMakeSpan,
        DefaultOnRequest,
        DefaultOnResponse,
        DefaultOnBodyChunk,
        DefaultOnEos,
        DefaultOnFailure,
    >
{
    /// Create a new [`Trace`] using [`GrpcErrorsAsFailures`] which supports classifying
    /// gRPC responses and streams based on the `grpc-status` header.
    pub fn new_for_grpc(inner: S) -> Self {
        Self {
            inner,
            make_classifier: SharedClassifier::new::<E>(GrpcErrorsAsFailures::default()),
            make_span: DefaultMakeSpan::new(),
            on_request: DefaultOnRequest::default(),
            on_response: DefaultOnResponse::default(),
            on_body_chunk: DefaultOnBodyChunk::default(),
            on_eos: DefaultOnEos::default(),
            on_failure: DefaultOnFailure::default(),
            _error: PhantomData,
        }
    }
}

impl<
        S,
        ReqBody,
        ResBody,
        M,
        OnRequestT,
        OnResponseT,
        OnFailureT,
        OnBodyChunkT,
        OnEosT,
        MakeSpanT,
    > Service<Request<ReqBody>>
    for Trace<S, M, S::Error, MakeSpanT, OnRequestT, OnResponseT, OnBodyChunkT, OnEosT, OnFailureT>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ReqBody: Body,
    ResBody: Body,
    M: MakeClassifier<S::Error>,
    M::Classifier: Clone,
    MakeSpanT: MakeSpan<ReqBody>,
    OnRequestT: OnRequest<ReqBody>,
    OnResponseT: OnResponse<ResBody> + Clone,
    OnBodyChunkT: OnBodyChunk<ResBody::Data> + Clone,
    OnEosT: OnEos + Clone,
    OnFailureT: OnFailure<M::FailureClass> + Clone,
{
    type Response =
        Response<ResponseBody<ResBody, M::ClassifyEos, OnBodyChunkT, OnEosT, OnFailureT>>;
    type Error = S::Error;
    type Future =
        ResponseFuture<S::Future, M::Classifier, OnResponseT, OnBodyChunkT, OnEosT, OnFailureT>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let start = Instant::now();

        let span = self.make_span.make_span(&req);

        let classifier = self.make_classifier.make_classifier(&req);

        let future = {
            let _guard = span.enter();
            self.on_request.on_request(&req);
            self.inner.call(req)
        };

        ResponseFuture {
            inner: future,
            span,
            classifier: Some(classifier),
            on_response: Some(self.on_response.clone()),
            on_body_chunk: Some(self.on_body_chunk.clone()),
            on_eos: Some(self.on_eos.clone()),
            on_failure: Some(self.on_failure.clone()),
            start,
        }
    }
}

impl<S, M, E, MakeSpan, OnRequest, OnResponse, OnBodyChunk, OnEos, OnFailure> Clone
    for Trace<S, M, E, MakeSpan, OnRequest, OnResponse, OnBodyChunk, OnEos, OnFailure>
where
    S: Clone,
    M: Clone,
    MakeSpan: Clone,
    OnFailure: Clone,
    OnRequest: Clone,
    OnResponse: Clone,
    OnBodyChunk: Clone,
    OnEos: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            on_failure: self.on_failure.clone(),
            make_span: self.make_span.clone(),
            on_response: self.on_response.clone(),
            on_body_chunk: self.on_body_chunk.clone(),
            on_eos: self.on_eos.clone(),
            make_classifier: self.make_classifier.clone(),
            on_request: self.on_request.clone(),
            _error: self._error,
        }
    }
}

impl<S, M, E, MakeSpan, OnRequest, OnResponse, OnBodyChunk, OnEos, OnFailure> fmt::Debug
    for Trace<S, M, E, MakeSpan, OnRequest, OnResponse, OnBodyChunk, OnEos, OnFailure>
where
    S: fmt::Debug,
    M: fmt::Debug,
    MakeSpan: fmt::Debug,
    OnFailure: fmt::Debug,
    OnRequest: fmt::Debug,
    OnBodyChunk: fmt::Debug,
    OnResponse: fmt::Debug,
    OnEos: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Trace")
            .field("inner", &self.inner)
            .field("make_classifier", &self.make_classifier)
            .field("make_span", &self.make_span)
            .field("on_request", &self.on_request)
            .field("on_response", &self.on_response)
            .field("on_body_chunk", &self.on_body_chunk)
            .field("on_eos", &self.on_eos)
            .field("on_failure", &self.on_failure)
            .finish()
    }
}
