use crate::{common::*, GetTraceStatus, GetTraceStatusFromHttpStatus, LatencyUnit, TraceStatus};
use http::{Method, Version};
use metrics::{gauge, histogram, increment_counter, SharedString};
use metrics_lib as metrics;
use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Instant,
};

#[derive(Debug)]
pub struct MetricsLayer<T> {
    latency_unit: LatencyUnit,
    get_trace_status: T,
    what_to_record: WhatToRecord,
}

impl Default for MetricsLayer<GetTraceStatusFromHttpStatus> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
struct WhatToRecord {
    path_label: bool,
    method_label: bool,
    http_version_label: bool,
    user_agent_label: bool,
    status_label: bool,
}

impl MetricsLayer<GetTraceStatusFromHttpStatus> {
    pub fn new() -> Self {
        Self {
            latency_unit: LatencyUnit::Millis,
            get_trace_status: GetTraceStatusFromHttpStatus,
            what_to_record: WhatToRecord {
                path_label: true,
                method_label: true,
                http_version_label: true,
                user_agent_label: true,
                status_label: true,
            },
        }
    }

    pub fn latency_unit(mut self, latency_unit: LatencyUnit) -> Self {
        self.latency_unit = latency_unit;
        self
    }

    pub fn path_label(mut self, record_path: bool) -> Self {
        self.what_to_record.path_label = record_path;
        self
    }

    pub fn method_label(mut self, record_method: bool) -> Self {
        self.what_to_record.method_label = record_method;
        self
    }

    pub fn http_version_label(mut self, record_http_version: bool) -> Self {
        self.what_to_record.http_version_label = record_http_version;
        self
    }

    pub fn user_agent_label(mut self, record_user_agent: bool) -> Self {
        self.what_to_record.user_agent_label = record_user_agent;
        self
    }

    pub fn status_label(mut self, record_status: bool) -> Self {
        self.what_to_record.status_label = record_status;
        self
    }

    pub fn get_trace_status<K>(self, get_trace_status: K) -> MetricsLayer<K> {
        MetricsLayer {
            get_trace_status,
            latency_unit: self.latency_unit,
            what_to_record: self.what_to_record,
        }
    }
}

impl<S, T> Layer<S> for MetricsLayer<T>
where
    T: Clone,
{
    type Service = Metrics<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        Metrics {
            inner,
            latency_unit: self.latency_unit,
            get_trace_status: self.get_trace_status.clone(),
            what_to_record: self.what_to_record,
            in_flight_requests: ShareableCounter::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Metrics<S, T> {
    inner: S,
    latency_unit: LatencyUnit,
    get_trace_status: T,
    what_to_record: WhatToRecord,
    in_flight_requests: ShareableCounter,
}

impl<ReqBody, ResBody, S, T> Service<Request<ReqBody>> for Metrics<S, T>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    T: Clone + GetTraceStatus<S::Response, S::Error>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, T>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let start = Instant::now();
        self.in_flight_requests.increment();

        let path = then(self.what_to_record.path_label, || {
            req.uri().path().to_owned()
        });
        let method = then(self.what_to_record.method_label, || req.method().to_owned());
        let http_version = then(self.what_to_record.http_version_label, || req.version());

        let user_agent = then(self.what_to_record.user_agent_label, || {
            req.headers()
                .get(header::USER_AGENT)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_owned())
        })
        .flatten();

        ResponseFuture {
            future: self.inner.call(req),
            start,
            latency_unit: self.latency_unit,
            get_trace_status: self.get_trace_status.clone(),
            path,
            method,
            http_version,
            user_agent,
            in_flight_requests: self.in_flight_requests.clone(),
            what_to_record: self.what_to_record,
        }
    }
}

// when `bool::then` is stabalized we can remove this
fn then<F, T>(cond: bool, f: F) -> Option<T>
where
    F: FnOnce() -> T,
{
    if cond {
        Some(f())
    } else {
        None
    }
}

#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F, T> {
    #[pin]
    future: F,
    start: Instant,
    latency_unit: LatencyUnit,
    get_trace_status: T,
    path: Option<String>,
    method: Option<Method>,
    http_version: Option<Version>,
    user_agent: Option<String>,
    in_flight_requests: ShareableCounter,
    what_to_record: WhatToRecord,
}

impl<F, ResBody, E, T> Future for ResponseFuture<F, T>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    T: GetTraceStatus<Response<ResBody>, E>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = ready!(this.future.poll(cx));
        let duration = this.start.elapsed();

        let mut labels = Vec::with_capacity(5 /* the max number of labels */);

        if let Some(path) = this.path.take() {
            labels.push(("path", SharedString::from(path)));
        }

        if let Some(method) = this.method.take() {
            labels.push(("method", SharedString::from(method.to_string())));
        }

        if let Some(http_version) = this.http_version.take() {
            labels.push((
                "http_version",
                SharedString::from(format!("{:?}", http_version)),
            ));
        }

        if let Some(user_agent) = this.user_agent.take() {
            labels.push(("user_agent", SharedString::from(user_agent)));
        }

        if this.what_to_record.status_label {
            match this.get_trace_status.trace_status(&result) {
                TraceStatus::Status(status) => {
                    labels.push(("status", SharedString::from(status.to_string())));
                }
                TraceStatus::Error => {
                    labels.push(("status", "error".into()));
                }
            }
        }

        match this.latency_unit {
            LatencyUnit::Nanos => {
                histogram!("latency_ns", duration.as_nanos() as f64, &labels);
            }
            LatencyUnit::Millis => {
                histogram!("latency_ms", duration.as_millis() as f64, &labels);
            }
        }

        increment_counter!("http_requests_total", &labels);

        gauge!("in_flight_requests", this.in_flight_requests.get() as f64);

        this.in_flight_requests.decrement();

        Poll::Ready(result)
    }
}

#[derive(Clone, Debug)]
struct ShareableCounter {
    count: Arc<AtomicU32>,
}

impl ShareableCounter {
    fn new() -> Self {
        Self {
            count: Default::default(),
        }
    }

    fn get(&self) -> u32 {
        self.count.load(Ordering::Relaxed)
    }

    fn increment(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }

    fn decrement(&self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }
}
