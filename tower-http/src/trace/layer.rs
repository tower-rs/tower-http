use super::{
    DefaultMakeSpan, DefaultOnEos, DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, Trace,
};
use crate::classify::{
    GrpcErrorsAsFailures, MakeClassifier, ServerErrorsAsFailures, SharedClassifier,
};
use std::{fmt, marker::PhantomData};
use tower_layer::Layer;

/// [`Layer`] that adds high level [tracing] to a [`Service`].
///
/// See the [module docs](crate::trace) for an example.
///
/// [`Layer`]: tower_layer::Layer
/// [tracing]: https://crates.io/crates/tracing
/// [`Service`]: tower_service::Service
pub struct TraceLayer<
    M,
    E,
    MakeSpan = DefaultMakeSpan,
    OnRequest = DefaultOnRequest,
    OnResponse = DefaultOnResponse,
    OnEos = DefaultOnEos,
    OnFailure = DefaultOnFailure,
> {
    pub(crate) make_classifier: M,
    pub(crate) make_span: MakeSpan,
    pub(crate) add_headers_to_span: bool,
    pub(crate) on_request: OnRequest,
    pub(crate) on_response: OnResponse,
    pub(crate) on_eos: OnEos,
    pub(crate) on_failure: OnFailure,
    pub(crate) _error: PhantomData<fn() -> E>,
}

impl<M, E, MakeSpan, OnRequest, OnResponse, OnEos, OnFailure> Clone
    for TraceLayer<M, E, MakeSpan, OnRequest, OnResponse, OnEos, OnFailure>
where
    M: Clone,
    MakeSpan: Clone,
    OnRequest: Clone,
    OnResponse: Clone,
    OnEos: Clone,
    OnFailure: Clone,
{
    fn clone(&self) -> Self {
        Self {
            on_request: self.on_request.clone(),
            on_response: self.on_response.clone(),
            on_failure: self.on_failure.clone(),
            on_eos: self.on_eos.clone(),
            make_span: self.make_span.clone(),
            add_headers_to_span: self.add_headers_to_span,
            make_classifier: self.make_classifier.clone(),
            _error: self._error,
        }
    }
}

impl<M, E> TraceLayer<M, E> {
    /// Create a new [`TraceLayer`] using the given [`MakeClassifier`].
    pub fn new(make_classifier: M) -> Self
    where
        M: MakeClassifier<E>,
    {
        Self {
            make_classifier,
            make_span: DefaultMakeSpan::new(),
            on_failure: DefaultOnFailure::default(),
            on_request: DefaultOnRequest::default(),
            on_eos: DefaultOnEos::default(),
            on_response: DefaultOnResponse::default(),
            add_headers_to_span: false,
            _error: PhantomData,
        }
    }
}

impl<M, E, MakeSpan, OnRequest, OnResponse, OnEos, OnFailure>
    TraceLayer<M, E, MakeSpan, OnRequest, OnResponse, OnEos, OnFailure>
{
    pub fn on_request<NewOnRequest>(
        self,
        new_on_request: NewOnRequest,
    ) -> TraceLayer<M, E, MakeSpan, NewOnRequest, OnResponse, OnEos, OnFailure> {
        TraceLayer {
            on_request: new_on_request,
            on_failure: self.on_failure,
            on_eos: self.on_eos,
            make_span: self.make_span,
            on_response: self.on_response,
            add_headers_to_span: self.add_headers_to_span,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn on_response<NewOnResponse>(
        self,
        new_on_response: NewOnResponse,
    ) -> TraceLayer<M, E, MakeSpan, OnRequest, NewOnResponse, OnEos, OnFailure> {
        TraceLayer {
            on_response: new_on_response,
            on_request: self.on_request,
            on_eos: self.on_eos,
            on_failure: self.on_failure,
            make_span: self.make_span,
            add_headers_to_span: self.add_headers_to_span,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn on_eos<NewOnEos>(
        self,
        new_on_eos: NewOnEos,
    ) -> TraceLayer<M, E, MakeSpan, OnRequest, OnResponse, NewOnEos, OnFailure> {
        TraceLayer {
            on_eos: new_on_eos,
            on_failure: self.on_failure,
            on_request: self.on_request,
            make_span: self.make_span,
            on_response: self.on_response,
            add_headers_to_span: self.add_headers_to_span,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn on_failure<NewOnFailure>(
        self,
        new_on_failure: NewOnFailure,
    ) -> TraceLayer<M, E, MakeSpan, OnRequest, OnResponse, OnEos, NewOnFailure> {
        TraceLayer {
            on_failure: new_on_failure,
            on_request: self.on_request,
            on_eos: self.on_eos,
            make_span: self.make_span,
            on_response: self.on_response,
            add_headers_to_span: self.add_headers_to_span,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn make_span_with<NewMakeSpan>(
        self,
        new_make_span: NewMakeSpan,
    ) -> TraceLayer<M, E, NewMakeSpan, OnRequest, OnResponse, OnEos, OnFailure> {
        TraceLayer {
            make_span: new_make_span,
            on_request: self.on_request,
            on_failure: self.on_failure,
            on_eos: self.on_eos,
            on_response: self.on_response,
            add_headers_to_span: self.add_headers_to_span,
            make_classifier: self.make_classifier,
            _error: self._error,
        }
    }

    pub fn add_headers_to_span(mut self, value: bool) -> Self {
        self.add_headers_to_span = value;
        self
    }
}

impl<E> TraceLayer<SharedClassifier<ServerErrorsAsFailures>, E> {
    /// Create a new [`TraceLayer`] using [`ServerErrorsAsFailures`] which supports classifying
    /// regular HTTP responses based on the status code.
    pub fn new_for_http() -> Self {
        Self {
            make_classifier: SharedClassifier::new::<E>(ServerErrorsAsFailures::default()),
            make_span: DefaultMakeSpan::new(),
            add_headers_to_span: false,
            on_response: DefaultOnResponse::default(),
            on_request: DefaultOnRequest::default(),
            on_eos: DefaultOnEos::default(),
            on_failure: DefaultOnFailure::default(),
            _error: PhantomData,
        }
    }
}

impl<E> TraceLayer<SharedClassifier<GrpcErrorsAsFailures>, E> {
    /// Create a new [`TraceLayer`] using [`GrpcErrorsAsFailures`] which supports classifying
    /// gRPC responses and streams based on the `grpc-status` header.
    pub fn new_for_grpc() -> Self {
        Self {
            make_classifier: SharedClassifier::new::<E>(GrpcErrorsAsFailures::default()),
            make_span: DefaultMakeSpan::new(),
            add_headers_to_span: false,
            on_response: DefaultOnResponse::default(),
            on_request: DefaultOnRequest::default(),
            on_eos: DefaultOnEos::default(),
            on_failure: DefaultOnFailure::default(),
            _error: PhantomData,
        }
    }
}

impl<S, M, E, MakeSpan, OnRequest, OnResponse, OnEos, OnFailure> Layer<S>
    for TraceLayer<M, E, MakeSpan, OnRequest, OnResponse, OnEos, OnFailure>
where
    M: Clone,
    MakeSpan: Clone,
    OnRequest: Clone,
    OnResponse: Clone,
    OnEos: Clone,
    OnFailure: Clone,
{
    type Service = Trace<S, M, E, MakeSpan, OnRequest, OnResponse, OnEos, OnFailure>;

    fn layer(&self, inner: S) -> Self::Service {
        Trace {
            inner,
            make_classifier: self.make_classifier.clone(),
            make_span: self.make_span.clone(),
            on_request: self.on_request.clone(),
            on_eos: self.on_eos.clone(),
            on_response: self.on_response.clone(),
            on_failure: self.on_failure.clone(),
            add_headers_to_span: self.add_headers_to_span,
            _error: PhantomData,
        }
    }
}

impl<M, E, MakeSpan, OnRequest, OnResponse, OnEos, OnFailure> fmt::Debug
    for TraceLayer<M, E, MakeSpan, OnRequest, OnResponse, OnEos, OnFailure>
where
    M: fmt::Debug,
    MakeSpan: fmt::Debug,
    OnRequest: fmt::Debug,
    OnResponse: fmt::Debug,
    OnEos: fmt::Debug,
    OnFailure: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraceLayer")
            .field("make_classifier", &self.make_classifier)
            .field("make_span", &self.make_span)
            .field("add_headers_to_span", &self.add_headers_to_span)
            .field("on_request", &self.on_request)
            .field("on_response", &self.on_response)
            .field("on_eos", &self.on_eos)
            .field("on_failure", &self.on_failure)
            .finish()
    }
}
