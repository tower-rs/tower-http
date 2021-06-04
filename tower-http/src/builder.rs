use crate::classify::{GrpcErrorsAsFailures, ServerErrorsAsFailures, SharedClassifier};
use http::header::HeaderName;
use tower::ServiceBuilder;
use tower_layer::Stack;

/// Extension trait that adds methods to [`tower::ServiceBuilder`] for adding middleware from
/// tower-http.
///
/// [`Service`]: tower::Service
///
/// # Example
///
/// ```rust
/// use http::{Request, Response, header::HeaderName};
/// use hyper::Body;
/// use std::{time::Duration, convert::Infallible};
/// use tower::{ServiceBuilder, ServiceExt, Service};
/// use tower_http::ServiceBuilderExt;
///
/// async fn handle(request: Request<Body>) -> Result<Response<Body>, Infallible> {
///     Ok(Response::new(Body::empty()))
/// }
///
/// # #[tokio::main]
/// # async fn main() {
/// let service = ServiceBuilder::new()
///     // Methods from tower
///     .timeout(Duration::from_secs(30))
///     // Methods from tower-http
///     .trace_for_http()
///     .compression()
///     .propagate_header(HeaderName::from_static("x-request-id"))
///     .service_fn(handle);
/// # let mut service = service;
/// # service.ready().await.unwrap().call(Request::new(Body::empty())).await.unwrap();
/// # }
/// ```
pub trait ServiceBuilderExt<L>: crate::sealed::Sealed<L> {
    /// Propagate a header from the request to the response.
    ///
    /// See [`tower_http::propagate_header`] for more details.
    ///
    /// [`tower_http::propagate_header`]: crate::propagate_header
    #[cfg(feature = "propagate-header")]
    #[cfg_attr(docsrs, doc(cfg(feature = "propagate-header")))]
    fn propagate_header(
        self,
        header: HeaderName,
    ) -> ServiceBuilder<Stack<crate::propagate_header::PropagateHeaderLayer, L>>;

    /// Add some shareable value to [request extensions].
    ///
    /// See [`tower_http::add_extension`] for more details.
    ///
    /// [`tower_http::add_extension`]: crate::add_extension
    /// [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
    #[cfg(feature = "add-extension")]
    #[cfg_attr(docsrs, doc(cfg(feature = "add-extension")))]
    fn add_extension<T>(
        self,
        value: T,
    ) -> ServiceBuilder<Stack<crate::add_extension::AddExtensionLayer<T>, L>>;

    /// Apply a transformation to the request body.
    ///
    /// See [`tower_http::map_request_body`] for more details.
    ///
    /// [`tower_http::map_request_body`]: crate::map_request_body
    #[cfg(feature = "map-request-body")]
    #[cfg_attr(docsrs, doc(cfg(feature = "map-request-body")))]
    fn map_request_body<F>(
        self,
        f: F,
    ) -> ServiceBuilder<Stack<crate::map_request_body::MapRequestBodyLayer<F>, L>>;

    /// Apply a transformation to the response body.
    ///
    /// See [`tower_http::map_response_body`] for more details.
    ///
    /// [`tower_http::map_response_body`]: crate::map_response_body
    #[cfg(feature = "map-response-body")]
    #[cfg_attr(docsrs, doc(cfg(feature = "map-response-body")))]
    fn map_response_body<F>(
        self,
        f: F,
    ) -> ServiceBuilder<Stack<crate::map_response_body::MapResponseBodyLayer<F>, L>>;

    /// Compresses response bodies.
    ///
    /// See [`tower_http::compression`] for more details.
    ///
    /// [`tower_http::compression`]: crate::compression
    #[cfg(feature = "compression")]
    #[cfg_attr(docsrs, doc(cfg(feature = "compression")))]
    fn compression(self) -> ServiceBuilder<Stack<crate::compression::CompressionLayer, L>>;

    /// Decompress response bodies.
    ///
    /// See [`tower_http::decompression`] for more details.
    ///
    /// [`tower_http::decompression`]: crate::decompression
    #[cfg(feature = "decompression")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression")))]
    fn decompression(self) -> ServiceBuilder<Stack<crate::decompression::DecompressionLayer, L>>;

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
    #[cfg_attr(docsrs, doc(cfg(feature = "trace")))]
    fn trace_for_http(
        self,
    ) -> ServiceBuilder<Stack<crate::trace::TraceLayer<SharedClassifier<ServerErrorsAsFailures>>, L>>;

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
    #[cfg_attr(docsrs, doc(cfg(feature = "trace")))]
    fn trace_for_grpc(
        self,
    ) -> ServiceBuilder<Stack<crate::trace::TraceLayer<SharedClassifier<GrpcErrorsAsFailures>>, L>>;
}

impl<L> crate::sealed::Sealed<L> for ServiceBuilder<L> {}

impl<L> ServiceBuilderExt<L> for ServiceBuilder<L> {
    #[cfg(feature = "propagate-header")]
    fn propagate_header(
        self,
        header: HeaderName,
    ) -> ServiceBuilder<Stack<crate::propagate_header::PropagateHeaderLayer, L>> {
        self.layer(crate::propagate_header::PropagateHeaderLayer::new(header))
    }

    #[cfg(feature = "add-extension")]
    fn add_extension<T>(
        self,
        value: T,
    ) -> ServiceBuilder<Stack<crate::add_extension::AddExtensionLayer<T>, L>> {
        self.layer(crate::add_extension::AddExtensionLayer::new(value))
    }

    #[cfg(feature = "map-request-body")]
    fn map_request_body<F>(
        self,
        f: F,
    ) -> ServiceBuilder<Stack<crate::map_request_body::MapRequestBodyLayer<F>, L>> {
        self.layer(crate::map_request_body::MapRequestBodyLayer::new(f))
    }

    #[cfg(feature = "map-response-body")]
    fn map_response_body<F>(
        self,
        f: F,
    ) -> ServiceBuilder<Stack<crate::map_response_body::MapResponseBodyLayer<F>, L>> {
        self.layer(crate::map_response_body::MapResponseBodyLayer::new(f))
    }

    #[cfg(feature = "compression")]
    fn compression(self) -> ServiceBuilder<Stack<crate::compression::CompressionLayer, L>> {
        self.layer(crate::compression::CompressionLayer::new())
    }

    #[cfg(feature = "decompression")]
    fn decompression(self) -> ServiceBuilder<Stack<crate::decompression::DecompressionLayer, L>> {
        self.layer(crate::decompression::DecompressionLayer::new())
    }

    #[cfg(feature = "trace")]
    fn trace_for_http(
        self,
    ) -> ServiceBuilder<Stack<crate::trace::TraceLayer<SharedClassifier<ServerErrorsAsFailures>>, L>>
    {
        self.layer(crate::trace::TraceLayer::new_for_http())
    }

    #[cfg(feature = "trace")]
    fn trace_for_grpc(
        self,
    ) -> ServiceBuilder<Stack<crate::trace::TraceLayer<SharedClassifier<GrpcErrorsAsFailures>>, L>>
    {
        self.layer(crate::trace::TraceLayer::new_for_grpc())
    }
}
