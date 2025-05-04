#[allow(unused_imports)]
use http::header::HeaderName;

mod sealed {
    #[allow(unreachable_pub, unused)]
    pub trait Sealed<R> {}
}

/// Extension trait that adds methods to any [`Service`] for adding middleware from
/// tower-http.
///
/// [`Service`]: tower::Service
#[cfg(feature = "util")]
// ^ work around rustdoc not inferring doc(cfg)s for cfg's from surrounding scopes
pub trait ServiceExt<R>: sealed::Sealed<R> + tower::Service<R> + Sized {
    /// Propagate a header from the request to the response.
    ///
    /// See [`tower_http::propagate_header`] for more details.
    ///
    /// [`tower_http::propagate_header`]: crate::propagate_header
    #[cfg(feature = "propagate-header")]
    fn propagate_header(
        self,
        header: HeaderName,
    ) -> crate::propagate_header::PropagateHeader<Self> {
        crate::propagate_header::PropagateHeader::new(self, header)
    }

    /// Add some shareable value to [request extensions].
    ///
    /// See [`tower_http::add_extension`] for more details.
    ///
    /// [`tower_http::add_extension`]: crate::add_extension
    /// [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
    #[cfg(feature = "add-extension")]
    fn add_extension<T>(self, value: T) -> crate::add_extension::AddExtension<Self, T> {
        crate::add_extension::AddExtension::new(self, value)
    }

    /// Apply a transformation to the request body.
    ///
    /// See [`tower_http::map_request_body`] for more details.
    ///
    /// [`tower_http::map_request_body`]: crate::map_request_body
    #[cfg(feature = "map-request-body")]
    fn map_request_body<F>(self, f: F) -> crate::map_request_body::MapRequestBody<Self, F> {
        crate::map_request_body::MapRequestBody::new(self, f)
    }

    /// Apply a transformation to the response body.
    ///
    /// See [`tower_http::map_response_body`] for more details.
    ///
    /// [`tower_http::map_response_body`]: crate::map_response_body
    #[cfg(feature = "map-response-body")]
    fn map_response_body<F>(self, f: F) -> crate::map_response_body::MapResponseBody<Self, F> {
        crate::map_response_body::MapResponseBody::new(self, f)
    }

    /// Compresses response bodies.
    ///
    /// See [`tower_http::compression`] for more details.
    ///
    /// [`tower_http::compression`]: crate::compression
    #[cfg(any(
        feature = "compression-br",
        feature = "compression-deflate",
        feature = "compression-gzip",
        feature = "compression-zstd",
    ))]
    fn compression(self) -> crate::compression::Compression<Self> {
        crate::compression::Compression::new(self)
    }

    /// Decompress response bodies.
    ///
    /// See [`tower_http::decompression`] for more details.
    ///
    /// [`tower_http::decompression`]: crate::decompression
    #[cfg(any(
        feature = "decompression-br",
        feature = "decompression-deflate",
        feature = "decompression-gzip",
        feature = "decompression-zstd",
    ))]
    fn decompression(self) -> crate::decompression::Decompression<Self> {
        crate::decompression::Decompression::new(self)
    }

    /// High level tracing that classifies responses using HTTP status codes.
    ///
    /// This method does not support customizing the output, to do that use [`TraceLayer`]
    /// instead.
    ///
    /// See [`tower_http::trace`] for more details.
    ///
    /// [`tower_http::trace`]: crate::trace
    /// [`TraceLayer`]: crate::trace::TraceLayer
    #[cfg(feature = "trace")]
    fn trace_for_http(self) -> crate::trace::Trace<Self, crate::trace::HttpMakeClassifier> {
        crate::trace::Trace::new_for_http(self)
    }

    /// High level tracing that classifies responses using gRPC headers.
    ///
    /// This method does not support customizing the output, to do that use [`TraceLayer`]
    /// instead.
    ///
    /// See [`tower_http::trace`] for more details.
    ///
    /// [`tower_http::trace`]: crate::trace
    /// [`TraceLayer`]: crate::trace::TraceLayer
    #[cfg(feature = "trace")]
    fn trace_for_grpc(self) -> crate::trace::Trace<Self, crate::trace::GrpcMakeClassifier> {
        crate::trace::Trace::new_for_grpc(self)
    }

    /// Follow redirect resposes using the [`Standard`] policy.
    ///
    /// See [`tower_http::follow_redirect`] for more details.
    ///
    /// [`tower_http::follow_redirect`]: crate::follow_redirect
    /// [`Standard`]: crate::follow_redirect::policy::Standard
    #[cfg(feature = "follow-redirect")]
    fn follow_redirects(
        self,
    ) -> crate::follow_redirect::FollowRedirect<Self, crate::follow_redirect::policy::Standard>
    {
        crate::follow_redirect::FollowRedirect::new(self)
    }

    /// Mark headers as [sensitive] on both requests and responses.
    ///
    /// See [`tower_http::sensitive_headers`] for more details.
    ///
    /// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
    /// [`tower_http::sensitive_headers`]: crate::sensitive_headers
    #[cfg(feature = "sensitive-headers")]
    fn sensitive_headers(
        self,
        headers: impl IntoIterator<Item = HeaderName>,
    ) -> crate::sensitive_headers::SetSensitiveHeaders<Self> {
        use tower_layer::Layer as _;
        crate::sensitive_headers::SetSensitiveHeadersLayer::new(headers).layer(self)
    }

    /// Mark headers as [sensitive] on requests.
    ///
    /// See [`tower_http::sensitive_headers`] for more details.
    ///
    /// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
    /// [`tower_http::sensitive_headers`]: crate::sensitive_headers
    #[cfg(feature = "sensitive-headers")]
    fn sensitive_request_headers(
        self,
        headers: impl IntoIterator<Item = HeaderName>,
    ) -> crate::sensitive_headers::SetSensitiveRequestHeaders<Self> {
        crate::sensitive_headers::SetSensitiveRequestHeaders::new(self, headers)
    }

    /// Mark headers as [sensitive] on responses.
    ///
    /// See [`tower_http::sensitive_headers`] for more details.
    ///
    /// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
    /// [`tower_http::sensitive_headers`]: crate::sensitive_headers
    #[cfg(feature = "sensitive-headers")]
    fn sensitive_response_headers(
        self,
        headers: impl IntoIterator<Item = HeaderName>,
    ) -> crate::sensitive_headers::SetSensitiveResponseHeaders<Self> {
        crate::sensitive_headers::SetSensitiveResponseHeaders::new(self, headers)
    }

    /// Insert a header into the request.
    ///
    /// If a previous value exists for the same header, it is removed and replaced with the new
    /// header value.
    ///
    /// See [`tower_http::set_header`] for more details.
    ///
    /// [`tower_http::set_header`]: crate::set_header
    #[cfg(feature = "set-header")]
    fn override_request_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> crate::set_header::SetRequestHeader<Self, M> {
        crate::set_header::SetRequestHeader::overriding(self, header_name, make)
    }

    /// Append a header into the request.
    ///
    /// If previous values exist, the header will have multiple values.
    ///
    /// See [`tower_http::set_header`] for more details.
    ///
    /// [`tower_http::set_header`]: crate::set_header
    #[cfg(feature = "set-header")]
    fn append_request_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> crate::set_header::SetRequestHeader<Self, M> {
        crate::set_header::SetRequestHeader::appending(self, header_name, make)
    }

    /// Insert a header into the request, if the header is not already present.
    ///
    /// See [`tower_http::set_header`] for more details.
    ///
    /// [`tower_http::set_header`]: crate::set_header
    #[cfg(feature = "set-header")]
    fn insert_request_header_if_not_present<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> crate::set_header::SetRequestHeader<Self, M> {
        crate::set_header::SetRequestHeader::if_not_present(self, header_name, make)
    }

    /// Insert a header into the response.
    ///
    /// If a previous value exists for the same header, it is removed and replaced with the new
    /// header value.
    ///
    /// See [`tower_http::set_header`] for more details.
    ///
    /// [`tower_http::set_header`]: crate::set_header
    #[cfg(feature = "set-header")]
    fn override_response_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> crate::set_header::SetResponseHeader<Self, M> {
        crate::set_header::SetResponseHeader::overriding(self, header_name, make)
    }

    /// Append a header into the response.
    ///
    /// If previous values exist, the header will have multiple values.
    ///
    /// See [`tower_http::set_header`] for more details.
    ///
    /// [`tower_http::set_header`]: crate::set_header
    #[cfg(feature = "set-header")]
    fn append_response_header<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> crate::set_header::SetResponseHeader<Self, M> {
        crate::set_header::SetResponseHeader::appending(self, header_name, make)
    }

    /// Insert a header into the response, if the header is not already present.
    ///
    /// See [`tower_http::set_header`] for more details.
    ///
    /// [`tower_http::set_header`]: crate::set_header
    #[cfg(feature = "set-header")]
    fn insert_response_header_if_not_present<M>(
        self,
        header_name: HeaderName,
        make: M,
    ) -> crate::set_header::SetResponseHeader<Self, M> {
        crate::set_header::SetResponseHeader::if_not_present(self, header_name, make)
    }

    /// Add request id header and extension.
    ///
    /// See [`tower_http::request_id`] for more details.
    ///
    /// [`tower_http::request_id`]: crate::request_id
    #[cfg(feature = "request-id")]
    fn set_request_id<M>(
        self,
        header_name: HeaderName,
        make_request_id: M,
    ) -> crate::request_id::SetRequestId<Self, M>
    where
        M: crate::request_id::MakeRequestId,
    {
        crate::request_id::SetRequestId::new(self, header_name, make_request_id)
    }

    /// Add request id header and extension, using `x-request-id` as the header name.
    ///
    /// See [`tower_http::request_id`] for more details.
    ///
    /// [`tower_http::request_id`]: crate::request_id
    #[cfg(feature = "request-id")]
    fn set_x_request_id<M>(self, make_request_id: M) -> crate::request_id::SetRequestId<Self, M>
    where
        M: crate::request_id::MakeRequestId,
    {
        self.set_request_id(crate::request_id::X_REQUEST_ID, make_request_id)
    }

    /// Propgate request ids from requests to responses.
    ///
    /// See [`tower_http::request_id`] for more details.
    ///
    /// [`tower_http::request_id`]: crate::request_id
    #[cfg(feature = "request-id")]
    fn propagate_request_id(
        self,
        header_name: HeaderName,
    ) -> crate::request_id::PropagateRequestId<Self> {
        crate::request_id::PropagateRequestId::new(self, header_name)
    }

    /// Propgate request ids from requests to responses, using `x-request-id` as the header name.
    ///
    /// See [`tower_http::request_id`] for more details.
    ///
    /// [`tower_http::request_id`]: crate::request_id
    #[cfg(feature = "request-id")]
    fn propagate_x_request_id(self) -> crate::request_id::PropagateRequestId<Self> {
        self.propagate_request_id(crate::request_id::X_REQUEST_ID)
    }

    /// Catch panics and convert them into `500 Internal Server` responses.
    ///
    /// See [`tower_http::catch_panic`] for more details.
    ///
    /// [`tower_http::catch_panic`]: crate::catch_panic
    #[cfg(feature = "catch-panic")]
    fn catch_panic(
        self,
    ) -> crate::catch_panic::CatchPanic<Self, crate::catch_panic::DefaultResponseForPanic> {
        crate::catch_panic::CatchPanic::new(self)
    }

    /// Intercept requests with over-sized payloads and convert them into
    /// `413 Payload Too Large` responses.
    ///
    /// See [`tower_http::limit`] for more details.
    ///
    /// [`tower_http::limit`]: crate::limit
    #[cfg(feature = "limit")]
    fn request_body_limit(self, limit: usize) -> crate::limit::RequestBodyLimit<Self> {
        crate::limit::RequestBodyLimit::new(self, limit)
    }

    /// Remove trailing slashes from paths.
    ///
    /// See [`tower_http::normalize_path`] for more details.
    ///
    /// [`tower_http::normalize_path`]: crate::normalize_path
    #[cfg(feature = "normalize-path")]
    fn trim_trailing_slash(self) -> crate::normalize_path::NormalizePath<Self> {
        crate::normalize_path::NormalizePath::trim_trailing_slash(self)
    }
}

impl<R, T> sealed::Sealed<R> for T where T: tower::Service<R> {}
impl<R, T> ServiceExt<R> for T where T: tower::Service<R> {}
