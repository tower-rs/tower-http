#[cfg(feature = "add-extension")]
use crate::add_extension::AddExtension;
#[cfg(all(feature = "validate-request", feature = "auth"))]
use crate::auth::require_authorization::{Basic, Bearer};
#[cfg(feature = "auth")]
use crate::auth::{AddAuthorization, AsyncRequireAuthorization};
#[cfg(feature = "catch-panic")]
use crate::catch_panic::{CatchPanic, DefaultResponseForPanic, ResponseForPanic};
#[cfg(any(
    feature = "compression-br",
    feature = "compression-deflate",
    feature = "compression-gzip",
    feature = "compression-zstd"
))]
use crate::compression::{Compression, DefaultPredicate, Predicate};
#[cfg(feature = "cors")]
use crate::cors::Cors;
#[cfg(any(
    feature = "decompression-br",
    feature = "decompression-deflate",
    feature = "decompression-gzip",
    feature = "decompression-zstd"
))]
use crate::decompression::{Decompression, RequestDecompression};
#[cfg(feature = "follow-redirect")]
use crate::follow_redirect::{policy::Standard, FollowRedirect};
#[cfg(feature = "limit")]
use crate::limit::RequestBodyLimit;
#[cfg(feature = "map-request-body")]
use crate::map_request_body::MapRequestBody;
#[cfg(feature = "map-response-body")]
use crate::map_response_body::MapResponseBody;
#[cfg(feature = "metrics")]
use crate::metrics::in_flight_requests::{InFlightRequests, InFlightRequestsCounter};
#[cfg(feature = "normalize-path")]
use crate::normalize_path::NormalizePath;
#[cfg(feature = "propagate-header")]
use crate::propagate_header::PropagateHeader;
#[cfg(feature = "request-id")]
use crate::request_id::{MakeRequestId, PropagateRequestId, SetRequestId, X_REQUEST_ID};
#[cfg(feature = "sensitive-headers")]
use crate::sensitive_headers::{
    SetSensitiveHeaders, SetSensitiveRequestHeaders, SetSensitiveResponseHeaders,
};
#[cfg(feature = "set-header")]
use crate::set_header::{SetRequestHeader, SetResponseHeader};
#[cfg(feature = "set-status")]
use crate::set_status::SetStatus;
#[cfg(feature = "validate-request")]
use crate::validate_request::{AcceptHeader, ValidateRequestHeader};
#[cfg(feature = "trace")]
use crate::{
    classify::{GrpcErrorsAsFailures, MakeClassifier, ServerErrorsAsFailures, SharedClassifier},
    trace::{
        DefaultMakeSpan, DefaultOnBodyChunk, DefaultOnEos, DefaultOnFailure, DefaultOnRequest,
        DefaultOnResponse, Trace,
    },
};
#[cfg(feature = "timeout")]
use {
    crate::timeout::{RequestBodyTimeout, ResponseBodyTimeout, Timeout},
    std::time::Duration,
};
#[allow(unused_imports)]
use {
    http::{header::HeaderName, status::StatusCode},
    http_body::Body,
};

/// An extension trait for `Service`s that provides a variety of convenient
/// adapters
pub trait ServiceExt<Request>: tower_service::Service<Request> {
    /// Create a new middleware for adding some shareable value to [request extensions].
    ///
    /// See the [add_extension](crate::add_extension) for more details.
    ///
    /// [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
    #[cfg(feature = "add-extension")]
    fn add_extension<T>(self, value: T) -> AddExtension<Self, T>
    where
        Self: Sized,
    {
        AddExtension::new(self, value)
    }

    /// Authorize requests using a username and password pair.
    ///
    /// The `Authorization` header will be set to `Basic {credentials}` where `credentials` is
    /// `base64_encode("{username}:{password}")`.
    ///
    /// Since the username and password is sent in clear text it is recommended to use HTTPS/TLS
    /// with this method. However use of HTTPS/TLS is not enforced by this middleware.
    ///
    /// See the [auth](crate::auth) for more details.
    #[cfg(feature = "auth")]
    fn require_basic_authorization(self, username: &str, password: &str) -> AddAuthorization<Self>
    where
        Self: Sized,
    {
        AddAuthorization::basic(self, username, password)
    }

    /// Authorize requests using a "bearer token". Commonly used for OAuth 2.
    ///
    /// The `Authorization` header will be set to `Bearer {token}`.
    ///
    /// # Panics
    ///
    /// Panics if the token is not a valid [HeaderValue](http::{HeaderValue).
    ///
    /// See the [auth](crate::auth) for more details.
    #[cfg(feature = "auth")]
    fn require_bearer_authorization(self, token: &str) -> AddAuthorization<Self>
    where
        Self: Sized,
    {
        AddAuthorization::bearer(self, token)
    }

    /// Authorize requests using a custom scheme.
    ///
    /// The `Authorization` header is required to have the value provided.
    ///
    /// See the [auth](crate::auth) for more details.
    #[cfg(feature = "auth")]
    fn async_require_authorization<T>(self, auth: T) -> AsyncRequireAuthorization<Self, T>
    where
        Self: Sized,
    {
        AsyncRequireAuthorization::new(self, auth)
    }

    /// Create a new middleware that catches panics and converts them into
    /// `500 Internal Server` responses with the default panic handler.
    ///
    /// See the [catch_panic](crate::catch_panic) for more details.
    #[cfg(feature = "catch-panic")]
    fn catch_panic(self) -> CatchPanic<Self, DefaultResponseForPanic>
    where
        Self: Sized,
    {
        CatchPanic::new(self)
    }

    /// Create a new middleware that catches panics and converts them into
    /// `500 Internal Server` responses with a custom panic handler.
    ///
    /// See the [catch_panic](crate::catch_panic) for more details.
    #[cfg(feature = "catch-panic")]
    fn catch_panic_custom<T>(self, panic_handler: T) -> CatchPanic<Self, T>
    where
        Self: Sized,
        T: ResponseForPanic,
    {
        CatchPanic::custom(self, panic_handler)
    }

    /// Creates a new  middleware that compress response bodies of the underlying service.
    ///
    /// This uses the `Accept-Encoding` header to pick an appropriate encoding and adds the
    /// `Content-Encoding` header to responses.
    ///
    /// See the [compression](crate::compression) for more details.
    #[cfg(any(
        feature = "compression-br",
        feature = "compression-deflate",
        feature = "compression-gzip",
        feature = "compression-zstd"
    ))]
    fn compress(self) -> Compression<Self, DefaultPredicate>
    where
        Self: Sized,
    {
        Compression::new(self)
    }

    /// Creates a new middleware that compress response bodies of the underlying service using
    /// a custom predicate to determine whether a response should be compressed or not.
    ///
    /// See the [compression](crate::compression) for more details.
    #[cfg(any(
        feature = "compression-br",
        feature = "compression-deflate",
        feature = "compression-gzip",
        feature = "compression-zstd"
    ))]
    fn compress_when<C>(self, predicate: C) -> Compression<Self, C>
    where
        Self: Sized,
        C: Predicate,
    {
        Compression::new(self).compress_when(predicate)
    }

    /// Creates a new middleware that adds headers for [CORS][mdn].
    ///
    /// See the [cors](crate::cors) for an example.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS
    #[cfg(feature = "cors")]
    fn add_cors(self) -> Cors<Self>
    where
        Self: Sized,
    {
        Cors::new(self)
    }

    /// Creates a new middleware that adds headers for [CORS][mdn] using a permissive configuration.
    ///
    /// See the [cors](crate::cors) for an example.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS
    #[cfg(feature = "cors")]
    fn add_cors_permissive(self) -> Cors<Self>
    where
        Self: Sized,
    {
        Cors::permissive(self)
    }

    /// Creates a new middleware that adds headers for [CORS][mdn] using a very permissive configuration.
    ///
    /// See the [cors](crate::cors) for an example.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS
    #[cfg(feature = "cors")]
    fn add_cors_very_permissive(self) -> Cors<Self>
    where
        Self: Sized,
    {
        Cors::very_permissive(self)
    }

    /// Creates a new middleware that decompresses response bodies of the underlying service.
    ///
    /// This adds the `Accept-Encoding` header to requests and transparently decompresses response
    /// bodies based on the `Content-Encoding` header.
    ///
    /// See the [decompression](crate::decompression) for more details.
    #[cfg(any(
        feature = "decompression-br",
        feature = "decompression-deflate",
        feature = "decompression-gzip",
        feature = "decompression-zstd"
    ))]
    fn decompress(self) -> Decompression<Self>
    where
        Self: Sized,
    {
        Decompression::new(self)
    }

    /// Creates a new middleware that decompresses request bodies and calls its underlying service.
    ///
    /// Transparently decompresses request bodies based on the `Content-Encoding` header.
    /// When the encoding in the `Content-Encoding` header is not accepted an `Unsupported Media Type`
    /// status code will be returned with the accepted encodings in the `Accept-Encoding` header.
    ///
    /// Enabling pass-through of unaccepted encodings will not return an `Unsupported Media Type` but
    /// will call the underlying service with the unmodified request if the encoding is not supported.
    /// This is disabled by default.
    ///
    /// See the [decompression](crate::decompression) for more details.
    #[cfg(any(
        feature = "decompression-br",
        feature = "decompression-deflate",
        feature = "decompression-gzip",
        feature = "decompression-zstd"
    ))]
    fn decompress_request(self) -> RequestDecompression<Self>
    where
        Self: Sized,
    {
        RequestDecompression::new(self)
    }

    /// Creates a new middleware that retries requests with a [`Service`](tower::Service) to follow redirection responses.
    ///
    /// See the [follow_redirect](crate::follow_redirect) for more details.
    #[cfg(feature = "follow-redirect")]
    fn follow_redirect(self) -> FollowRedirect<Self, Standard>
    where
        Self: Sized,
    {
        FollowRedirect::new(self)
    }

    /// Creates a new middleware that retries requests with a [`Service`](tower::Service) to follow redirection responses
    /// with the given redirection [`Policy`](crate::follow_redirect::policy::Policy).
    ///
    /// See the [follow_redirect](crate::follow_redirect) for more details.
    #[cfg(feature = "follow-redirect")]
    fn follow_redirect_with_policy<P>(self, policy: P) -> FollowRedirect<Self, P>
    where
        Self: Sized,
        P: Clone,
    {
        FollowRedirect::with_policy(self, policy)
    }

    /// Creates a new middleware that intercepts requests with body lengths greater than the
    /// configured limit and converts them into `413 Payload Too Large` responses.
    ///
    /// See the [limit](crate::limit) for an example.
    #[cfg(feature = "limit")]
    fn limit_request_body(self, limit: usize) -> RequestBodyLimit<Self>
    where
        Self: Sized,
    {
        RequestBodyLimit::new(self, limit)
    }

    /// Creates a new middleware that apply a transformation to the request body.
    #[cfg(feature = "map-request-body")]
    fn map_request_body<F>(self, f: F) -> MapRequestBody<Self, F>
    where
        Self: Sized,
    {
        MapRequestBody::new(self, f)
    }

    /// Creates a new middleware that apply a transformation to the response body.
    #[cfg(feature = "map-response-body")]
    fn map_response_body<F>(self, f: F) -> MapResponseBody<Self, F>
    where
        Self: Sized,
    {
        MapResponseBody::new(self, f)
    }

    /// Creates a new middleware that counts the number of in-flight requests.
    #[cfg(feature = "metrics")]
    fn count_in_flight_requests(self, counter: InFlightRequestsCounter) -> InFlightRequests<Self>
    where
        Self: Sized,
    {
        InFlightRequests::new(self, counter)
    }

    /// Creates a new middleware that normalizes paths.
    ///
    /// Any trailing slashes from request paths will be removed. For example, a request with `/foo/`
    /// will be changed to `/foo` before reaching the inner service.
    ///
    /// See the [normalize_path](crate::normalize_path) for more details.
    #[cfg(feature = "normalize-path")]
    fn normalize_path(self) -> NormalizePath<Self>
    where
        Self: Sized,
    {
        NormalizePath::trim_trailing_slash(self)
    }

    /// Creates a new middleware that propagates headers from requests to responses.
    ///
    /// If the header is present on the request it'll be applied to the response as well. This could
    /// for example be used to propagate headers such as `X-Request-Id`.
    ///
    /// See the [propagate_header](crate::propagate_header) for more details.
    #[cfg(feature = "propagate-header")]
    fn propagate_header(self, header_name: HeaderName) -> PropagateHeader<Self>
    where
        Self: Sized,
    {
        PropagateHeader::new(self, header_name)
    }

    /// Creates a new middleware that propagate request ids from requests to responses.
    ///
    /// If the request contains a matching header that header will be applied to responses. If a
    /// [`RequestId`](crate::request_id::RequestId) extension is also present it will be propagated as well.
    ///
    /// See the [request_id](crate::request_id) for an example.
    #[cfg(feature = "request-id")]
    fn propagate_request_id(self, header_name: HeaderName) -> PropagateRequestId<Self>
    where
        Self: Sized,
    {
        PropagateRequestId::new(self, header_name)
    }

    /// Creates a new middleware that propagate request ids from requests to responses
    /// using `x-request-id` as the header name.
    ///
    /// If the request contains a matching header that header will be applied to responses. If a
    /// [`RequestId`](crate::request_id::RequestId) extension is also present it will be propagated as well.
    ///
    /// See the [request_id](crate::request_id) for an example.
    #[cfg(feature = "request-id")]
    fn propagate_x_request_id(self) -> PropagateRequestId<Self>
    where
        Self: Sized,
    {
        PropagateRequestId::new(self, HeaderName::from_static(X_REQUEST_ID))
    }

    /// Creates a new middleware that set request id headers and extensions on requests.
    ///
    /// If [`MakeRequestId::make_request_id`] returns `Some(_)` and the request doesn't already have a
    /// header with the same name, then the header will be inserted.
    ///
    /// Additionally [`RequestId`](crate::request_id::RequestId) will be inserted into
    /// the Request extensions so other services can access it.
    ///
    /// See the [request_id](crate::request_id) for an example.
    #[cfg(feature = "request-id")]
    fn set_request_id<M>(self, header_name: HeaderName, make_request_id: M) -> SetRequestId<Self, M>
    where
        Self: Sized,
        M: MakeRequestId,
    {
        SetRequestId::new(self, header_name, make_request_id)
    }

    /// Creates a new middleware that set request id headers and extensions on requests
    /// using `x-request-id` as the header name.
    ///
    /// If [`MakeRequestId::make_request_id`] returns `Some(_)` and the request doesn't already have a
    /// header with the same name, then the header will be inserted.
    ///
    /// Additionally [`RequestId`](crate::request_id::RequestId) will be inserted into
    /// the Request extensions so other services can access it.
    ///
    /// See the [request_id](crate::request_id) for an example.
    #[cfg(feature = "request-id")]
    fn set_x_request_id<M>(self, make_request_id: M) -> SetRequestId<Self, M>
    where
        Self: Sized,
        M: MakeRequestId,
    {
        SetRequestId::new(self, HeaderName::from_static(X_REQUEST_ID), make_request_id)
    }

    /// Creates a new middleware that marks headers as [sensitive].
    ///
    /// See the [sensitive_headers](crate::sensitive_headers) for more details.
    ///
    /// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
    #[cfg(feature = "sensitive-headers")]
    fn set_sensitive_headers<I>(self, headers: I) -> SetSensitiveHeaders<Self>
    where
        Self: Sized,
        I: IntoIterator<Item = HeaderName>,
    {
        use std::iter::FromIterator;
        let headers = Vec::from_iter(headers);
        SetSensitiveRequestHeaders::new(
            SetSensitiveResponseHeaders::new(self, headers.iter().cloned()),
            headers.into_iter(),
        )
    }

    /// Creates a new middleware that marks request headers as [sensitive].
    ///
    /// See the [sensitive_headers](crate::sensitive_headers) for more details.
    ///
    /// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
    #[cfg(feature = "sensitive-headers")]
    fn set_sensitive_request_headers<I>(self, headers: I) -> SetSensitiveRequestHeaders<Self>
    where
        Self: Sized,
        I: IntoIterator<Item = HeaderName>,
    {
        SetSensitiveRequestHeaders::new(self, headers)
    }

    /// Creates a new middleware that marks response headers as [sensitive].
    ///
    /// See the [sensitive_headers](crate::sensitive_headers) for more details.
    ///
    /// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
    #[cfg(feature = "sensitive-headers")]
    fn set_sensitive_response_headers<I>(self, headers: I) -> SetSensitiveResponseHeaders<Self>
    where
        Self: Sized,
        I: IntoIterator<Item = HeaderName>,
    {
        SetSensitiveResponseHeaders::new(self, headers)
    }

    /// Creates a new middleware that sets a header on the request.
    ///
    /// If a previous value exists for the same header, it is removed and replaced with the new
    /// header value.
    #[cfg(feature = "set-header")]
    fn override_request_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetRequestHeader<Self, M>
    where
        Self: Sized,
    {
        SetRequestHeader::overriding(self, header_name, make)
    }

    /// Creates a new middleware that sets a header on the request.
    ///
    /// The new header is always added, preserving any existing values. If previous values exist,
    /// the header will have multiple values.
    #[cfg(feature = "set-header")]
    fn append_request_header<M>(self, header_name: HeaderName, make: M) -> SetRequestHeader<Self, M>
    where
        Self: Sized,
    {
        SetRequestHeader::appending(self, header_name, make)
    }

    /// Creates a new middleware that sets a header on the request.
    ///
    /// If a previous value exists for the header, the new value is not inserted.
    #[cfg(feature = "set-header")]
    fn set_request_header_if_not_present<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetRequestHeader<Self, M>
    where
        Self: Sized,
    {
        SetRequestHeader::if_not_present(self, header_name, make)
    }

    /// Creates a new middleware that sets a header on the response.
    ///
    /// If a previous value exists for the same header, it is removed and replaced with the new
    /// header value.
    #[cfg(feature = "set-header")]
    fn override_response_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetResponseHeader<Self, M>
    where
        Self: Sized,
    {
        SetResponseHeader::overriding(self, header_name, make)
    }

    /// Creates a new middleware that sets a header on the response.
    ///
    /// The new header is always added, preserving any existing values. If previous values exist,
    /// the header will have multiple values.
    #[cfg(feature = "set-header")]
    fn append_response_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetResponseHeader<Self, M>
    where
        Self: Sized,
    {
        SetResponseHeader::appending(self, header_name, make)
    }

    /// Creates a new middleware that sets a header on the response.
    ///
    /// If a previous value exists for the header, the new value is not inserted.
    #[cfg(feature = "set-header")]
    fn set_response_header_if_not_present<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetResponseHeader<Self, M>
    where
        Self: Sized,
    {
        SetResponseHeader::if_not_present(self, header_name, make)
    }

    /// Creates a new middleware that override status codes.
    ///
    /// See the [set_status](crate::set_status) for more details.
    #[cfg(feature = "set-status")]
    fn set_status(self, status: StatusCode) -> SetStatus<Self>
    where
        Self: Sized,
    {
        SetStatus::new(self, status)
    }

    /// Creates a new middleware that applies a timeout to requests.
    ///
    /// If the request does not complete within the specified timeout it will be aborted and a `408
    /// Request Timeout` response will be sent.
    ///
    /// See the [timeout](crate::timeout) for an example.
    #[cfg(feature = "timeout")]
    fn timeout(self, timeout: Duration) -> Timeout<Self>
    where
        Self: Sized,
    {
        Timeout::new(self, timeout)
    }

    /// Creates a new middleware that applies a timeout to request bodies.
    ///
    /// See the [timeout](crate::timeout) for an example.
    #[cfg(feature = "timeout")]
    fn timeout_request_body(self, timeout: Duration) -> RequestBodyTimeout<Self>
    where
        Self: Sized,
    {
        RequestBodyTimeout::new(self, timeout)
    }

    /// Creates a new middleware that applies a timeout to response bodies.
    ///
    /// See the [timeout](crate::timeout) for an example.
    #[cfg(feature = "timeout")]
    fn timeout_response_body(self, timeout: Duration) -> ResponseBodyTimeout<Self>
    where
        Self: Sized,
    {
        ResponseBodyTimeout::new(self, timeout)
    }

    /// Creates a new middleware that adds high level [tracing] to a [`Service`]
    /// using the given [`MakeClassifier`].
    ///
    /// See the [trace](crate::trace) for an example.
    ///
    /// [tracing]: https://crates.io/crates/tracing
    /// [`Service`]: tower_service::Service
    #[cfg(feature = "trace")]
    fn trace<M>(
        self,
        make_classifier: M,
    ) -> Trace<
        Self,
        M,
        DefaultMakeSpan,
        DefaultOnRequest,
        DefaultOnResponse,
        DefaultOnBodyChunk,
        DefaultOnEos,
        DefaultOnFailure,
    >
    where
        Self: Sized,
        M: MakeClassifier,
    {
        Trace::new(self, make_classifier)
    }

    /// Creates a new middleware that adds high level [tracing] to a [`Service`]
    /// which supports classifying regular HTTP responses based on the status code.
    ///
    /// See the [trace](crate::trace) for an example.
    ///
    /// [tracing]: https://crates.io/crates/tracing
    /// [`Service`]: tower_service::Service
    #[cfg(feature = "trace")]
    fn trace_http(
        self,
    ) -> Trace<
        Self,
        SharedClassifier<ServerErrorsAsFailures>,
        DefaultMakeSpan,
        DefaultOnRequest,
        DefaultOnResponse,
        DefaultOnBodyChunk,
        DefaultOnEos,
        DefaultOnFailure,
    >
    where
        Self: Sized,
    {
        Trace::new_for_http(self)
    }

    /// Creates a new middleware that adds high level [tracing] to a [`Service`]
    /// which supports classifying gRPC responses and streams based on the `grpc-status` header.
    ///
    /// See the [trace](crate::trace) for an example.
    ///
    /// [tracing]: https://crates.io/crates/tracing
    /// [`Service`]: tower_service::Service
    #[cfg(feature = "trace")]
    fn trace_grpc(
        self,
    ) -> Trace<
        Self,
        SharedClassifier<GrpcErrorsAsFailures>,
        DefaultMakeSpan,
        DefaultOnRequest,
        DefaultOnResponse,
        DefaultOnBodyChunk,
        DefaultOnEos,
        DefaultOnFailure,
    >
    where
        Self: Sized,
    {
        Trace::new_for_grpc(self)
    }

    /// Creates a new middleware that authorize requests using a username and password pair.
    ///
    /// The `Authorization` header is required to be `Basic {credentials}` where `credentials` is
    /// `base64_encode("{username}:{password}")`.
    ///
    /// Since the username and password is sent in clear text it is recommended to use HTTPS/TLS
    /// with this method. However use of HTTPS/TLS is not enforced by this middleware.
    ///
    /// See the [validate_request](crate::validate_request) for an example.
    #[cfg(all(feature = "validate-request", feature = "auth"))]
    fn validate_basic_authorization<Resbody>(
        self,
        username: &str,
        password: &str,
    ) -> ValidateRequestHeader<Self, Basic<Resbody>>
    where
        Self: Sized,
        Resbody: Body + Default,
    {
        ValidateRequestHeader::basic(self, username, password)
    }

    /// Creates a new middleware that authorize requests using a "bearer token".
    /// Commonly used for OAuth 2.
    ///
    /// The `Authorization` header is required to be `Bearer {token}`.
    ///
    /// # Panics
    ///
    /// Panics if the token is not a valid [`HeaderValue`](http::header::HeaderValue).
    ///
    /// See the [validate_request](crate::validate_request) for an example.
    #[cfg(all(feature = "validate-request", feature = "auth"))]
    fn validate_bearer_authorization<Resbody>(
        self,
        token: &str,
    ) -> ValidateRequestHeader<Self, Bearer<Resbody>>
    where
        Self: Sized,
        Resbody: Body + Default,
    {
        ValidateRequestHeader::bearer(self, token)
    }

    /// Creates a new middleware that authorize requests that have the required Accept header.
    ///
    /// The `Accept` header is required to be `*/*`, `type/*` or `type/subtype`,
    /// as configured.
    ///
    /// # Panics
    ///
    /// See `AcceptHeader::new` for when this method panics.
    ///
    /// See the [validate_request](crate::validate_request) for an example.
    #[cfg(feature = "validate-request")]
    fn validate_accept_header<Resbody>(
        self,
        value: &str,
    ) -> ValidateRequestHeader<Self, AcceptHeader<Resbody>>
    where
        Self: Sized,
        Resbody: Body + Default,
    {
        ValidateRequestHeader::accept(self, value)
    }

    /// Creates a new middleware that authorize requests using a custom method.
    ///
    /// See the [validate_request](crate::validate_request) for an example.
    #[cfg(feature = "validate-request")]
    fn validate<T>(self, validate: T) -> ValidateRequestHeader<Self, T>
    where
        Self: Sized,
    {
        ValidateRequestHeader::custom(self, validate)
    }
}

impl<T: ?Sized, Request> ServiceExt<Request> for T where T: tower_service::Service<Request> + Sized {}
