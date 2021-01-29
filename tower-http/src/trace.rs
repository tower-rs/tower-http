//! Middleware that add high level [tracing] to a [`Service`].
//!
//! [tracing]: https://crates.io/crates/tracing
//! [`Service`]: tower_service::Service

use crate::classify_response::{
    ClassifyResponse, DefaultErrorClassification, DefaultHttpResponseClassifier,
    ResponseClassification,
};
use crate::LatencyUnit;
use futures_util::ready;
use http::{Request, Response, StatusCode};
use pin_project::pin_project;
use std::{future::Future, time::Duration};
use std::{marker::PhantomData, time::Instant};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;
use tracing::{field::debug, Level, Span};

/// [`Layer`] that adds high level [tracing] to a [`Service`].
///
/// [`Layer`]: tower_layer::Layer
/// [tracing]: https://crates.io/crates/tracing
/// [`Service`]: tower_service::Service
#[derive(Clone, Debug)]
pub struct TraceLayer<
    Classifier = DefaultHttpResponseClassifier,
    EventEmitter = DefaultEmitTracingEvents,
> {
    latency_unit: LatencyUnit,
    classifier: Classifier,
    event_emitter: EventEmitter,
}

impl<Classifier, EventEmitter> Default for TraceLayer<Classifier, EventEmitter>
where
    Classifier: Default,
    EventEmitter: Default,
{
    fn default() -> Self {
        Self {
            latency_unit: LatencyUnit::Millis,
            classifier: Classifier::default(),
            event_emitter: EventEmitter::default(),
        }
    }
}

impl TraceLayer {
    /// Create a new [`TraceLayer`] with the default configuration.
    pub fn new() -> Self {
        Self {
            latency_unit: LatencyUnit::Millis,
            classifier: DefaultHttpResponseClassifier::default(),
            event_emitter: DefaultEmitTracingEvents::default(),
        }
    }
}

impl<Classifier, EventEmitter> TraceLayer<Classifier, EventEmitter> {
    /// Change the latency unit events will use.
    #[inline]
    pub fn latency_unit(mut self, latency_unit: LatencyUnit) -> Self {
        self.latency_unit = latency_unit;
        self
    }

    /// Change the [`ClassifyResponse`] that will be used.
    ///
    /// [`ClassifyResponse`]: crate::classify_response::ClassifyResponse
    #[inline]
    pub fn classify_responses_with<NewClassifier>(
        self,
        classifier: NewClassifier,
    ) -> TraceLayer<NewClassifier, EventEmitter> {
        TraceLayer {
            classifier,
            latency_unit: self.latency_unit,
            event_emitter: self.event_emitter,
        }
    }

    /// Change the [`EmitTracingEvents`] that will be used.
    #[inline]
    pub fn emit_events_with<NewEventEmitter>(
        self,
        event_emitter: NewEventEmitter,
    ) -> TraceLayer<Classifier, NewEventEmitter> {
        TraceLayer {
            event_emitter,
            classifier: self.classifier,
            latency_unit: self.latency_unit,
        }
    }
}

impl<Classifier> TraceLayer<Classifier, DefaultEmitTracingEvents> {
    /// Change whether headers should be recorded on the span.
    ///
    /// Defaults to `false`.
    ///
    /// Note that this method is only accessible if you're using [`DefaultEmitTracingEvents`]
    #[inline]
    pub fn record_headers(mut self, record_headers: bool) -> Self {
        self.event_emitter.record_headers = record_headers;
        self
    }

    /// Change whether the full URI should be recorded on the span or if only the path should be
    /// recorded.
    ///
    /// Defaults to only recording the path no the full URI.
    ///
    /// Note that this method is only accessible if you're using [`DefaultEmitTracingEvents`]
    #[inline]
    pub fn record_full_uri(mut self, record_full_uri: bool) -> Self {
        self.event_emitter.record_full_uri = record_full_uri;
        self
    }
}

impl<S, Classifier, EventEmitter> Layer<S> for TraceLayer<Classifier, EventEmitter>
where
    Classifier: Clone,
    EventEmitter: Clone,
{
    type Service = Trace<S, Classifier, EventEmitter>;

    fn layer(&self, inner: S) -> Self::Service {
        Trace {
            inner,
            latency_unit: self.latency_unit,
            classifier: self.classifier.clone(),
            event_emitter: self.event_emitter.clone(),
        }
    }
}

/// Middleware that add high level [tracing] to a [`Service`].
///
/// [tracing]: https://crates.io/crates/tracing
/// [`Service`]: tower_service::Service
#[derive(Clone, Debug)]
pub struct Trace<S, Classifier, EventEmitter> {
    inner: S,
    latency_unit: LatencyUnit,
    classifier: Classifier,
    event_emitter: EventEmitter,
}

impl<ReqBody, ResBody, S, Classifier, EventEmitter> Service<Request<ReqBody>>
    for Trace<S, Classifier, EventEmitter>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    Classifier: ClassifyResponse<ResBody, S::Error> + Clone,
    EventEmitter: EmitTracingEvents<ReqBody, OkClass = Classifier::OkClass, ErrClass = Classifier::ErrClass>
        + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, Classifier, EventEmitter, ReqBody>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let start = Instant::now();

        let span = self.event_emitter.make_span(&req);

        let future = {
            let _guard = span.enter();
            self.event_emitter.on_request_received(&req);
            self.inner.call(req)
        };

        ResponseFuture {
            future,
            span,
            start,
            latency_unit: self.latency_unit,
            classifier: self.classifier.clone(),
            event_emitter: self.event_emitter.clone(),
            _marker: PhantomData,
        }
    }
}

/// The [`Future`] produced by [`Trace`] services.
///
/// [`Future`]: std::future::Future
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F, Classifier, EventEmitter, ReqBody> {
    #[pin]
    future: F,
    span: Span,
    start: Instant,
    latency_unit: LatencyUnit,
    classifier: Classifier,
    event_emitter: EventEmitter,
    _marker: PhantomData<fn() -> ReqBody>,
}

impl<F, ReqBody, ResBody, E, Classifier, EventEmitter> Future
    for ResponseFuture<F, Classifier, EventEmitter, ReqBody>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    Classifier: ClassifyResponse<ResBody, E>,
    EventEmitter:
        EmitTracingEvents<ReqBody, OkClass = Classifier::OkClass, ErrClass = Classifier::ErrClass>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let _guard = this.span.enter();

        let result = ready!(this.future.poll(cx));
        let latency = this.start.elapsed();

        let classification = this.classifier.classify_request_result(&result);
        this.event_emitter
            .on_classified_result(classification, latency, *this.latency_unit);

        Poll::Ready(result)
    }
}

/// Trait for emitting events and creating spans.
///
/// Designed to work with some implementation of [`ClassifyResponse`].
pub trait EmitTracingEvents<ReqBody> {
    /// The type used to classify successful responses.
    ///
    /// This mirrors [`ClassifyResponse::OkClass`].
    type OkClass;

    /// The type used to classify failed responses.
    ///
    /// This mirrors [`ClassifyResponse::ErrClass`].
    type ErrClass;

    /// Make a new [`tracing::Span`] from a request.
    ///
    /// [`tracing::Span`]: https://docs.rs/tracing/latest/tracing/struct.Span.html
    fn make_span(&self, req: &Request<ReqBody>) -> Span;

    /// Emit an event when a new request has been received.
    fn on_request_received(&self, req: &Request<ReqBody>);

    /// Emit an event when a request has been processed and the result has been classified.
    fn on_classified_result(
        &self,
        result: ResponseClassification<Self::OkClass, Self::ErrClass>,
        latency: Duration,
        latency_unit: LatencyUnit,
    );
}

/// The default [`EmitTracingEvents`] implementation used in [`Trace`].
#[derive(Debug, Clone, Default)]
pub struct DefaultEmitTracingEvents {
    record_headers: bool,
    record_full_uri: bool,
}

impl<ReqBody> EmitTracingEvents<ReqBody> for DefaultEmitTracingEvents {
    type OkClass = StatusCode;
    type ErrClass = DefaultErrorClassification;

    fn make_span(&self, req: &Request<ReqBody>) -> Span {
        let method = req.method();

        let path = if self.record_full_uri {
            req.uri().to_string()
        } else {
            req.uri().path().to_string()
        };

        let span = tracing::span!(
            Level::INFO,
            "http-request",
            method = %method,
            path = %path,
            headers = tracing::field::Empty,
        );

        if self.record_headers {
            span.record("headers", &debug(req.headers()));
        }

        span
    }

    fn on_request_received(&self, _req: &Request<ReqBody>) {
        tracing::info!(message = "received request");
    }

    fn on_classified_result(
        &self,
        result: ResponseClassification<StatusCode, DefaultErrorClassification>,
        latency: Duration,
        latency_unit: LatencyUnit,
    ) {
        match result {
            ResponseClassification::Ok(status) => match latency_unit {
                LatencyUnit::Millis => {
                    tracing::info!(
                        message = "completed request",
                        status = status.as_u16(),
                        latency_ms = %latency.as_millis(),
                    );
                }
                LatencyUnit::Nanos => {
                    tracing::info!(
                        message = "completed request",
                        status = status.as_u16(),
                        latency_ns = %latency.as_nanos(),
                    );
                }
            },
            ResponseClassification::Err(err) => match latency_unit {
                LatencyUnit::Millis => {
                    tracing::info!(
                        message = "request failed",
                        error = %err.err,
                        latency_ms = %latency.as_millis(),
                    );
                }
                LatencyUnit::Nanos => {
                    tracing::info!(
                        message = "request failed",
                        error = %err.err,
                        latency_ns = %latency.as_nanos(),
                    );
                }
            },
        }
    }
}
