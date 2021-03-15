use super::{ClassifyResponse, MakeClassifier};
use http::{HeaderMap, Method, Request, Uri, Version};
use std::fmt;

/// Create a [`MakeClassifier`] from a closure.
///
/// The closure is given the request method, uri, version, and headers.
///
/// # Example
///
/// This can for example be used to create a [`MakeClassifier`] that supports both gRPC and regular
/// HTTP requests. The classifier to use is picked based on the `Content-Type` header.
///
/// ```rust
/// use http::{Method, Uri, HeaderMap, Request, Response, StatusCode, Version, header};
/// use hyper::Body;
/// use tower::util::Either;
/// use tower_http::classify::{
///     make_classifier_fn, ClassifiedResponse, ClassifyResponse, ServerErrorsAsFailures,
///     MakeClassifier, GrpcErrorsAsFailures
/// };
/// #
/// # // we have to help rust infer the error types. Shouldn't be necessary when using
/// # // classifiers in middlewares since you'll have to specify the trait bound then
/// # fn fix_make_error_type<T, E>(_: &T)
/// # where T: MakeClassifier<E> {}
/// # fn fix_class_error_type<T, E>(_: &T)
/// # where T: ClassifyResponse<E> {}
///
/// // Our `MakeClassifier` that returns either a gRPC classifier or a status code classifier.
/// let make_classifier = make_classifier_fn(
///     |method: &Method, uri: &Uri, version: Version, headers: &HeaderMap| {
///         if is_grpc(headers) {
///             Either::A(GrpcErrorsAsFailures::new())
///         } else {
///             // The classifiers have to have the same output type. So we map the `StatusCode`
///             // into an `i32`.
///             let server_errors = ServerErrorsAsFailures::new()
///                 .map_failure_class(|status| i32::from(status.as_u16()));
///             Either::B(server_errors)
///         }
///     },
/// );
/// # fix_make_error_type::<_, tower::BoxError>(&make_classifier);
///
/// // Identify gRPC requests based on the `Content-Type` header
/// fn is_grpc(headers: &HeaderMap) -> bool {
///     headers
///         .get(header::CONTENT_TYPE)
///         .and_then(|value| value.to_str().ok())
///         .map_or(false, |value| value == "application/grpc")
/// }
///
/// // We can now classify regular HTTP responses based on status code
/// let request = Request::new(Body::empty());
///
/// let classifier = make_classifier.make_classifier(&request);
/// # fix_class_error_type::<_, tower::BoxError>(&classifier);
///
/// let response = Response::builder()
///     .status(StatusCode::INTERNAL_SERVER_ERROR)
///     .header(header::CONTENT_TYPE, "text/html")
///     .body(Body::from("<h1>something went wrong</h1>"))
///     .unwrap();
///
/// let classification = classifier.classify_response(&response);
///
/// assert!(matches!(classification, ClassifiedResponse::Ready(Err(500))));
///
/// // Or gRPC responses based on the `grpc-status` header
/// let request = Request::builder()
///     .header(header::CONTENT_TYPE, "application/grpc")
///     .body(Body::empty())
///     .unwrap();
///
/// let classifier = make_classifier.make_classifier(&request);
/// # fix_class_error_type::<_, tower::BoxError>(&classifier);
///
/// let response = Response::builder()
///     .header(header::CONTENT_TYPE, "application/grpc")
///     .header(header::HeaderName::from_static("grpc-status"), 13)
///     .body(Body::empty())
///     .unwrap();
///
/// let classification = classifier.classify_response(&response);
///
/// assert!(matches!(classification, ClassifiedResponse::Ready(Err(13))));
/// ```
pub fn make_classifier_fn<F, C>(f: F) -> MakeClassifierFn<F>
where
    F: Fn(&Method, &Uri, Version, &HeaderMap) -> C,
{
    MakeClassifierFn { f }
}

/// A [`MakeClassifier`] created from a closure.
///
/// See [`make_classifier_fn`] for more details.
#[derive(Clone, Copy)]
pub struct MakeClassifierFn<F> {
    f: F,
}

impl<E, F, C> MakeClassifier<E> for MakeClassifierFn<F>
where
    F: Fn(&Method, &Uri, Version, &HeaderMap) -> C,
    C: ClassifyResponse<E>,
{
    type Classifier = C;
    type FailureClass = C::FailureClass;
    type ClassifyEos = C::ClassifyEos;

    fn make_classifier<B>(&self, req: &Request<B>) -> Self::Classifier {
        let method = req.method();
        let uri = req.uri();
        let version = req.version();
        let headers = req.headers();
        (self.f)(method, uri, version, headers)
    }
}

impl<F> fmt::Debug for MakeClassifierFn<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MakeClassifierFn")
            .field("f", &format_args!("{}", std::any::type_name::<F>()))
            .finish()
    }
}
