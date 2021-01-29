use crate::LatencyUnit;
use crate::{GetTraceStatus, GetTraceStatusFromHttpStatus, TraceStatus};
use futures_util::ready;
use http::{Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::time::Instant;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;
use tracing::{field::debug, Level, Span};

#[derive(Clone, Debug)]
pub struct TraceLayer<MakeSpan, T = GetTraceStatusFromHttpStatus> {
    record_headers: bool,
    latency_unit: LatencyUnit,
    get_trace_status: T,
    record_full_uri: bool,
    make_span: MakeSpan,
}

impl<B> Default for TraceLayer<fn(&Request<B>) -> Span, GetTraceStatusFromHttpStatus> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B> TraceLayer<fn(&Request<B>) -> Span, GetTraceStatusFromHttpStatus> {
    pub fn new() -> Self {
        Self {
            record_headers: false,
            latency_unit: LatencyUnit::Millis,
            get_trace_status: GetTraceStatusFromHttpStatus(()),
            record_full_uri: false,
            make_span: default_make_span,
        }
    }
}

fn default_make_span<B>(req: &Request<B>) -> Span {
    let method = req.method();
    let path = req.uri().to_string();

    tracing::span!(
        Level::INFO,
        "http-request",
        method = %method,
        path = %path,
        headers = tracing::field::Empty,
    )
}

impl<MakeSpan, T> TraceLayer<MakeSpan, T> {
    pub fn record_headers(mut self, record_headers: bool) -> Self {
        self.record_headers = record_headers;
        self
    }

    /// Provide a closure to create the span. The span is expected to at least have the fields
    /// `method`, `path`, and `headers`. If any of the fields are missing from the span they'll
    /// also be missing from whatever output you may have configured.
    ///
    /// The default span uses `INFO` level and is called `http-request`.
    pub fn span<NewMakeSpan>(self, make_span: NewMakeSpan) -> TraceLayer<NewMakeSpan, T> {
        TraceLayer {
            make_span,
            get_trace_status: self.get_trace_status,
            record_headers: self.record_headers,
            latency_unit: self.latency_unit,
            record_full_uri: self.record_full_uri,
        }
    }

    pub fn latency_unit(mut self, latency_unit: LatencyUnit) -> Self {
        self.latency_unit = latency_unit;
        self
    }

    pub fn record_full_uri(mut self, record_full_uri: bool) -> Self {
        self.record_full_uri = record_full_uri;
        self
    }

    pub fn get_trace_status<K>(self, get_trace_status: K) -> TraceLayer<MakeSpan, K> {
        TraceLayer {
            get_trace_status,
            record_headers: self.record_headers,
            latency_unit: self.latency_unit,
            record_full_uri: self.record_full_uri,
            make_span: self.make_span,
        }
    }
}

impl<MakeSpan, S, T> Layer<S> for TraceLayer<MakeSpan, T>
where
    T: Clone,
    MakeSpan: Clone,
{
    type Service = Trace<S, MakeSpan, T>;

    fn layer(&self, inner: S) -> Self::Service {
        Trace {
            inner,
            record_headers: self.record_headers,
            latency_unit: self.latency_unit,
            get_trace_status: self.get_trace_status.clone(),
            record_full_uri: self.record_full_uri,
            make_span: self.make_span.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Trace<S, MakeSpan, T> {
    inner: S,
    record_headers: bool,
    latency_unit: LatencyUnit,
    get_trace_status: T,
    record_full_uri: bool,
    make_span: MakeSpan,
}

impl<ReqBody, ResBody, S, T, MakeSpan> Service<Request<ReqBody>> for Trace<S, MakeSpan, T>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    T: Clone + GetTraceStatus<Response<ResBody>, S::Error>,
    MakeSpan: Fn(&Request<ReqBody>) -> Span,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, T>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let start = Instant::now();

        let span = (self.make_span)(&req);

        if self.record_headers {
            span.record("headers", &debug(req.headers()));
        }

        let future = {
            let _guard = span.enter();
            tracing::info!(message = "received request");
            self.inner.call(req)
        };

        ResponseFuture {
            future,
            span,
            start,
            latency_unit: self.latency_unit,
            get_trace_status: self.get_trace_status.clone(),
        }
    }
}

#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F, T> {
    #[pin]
    future: F,
    span: Span,
    start: Instant,
    latency_unit: LatencyUnit,
    get_trace_status: T,
}

impl<F, ResBody, E, T> Future for ResponseFuture<F, T>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    T: GetTraceStatus<Response<ResBody>, E>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let _guard = this.span.enter();

        let result = ready!(this.future.poll(cx));
        let time = this.start.elapsed();

        match (
            this.get_trace_status.trace_status(&result),
            *this.latency_unit,
        ) {
            (TraceStatus::Status(status), LatencyUnit::Nanos) => {
                tracing::info!(
                    message = "completed request",
                    status = status,
                    latency_ns = %time.as_nanos(),
                );
            }
            (TraceStatus::Status(status), LatencyUnit::Millis) => {
                tracing::info!(
                    message = "completed request",
                    status = status,
                    latency_ms = %time.as_millis(),
                );
            }
            (TraceStatus::Error, LatencyUnit::Nanos) => {
                tracing::info!(
                    message = "request failed",
                    latency_ns = %time.as_nanos(),
                );
            }
            (TraceStatus::Error, LatencyUnit::Millis) => {
                tracing::info!(
                    message = "request failed",
                    latency_ms = %time.as_millis(),
                );
            }
        }

        Poll::Ready(result)
    }
}
