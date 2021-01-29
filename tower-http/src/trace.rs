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
use tracing::{
    field::{debug, display},
    Level, Span,
};

#[derive(Clone, Debug)]
pub struct TraceLayer<T = GetTraceStatusFromHttpStatus> {
    record_headers: bool,
    span: Option<Span>,
    latency_unit: LatencyUnit,
    get_trace_status: T,
    record_full_uri: bool,
}

impl Default for TraceLayer<GetTraceStatusFromHttpStatus> {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceLayer<GetTraceStatusFromHttpStatus> {
    pub fn new() -> Self {
        Self {
            record_headers: false,
            span: None,
            latency_unit: LatencyUnit::Millis,
            get_trace_status: GetTraceStatusFromHttpStatus(()),
            record_full_uri: false,
        }
    }
}

impl<T> TraceLayer<T> {
    pub fn record_headers(mut self, record_headers: bool) -> Self {
        self.record_headers = record_headers;
        self
    }

    /// Provide a custom span. The span is expected to at least have the fields `method`, `path`,
    /// and `headers`. If any of the fields are missing from the span they'll also be missing from
    /// whatever output you may have configured.
    ///
    /// The default span uses `INFO` level and is called `http-request`.
    pub fn span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn latency_unit(mut self, latency_unit: LatencyUnit) -> Self {
        self.latency_unit = latency_unit;
        self
    }

    pub fn record_full_uri(mut self, record_full_uri: bool) -> Self {
        self.record_full_uri = record_full_uri;
        self
    }

    pub fn get_trace_status<K>(self, get_trace_status: K) -> TraceLayer<K> {
        TraceLayer {
            record_headers: self.record_headers,
            span: self.span,
            latency_unit: self.latency_unit,
            get_trace_status,
            record_full_uri: self.record_full_uri,
        }
    }
}

impl<S, T> Layer<S> for TraceLayer<T>
where
    T: Clone,
{
    type Service = Trace<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        Trace {
            inner,
            record_headers: self.record_headers,
            span: self.span.clone(),
            latency_unit: self.latency_unit,
            get_trace_status: self.get_trace_status.clone(),
            record_full_uri: self.record_full_uri,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Trace<S, T> {
    inner: S,
    record_headers: bool,
    span: Option<Span>,
    latency_unit: LatencyUnit,
    get_trace_status: T,
    record_full_uri: bool,
}

impl<ReqBody, ResBody, S, T> Service<Request<ReqBody>> for Trace<S, T>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    T: Clone + GetTraceStatus<Response<ResBody>, S::Error>,
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

        let method = req.method();
        let path = if self.record_full_uri {
            req.uri().to_string()
        } else {
            req.uri().path().to_string()
        };

        let span = if let Some(span) = &self.span {
            let span = span.clone();
            span.record("method", &display(method));
            span.record("path", &display(path));
            span
        } else {
            tracing::span!(
                Level::INFO,
                "http-request",
                method = %method,
                path = %path,
                headers = tracing::field::Empty,
            )
        };

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
