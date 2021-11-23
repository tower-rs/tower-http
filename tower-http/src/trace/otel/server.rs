//! OpenTelemetry [semantic conventions for HTTP spans][otel] for servers.
//!
//! # Span fields
//!
//! [`TraceLayer::opentelemetry_server`] and [`Trace::opentelemetry_server`] will add the following
//! fields to the request span:
//!
//! - `http.method`: [`Method`] of the incoming request.
//! - `http.route`: The path of the request as returned by [`ExtractMatchedPath`]. Use
//! [`OtelConfig::extract_matched_path_with`] to customize this. Defaults to the requests exact path.
//! - `http.flavor`: [`Version`] of the incoming request.
//! - `http.scheme`: [`Scheme`] used by the server. Use [`OtelConfig::scheme`] to customize this.
//! Defaults to HTTP.
//! - `http.host`: `Host` header of the incoming request.
//! - `http.client_ip`: Address of the connected client. Use [`OtelConfig::extract_client_ip_with`] to
//! customize this. Defaults to being empty.
//! - `http.user_agent`: `User-Agent` header of the incoming request.
//! - `http.target`: The requests exact path.
//! - `http.status_code`: The response status code.
//! - `request_id`: The request's id. Requires using the [`SetRequestId`] middleware.
//! - `trace_id`: Trace ID for the parent trace context. Use [`OtelConfig::set_otel_parent_with`] to
//! customize this.
//! - `exception.message`: The error message, if any. Applied if the [classifier] you're using
//! deems the response to be a failure. See ["Customizing `exception.message` and `exception.details`".][exec]
//! - `exception.details`: The error details, if any. See ["Customizing `exception.message` and
//! `exception.details`".][exec]
//! - `otel.kind`: Always `"server"`.
//! - `otel.status_code`: Whether the response was an error or not, as determined by the
//! [classifier].
//!
//! [`SetRequestId`]: crate::request_id::SetRequestId
//! [classifier]: crate::classify::ClassifyResponse
//! [exec]: #customizing-exceptionmessage-and-exceptiondetails
//!
//! # Example
//!
//! ```
//! use tower_http::trace::{
//!     TraceLayer,
//!     otel::server::{
//!         OtelConfig,
//!         ExtractMatchedPath,
//!         ExtractClientIp,
//!         SetOtelParent,
//!     },
//! };
//! use http::{uri::{Scheme, Uri}, HeaderMap, Request, Response, Extensions};
//! use tracing::Span;
//! use std::borrow::Cow;
//! use hyper::{Body, Error};
//! use tower::ServiceBuilder;
//!
//! let otel_config = OtelConfig::default()
//!     .scheme(Scheme::HTTP)
//!     .extract_matched_path_with(MyOtelConfig)
//!     .extract_client_ip_with(MyOtelConfig)
//!     .set_otel_parent_with(MyOtelConfig);
//!
//! let service = ServiceBuilder::new()
//!     .layer(TraceLayer::new_for_http().opentelemetry_server(otel_config))
//!     .service_fn(handler);
//!
//! async fn handler(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     // ...
//!     # todo!()
//! }
//!
//! #[derive(Copy, Clone)]
//! struct MyOtelConfig;
//!
//! impl ExtractMatchedPath for MyOtelConfig {
//!     fn extract_matched_path<'a>(
//!         &self,
//!         uri: &'a Uri,
//!         headers: &'a HeaderMap,
//!         extensions: &'a Extensions,
//!     ) -> Cow<'a, str> {
//!         // ...
//!         # unimplemented!()
//!     }
//! }
//!
//! impl ExtractClientIp for MyOtelConfig {
//!     fn extract_client_ip<'a>(
//!         &self,
//!         uri: &'a Uri,
//!         headers: &'a HeaderMap,
//!         extensions: &'a Extensions,
//!     ) -> Option<Cow<'a, str>> {
//!         // ...
//!         # unimplemented!()
//!     }
//! }
//!
//! impl SetOtelParent for MyOtelConfig {
//!     fn set_otel_parent(
//!         &self,
//!         uri: &Uri,
//!         headers: &HeaderMap,
//!         extensions: &Extensions,
//!         span: &Span,
//!     ) {
//!         // ...
//!         # unimplemented!()
//!     }
//! }
//! ```
//!
//! See [axum-key-value-store] for a complete example that also sends traces to a collector.
//!
//! [axum-key-value-store]: https://github.com/tower-rs/tower-http/tree/master/examples/axum-key-value-store
//!
//! # Using functions for customization
//!
//! ```
//! use tower_http::trace::{TraceLayer, otel::server::OtelConfig};
//! use http::{uri::{Scheme, Uri}, HeaderMap, Request, Response, Extensions};
//! use tracing::Span;
//! use std::borrow::Cow;
//! use hyper::{Body, Error};
//! use tower::ServiceBuilder;
//!
//! let otel_config = OtelConfig::default()
//!     .extract_matched_path_with(extract_matched_path)
//!     .extract_client_ip_with(extract_client_ip)
//!     .set_otel_parent_with(set_otel_parent);
//!
//! let service = ServiceBuilder::new()
//!     .layer(TraceLayer::new_for_http().opentelemetry_server(otel_config))
//!     .service_fn(handler);
//!
//! async fn handler(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     // ...
//!     # todo!()
//! }
//!
//! // The functions most be defined as standalone functions like so
//! // otherwise Rust cannot infer the correct lifetimes of the arguments.
//! //
//! // If you need to pass state to the callbacks consider making a struct and
//! // implementing the corresponding trait instead.
//!
//! fn extract_matched_path<'a>(
//!     uri: &'a Uri,
//!     headers: &'a HeaderMap,
//!     extensions: &'a Extensions,
//! ) -> Cow<'a, str> {
//!     # unimplemented!();
//!     // ...
//! }
//!
//! fn extract_client_ip<'a>(
//!     uri: &'a Uri,
//!     headers: &'a HeaderMap,
//!     extensions: &'a Extensions,
//! ) -> Option<Cow<'a, str>> {
//!     # unimplemented!();
//!     // ...
//! }
//!
//! fn set_otel_parent(
//!     uri: &Uri,
//!     headers: &HeaderMap,
//!     extensions: &Extensions,
//!     span: &Span,
//! ) {
//!     # unimplemented!();
//!     // ...
//! }
//! ```
//!
//! # Request ids
//!
//! Request ids applied with the [`SetRequestId`] middleware will be automatically picked up:
//!
//! ```
//! use tower_http::{
//!     trace::{TraceLayer, otel::server::OtelConfig},
//!     request_id::{SetRequestIdLayer, MakeRequestId, RequestId},
//! };
//! use http::{uri::{Scheme, Uri}, HeaderMap, Request, Response, Extensions};
//! use tracing::Span;
//! use std::borrow::Cow;
//! use hyper::{Body, Error};
//! use tower::ServiceBuilder;
//!
//! let service = ServiceBuilder::new()
//!     // make sure you're adding the middleware above `TraceLayer`
//!     .layer(SetRequestIdLayer::x_request_id(MyMakeRequestId))
//!     .layer(TraceLayer::new_for_http().opentelemetry_server(OtelConfig::default()))
//!     .service_fn(handler);
//!
//! async fn handler(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     // ...
//!     # todo!()
//! }
//!
//! #[derive(Clone, Copy)]
//! struct MyMakeRequestId;
//!
//! impl MakeRequestId for MyMakeRequestId {
//!     fn make_request_id<B>(&mut self, request: &Request<B>) -> Option<RequestId> {
//!         # unimplemented!()
//!         // ...
//!     }
//! }
//! ```
//!
//! # Customizing `exception.message` and `exception.details`
//!
//! [otel]: https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/trace/semantic_conventions/http.md

use crate::{
    classify::{MakeClassifier, ServerErrorsFailureClass},
    request_id::RequestId,
    trace::{MakeSpan, OnBodyChunk, OnEos, OnFailure, OnRequest, OnResponse, Trace, TraceLayer},
};
use http::{header, uri::Scheme, Extensions, HeaderMap, Method, Request, Response, Uri, Version};
use std::{borrow::Cow, sync::Arc, time::Duration};
use tracing::{field::Empty, Span};

pub struct OtelConfig {
    scheme: Cow<'static, str>,
    // these are trait objects such that we can add more callbacks without
    // adding more type params, which would be a breaking change
    extract_matched_path: Arc<dyn ExtractMatchedPath>,
    extract_client_ip: Arc<dyn ExtractClientIp>,
    set_otel_parent: Arc<dyn SetOtelParent>,
}

impl OtelConfig {
    pub fn new() -> Self {
        Self {
            scheme: http_scheme(&Scheme::HTTP),
            extract_matched_path: Arc::new(DefaultExtractMatchedPath),
            extract_client_ip: Arc::new(DefaultExtractClientIp),
            set_otel_parent: Arc::new(DefaultSetOtelParent),
        }
    }

    pub fn scheme(mut self, scheme: Scheme) -> Self {
        self.scheme = http_scheme(&scheme);
        self
    }

    pub fn extract_matched_path_with<T>(mut self, extract_matched_path: T) -> OtelConfig
    where
        T: ExtractMatchedPath,
    {
        self.extract_matched_path = Arc::new(extract_matched_path);
        self
    }

    pub fn extract_client_ip_with<T>(mut self, extract_client_ip: T) -> OtelConfig
    where
        T: ExtractClientIp,
    {
        self.extract_client_ip = Arc::new(extract_client_ip);
        self
    }

    pub fn set_otel_parent_with<T>(mut self, set_otel_parent: T) -> OtelConfig
    where
        T: SetOtelParent,
    {
        self.set_otel_parent = Arc::new(set_otel_parent);
        self
    }
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> TraceLayer<M> {
    /// Change this layer to use OpenTelemetry's [semantic conventions for HTTP spans][otel].
    ///
    /// Note this overrides all callbacks added previously.
    ///
    /// See [`tower_http::trace::otel::server`](self) for more details and examples.
    ///
    /// [otel]: https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/trace/semantic_conventions/http.md
    pub fn opentelemetry_server(
        self,
        config: OtelConfig,
    ) -> TraceLayer<
        M,
        OtelMakeSpan,
        OtelOnRequest,
        OtelOnResponse,
        OtelOnBodyChunk,
        OtelOnEos,
        OtelOnFailure,
    >
    where
        M: MakeClassifier,
        M::FailureClass: FailureDetails,
    {
        let OtelConfig {
            scheme,
            extract_matched_path,
            extract_client_ip,
            set_otel_parent,
        } = config;

        TraceLayer {
            make_classifier: self.make_classifier,
            make_span: OtelMakeSpan {
                scheme,
                extract_matched_path,
                extract_client_ip,
                set_otel_parent,
            },
            on_request: OtelOnRequest,
            on_response: OtelOnResponse,
            on_body_chunk: OtelOnBodyChunk,
            on_eos: OtelOnEos,
            on_failure: OtelOnFailure,
        }
    }
}

impl<S, M> Trace<S, M> {
    /// Change this middleware to use OpenTelemetry's [semantic conventions for HTTP spans][otel].
    ///
    /// Note this overrides all callbacks added previously.
    ///
    /// See [`tower_http::trace::otel::server`](self) for more details and examples.
    ///
    /// [otel]: https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/trace/semantic_conventions/http.md
    pub fn opentelemetry_server(
        self,
        config: OtelConfig,
    ) -> Trace<
        S,
        M,
        OtelMakeSpan,
        OtelOnRequest,
        OtelOnResponse,
        OtelOnBodyChunk,
        OtelOnEos,
        OtelOnFailure,
    >
    where
        M: MakeClassifier,
        M::FailureClass: FailureDetails,
    {
        let OtelConfig {
            scheme,
            extract_matched_path,
            extract_client_ip,
            set_otel_parent,
        } = config;

        Trace {
            inner: self.inner,
            make_classifier: self.make_classifier,
            make_span: OtelMakeSpan {
                scheme,
                extract_matched_path,
                extract_client_ip,
                set_otel_parent,
            },
            on_request: OtelOnRequest,
            on_response: OtelOnResponse,
            on_body_chunk: OtelOnBodyChunk,
            on_eos: OtelOnEos,
            on_failure: OtelOnFailure,
        }
    }
}

#[derive(Clone)]
pub struct OtelMakeSpan {
    scheme: Cow<'static, str>,
    extract_matched_path: Arc<dyn ExtractMatchedPath>,
    extract_client_ip: Arc<dyn ExtractClientIp>,
    set_otel_parent: Arc<dyn SetOtelParent>,
}

impl<B> MakeSpan<B> for OtelMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        let user_agent = request
            .headers()
            .get(header::USER_AGENT)
            .map(|h| h.to_str().unwrap_or(""))
            .unwrap_or("");

        let host = request
            .headers()
            .get(header::HOST)
            .map(|h| h.to_str().unwrap_or(""))
            .unwrap_or("");

        let http_route = self.extract_matched_path.extract_matched_path(
            request.uri(),
            request.headers(),
            request.extensions(),
        );

        let client_ip = self
            .extract_client_ip
            .extract_client_ip(request.uri(), request.headers(), request.extensions())
            .unwrap_or_default();

        let span = tracing::info_span!(
            "HTTP request",
            http.method = %http_method(request.method()),
            http.route = %http_route,
            http.flavor = %http_flavor(request.version()),
            http.scheme = %self.scheme,
            http.host = %host,
            http.client_ip = %client_ip,
            http.user_agent = %user_agent,
            http.target = %request.uri().path_and_query().map(|p| p.as_str()).unwrap_or(""),
            http.status_code = Empty,
            otel.kind = "server",
            otel.status_code = Empty,
            trace_id = Empty,
            request_id = Empty,
            exception.message = Empty,
            exception.details = Empty,
        );

        if let Some(request_id) = request
            .extensions()
            .get::<RequestId>()
            .and_then(|id| id.header_value().to_str().ok())
        {
            span.record("request_id", &request_id);
        }

        self.set_otel_parent.set_otel_parent(
            request.uri(),
            request.headers(),
            request.extensions(),
            &span,
        );

        span
    }
}

fn http_method(method: &Method) -> Cow<'static, str> {
    match method {
        &Method::CONNECT => "CONNECT".into(),
        &Method::DELETE => "DELETE".into(),
        &Method::GET => "GET".into(),
        &Method::HEAD => "HEAD".into(),
        &Method::OPTIONS => "OPTIONS".into(),
        &Method::PATCH => "PATCH".into(),
        &Method::POST => "POST".into(),
        &Method::PUT => "PUT".into(),
        &Method::TRACE => "TRACE".into(),
        other => other.to_string().into(),
    }
}

pub trait ExtractMatchedPath: Send + Sync + 'static {
    fn extract_matched_path<'a>(
        &self,
        uri: &'a Uri,
        headers: &'a HeaderMap,
        extensions: &'a Extensions,
    ) -> Cow<'a, str>;
}

impl<F> ExtractMatchedPath for F
where
    F: for<'a> Fn(&'a Uri, &'a HeaderMap, &'a Extensions) -> Cow<'a, str> + Send + Sync + 'static,
{
    fn extract_matched_path<'a>(
        &self,
        uri: &'a Uri,
        headers: &'a HeaderMap,
        extensions: &'a Extensions,
    ) -> Cow<'a, str> {
        self(uri, headers, extensions)
    }
}

#[derive(Clone, Copy)]
struct DefaultExtractMatchedPath;

impl ExtractMatchedPath for DefaultExtractMatchedPath {
    fn extract_matched_path<'a>(
        &self,
        uri: &'a Uri,
        _headers: &'a HeaderMap,
        _extensions: &'a Extensions,
    ) -> Cow<'a, str> {
        uri.path().into()
    }
}

pub trait ExtractClientIp: Send + Sync + 'static {
    fn extract_client_ip<'a>(
        &self,
        uri: &'a Uri,
        headers: &'a HeaderMap,
        extensions: &'a Extensions,
    ) -> Option<Cow<'a, str>>;
}

impl<F> ExtractClientIp for F
where
    F: for<'a> Fn(&'a Uri, &'a HeaderMap, &'a Extensions) -> Option<Cow<'a, str>>
        + Send
        + Sync
        + 'static,
{
    fn extract_client_ip<'a>(
        &self,
        uri: &'a Uri,
        headers: &'a HeaderMap,
        extensions: &'a Extensions,
    ) -> Option<Cow<'a, str>> {
        self(uri, headers, extensions)
    }
}

#[derive(Clone, Copy)]
struct DefaultExtractClientIp;

impl ExtractClientIp for DefaultExtractClientIp {
    fn extract_client_ip<'a>(
        &self,
        _uri: &'a Uri,
        _headers: &'a HeaderMap,
        _extensions: &'a Extensions,
    ) -> Option<Cow<'a, str>> {
        None
    }
}

// NOTE: document should also record trace_id on span a la
pub trait SetOtelParent: Send + Sync + 'static {
    fn set_otel_parent(&self, uri: &Uri, headers: &HeaderMap, extensions: &Extensions, span: &Span);
}

impl<F> SetOtelParent for F
where
    F: for<'a> Fn(&'a Uri, &'a HeaderMap, &'a Extensions, &'a Span) + Send + Sync + 'static,
{
    fn set_otel_parent(
        &self,
        uri: &Uri,
        headers: &HeaderMap,
        extensions: &Extensions,
        span: &Span,
    ) {
        self(uri, headers, extensions, span)
    }
}

#[derive(Clone, Copy)]
struct DefaultSetOtelParent;

impl SetOtelParent for DefaultSetOtelParent {
    fn set_otel_parent(
        &self,
        _uri: &Uri,
        _headers: &HeaderMap,
        _extensions: &Extensions,
        _span: &Span,
    ) {
    }
}

fn http_flavor(version: Version) -> Cow<'static, str> {
    match version {
        Version::HTTP_09 => "0.9".into(),
        Version::HTTP_10 => "1.0".into(),
        Version::HTTP_11 => "1.1".into(),
        Version::HTTP_2 => "2.0".into(),
        Version::HTTP_3 => "3.0".into(),
        other => format!("{:?}", other).into(),
    }
}

fn http_scheme(scheme: &Scheme) -> Cow<'static, str> {
    if scheme == &Scheme::HTTP {
        "http".into()
    } else if scheme == &Scheme::HTTPS {
        "https".into()
    } else {
        scheme.to_string().into()
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OtelOnRequest;

impl<B> OnRequest<B> for OtelOnRequest {
    fn on_request(&mut self, _request: &Request<B>, _span: &Span) {}
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OtelOnResponse;

impl<B> OnResponse<B> for OtelOnResponse {
    fn on_response(self, response: &Response<B>, _latency: Duration, span: &Span) {
        let status = response.status().as_u16().to_string();
        span.record("http.status_code", &tracing::field::display(status));
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OtelOnBodyChunk;

impl<B> OnBodyChunk<B> for OtelOnBodyChunk {
    fn on_body_chunk(&mut self, _chunk: &B, _latency: Duration, _span: &Span) {}
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OtelOnEos;

impl OnEos for OtelOnEos {
    fn on_eos(self, _trailers: Option<&HeaderMap>, _stream_duration: Duration, _span: &Span) {}
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OtelOnFailure;

impl<E> OnFailure<E> for OtelOnFailure
where
    E: FailureDetails,
{
    fn on_failure(&mut self, failure: E, _latency: Duration, span: &Span) {
        span.record("otel.status_code", &"ERROR");

        if let Some(message) = failure.message() {
            span.record("exception.message", &tracing::field::display(message));
        }

        if let Some(details) = failure.details() {
            span.record("exception.details", &tracing::field::display(details));
        }
    }
}

pub trait FailureDetails {
    fn message(&self) -> Option<String>;

    fn details(&self) -> Option<String>;
}

impl FailureDetails for ServerErrorsFailureClass {
    fn message(&self) -> Option<String> {
        match self {
            ServerErrorsFailureClass::StatusCode(_) => None,
            ServerErrorsFailureClass::Error(err) => Some(err.to_owned()),
        }
    }

    fn details(&self) -> Option<String> {
        None
    }
}
