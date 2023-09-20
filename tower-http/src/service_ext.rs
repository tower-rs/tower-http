#![allow(missing_docs)] // todo

#[cfg(feature = "add-extension")]
use crate::add_extension::AddExtension;
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
use crate::{
    auth::require_authorization::{Basic, Bearer},
    validate_request::{AcceptHeader, ValidateRequestHeader},
};
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

pub trait ServiceExt<Request>: tower_service::Service<Request> + Sized {
    #[cfg(feature = "add-extension")]
    fn add_extension<T>(self, value: T) -> AddExtension<Self, T> {
        AddExtension::new(self, value)
    }

    #[cfg(feature = "auth")]
    fn require_basic_authorization(self, username: &str, password: &str) -> AddAuthorization<Self> {
        AddAuthorization::basic(self, username, password)
    }

    #[cfg(feature = "auth")]
    fn require_bearer_authorization(self, token: &str) -> AddAuthorization<Self> {
        AddAuthorization::bearer(self, token)
    }

    #[cfg(feature = "auth")]
    fn async_require_authorization<T>(self, auth: T) -> AsyncRequireAuthorization<Self, T> {
        AsyncRequireAuthorization::new(self, auth)
    }

    #[cfg(feature = "catch-panic")]
    fn catch_panic(self) -> CatchPanic<Self, DefaultResponseForPanic> {
        CatchPanic::new(self)
    }

    #[cfg(feature = "catch-panic")]
    fn catch_panic_custom<T>(self, panic_handler: T) -> CatchPanic<Self, T>
    where
        T: ResponseForPanic,
    {
        CatchPanic::custom(self, panic_handler)
    }

    #[cfg(any(
        feature = "compression-br",
        feature = "compression-deflate",
        feature = "compression-gzip",
        feature = "compression-zstd"
    ))]
    fn compress(self) -> Compression<Self, DefaultPredicate> {
        Compression::new(self)
    }

    #[cfg(any(
        feature = "compression-br",
        feature = "compression-deflate",
        feature = "compression-gzip",
        feature = "compression-zstd"
    ))]
    fn compress_when<C>(self, predicate: C) -> Compression<Self, C>
    where
        C: Predicate,
    {
        Compression::new(self).compress_when(predicate)
    }

    #[cfg(feature = "cors")]
    fn add_cors(self) -> Cors<Self> {
        Cors::new(self)
    }

    #[cfg(feature = "cors")]
    fn add_cors_permissive(self) -> Cors<Self> {
        Cors::permissive(self)
    }

    #[cfg(feature = "cors")]
    fn add_cors_very_permissive(self) -> Cors<Self> {
        Cors::very_permissive(self)
    }

    #[cfg(any(
        feature = "decompression-br",
        feature = "decompression-deflate",
        feature = "decompression-gzip",
        feature = "decompression-zstd"
    ))]
    fn decompress(self) -> Decompression<Self> {
        Decompression::new(self)
    }

    #[cfg(any(
        feature = "decompression-br",
        feature = "decompression-deflate",
        feature = "decompression-gzip",
        feature = "decompression-zstd"
    ))]
    fn decompress_request(self) -> RequestDecompression<Self> {
        RequestDecompression::new(self)
    }

    #[cfg(feature = "follow-redirect")]
    fn follow_redirect(self) -> FollowRedirect<Self, Standard> {
        FollowRedirect::new(self)
    }

    #[cfg(feature = "follow-redirect")]
    fn follow_redirect_with_policy<P>(self, policy: P) -> FollowRedirect<Self, P>
    where
        P: Clone,
    {
        FollowRedirect::with_policy(self, policy)
    }

    #[cfg(feature = "limit")]
    fn limit_request_body(self, limit: usize) -> RequestBodyLimit<Self> {
        RequestBodyLimit::new(self, limit)
    }

    #[cfg(feature = "map-request-body")]
    fn map_request_body<F>(self, f: F) -> MapRequestBody<Self, F> {
        MapRequestBody::new(self, f)
    }

    #[cfg(feature = "map-response-body")]
    fn map_response_body<F>(self, f: F) -> MapResponseBody<Self, F> {
        MapResponseBody::new(self, f)
    }

    #[cfg(feature = "metrics")]
    fn count_in_flight_requests(self, counter: InFlightRequestsCounter) -> InFlightRequests<Self> {
        InFlightRequests::new(self, counter)
    }

    #[cfg(feature = "normalize-path")]
    fn normalize_path(self) -> NormalizePath<Self> {
        NormalizePath::trim_trailing_slash(self)
    }

    #[cfg(feature = "propagate-header")]
    fn propagate_header(self, header_name: HeaderName) -> PropagateHeader<Self> {
        PropagateHeader::new(self, header_name)
    }

    #[cfg(feature = "request-id")]
    fn propagate_request_id(self, header_name: HeaderName) -> PropagateRequestId<Self> {
        PropagateRequestId::new(self, header_name)
    }

    #[cfg(feature = "request-id")]
    fn propagate_x_request_id(self) -> PropagateRequestId<Self> {
        PropagateRequestId::new(self, HeaderName::from_static(X_REQUEST_ID))
    }

    #[cfg(feature = "request-id")]
    fn set_request_id<M>(self, header_name: HeaderName, make_request_id: M) -> SetRequestId<Self, M>
    where
        M: MakeRequestId,
    {
        SetRequestId::new(self, header_name, make_request_id)
    }

    #[cfg(feature = "request-id")]
    fn set_x_request_id<M>(
        self,
        make_request_id: M,
    ) -> SetRequestId<Self, M>
    where
        M: MakeRequestId,
    {
        SetRequestId::new(self, HeaderName::from_static(X_REQUEST_ID), make_request_id)
    }

    #[cfg(feature = "sensitive-headers")]
    fn set_sensitive_headers<I>(self, headers: I) -> SetSensitiveHeaders<Self>
    where
        I: IntoIterator<Item = HeaderName>,
    {
        use std::iter::FromIterator;
        let headers = Vec::from_iter(headers);
        SetSensitiveRequestHeaders::new(
            SetSensitiveResponseHeaders::new(self, headers.iter().cloned()),
            headers.into_iter(),
        )
    }

    #[cfg(feature = "sensitive-headers")]
    fn set_sensitive_request_headers<I>(self, headers: I) -> SetSensitiveRequestHeaders<Self>
    where
        I: IntoIterator<Item = HeaderName>,
    {
        SetSensitiveRequestHeaders::new(self, headers)
    }

    #[cfg(feature = "sensitive-headers")]
    fn set_sensitive_response_headers<I>(self, headers: I) -> SetSensitiveResponseHeaders<Self>
    where
        I: IntoIterator<Item = HeaderName>,
    {
        SetSensitiveResponseHeaders::new(self, headers)
    }

    #[cfg(feature = "set-header")]
    fn override_request_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetRequestHeader<Self, M> {
        SetRequestHeader::overriding(self, header_name, make)
    }

    #[cfg(feature = "set-header")]
    fn append_request_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetRequestHeader<Self, M> {
        SetRequestHeader::appending(self, header_name, make)
    }

    #[cfg(feature = "set-header")]
    fn set_request_header_if_not_present<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetRequestHeader<Self, M> {
        SetRequestHeader::if_not_present(self, header_name, make)
    }

    #[cfg(feature = "set-header")]
    fn override_response_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetResponseHeader<Self, M> {
        SetResponseHeader::overriding(self, header_name, make)
    }

    #[cfg(feature = "set-header")]
    fn append_response_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetResponseHeader<Self, M> {
        SetResponseHeader::appending(self, header_name, make)
    }

    #[cfg(feature = "set-header")]
    fn set_response_header_if_not_present<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> SetResponseHeader<Self, M> {
        SetResponseHeader::if_not_present(self, header_name, make)
    }

    #[cfg(feature = "set-status")]
    fn set_status(self, status: StatusCode) -> SetStatus<Self> {
        SetStatus::new(self, status)
    }

    fn timeout(self, timeout: Duration) -> Timeout<Self> {
        Timeout::new(self, timeout)
    }

    fn timeout_request_body(self, timeout: Duration) -> RequestBodyTimeout<Self> {
        RequestBodyTimeout::new(self, timeout)
    }

    fn timeout_response_body(self, timeout: Duration) -> ResponseBodyTimeout<Self> {
        ResponseBodyTimeout::new(self, timeout)
    }

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
        M: MakeClassifier,
    {
        Trace::new(self, make_classifier)
    }

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
    > {
        Trace::new_for_http(self)
    }

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
    > {
        Trace::new_for_grpc(self)
    }

    #[cfg(feature = "validate-request")]
    fn validate_basic_authorization<Resbody>(
        self,
        username: &str,
        password: &str,
    ) -> ValidateRequestHeader<Self, Basic<Resbody>>
    where
        Resbody: Body + Default,
    {
        ValidateRequestHeader::basic(self, username, password)
    }

    #[cfg(feature = "validate-request")]
    fn validate_bearer_authorization<Resbody>(
        self,
        token: &str,
    ) -> ValidateRequestHeader<Self, Bearer<Resbody>>
    where
        Resbody: Body + Default,
    {
        ValidateRequestHeader::bearer(self, token)
    }

    #[cfg(feature = "validate-request")]
    fn validate_accept_header<Resbody>(
        self,
        value: &str,
    ) -> ValidateRequestHeader<Self, AcceptHeader<Resbody>>
    where
        Resbody: Body + Default,
    {
        ValidateRequestHeader::accept(self, value)
    }

    #[cfg(feature = "validate-request")]
    fn validate<T>(self, validate: T) -> ValidateRequestHeader<Self, T> {
        ValidateRequestHeader::custom(self, validate)
    }
}

impl<T: ?Sized, Request> ServiceExt<Request> for T where T: tower_service::Service<Request> + Sized {}
