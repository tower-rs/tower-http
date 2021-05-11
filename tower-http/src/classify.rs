//! Tools for classifying responses as either success or failure.

use http::{HeaderMap, Request, Response, StatusCode};
use std::{convert::Infallible, fmt, marker::PhantomData, num::NonZeroI32};

/// Trait for producing response classifiers from a request.
///
/// This is useful when a classifier depends on data from the request. For example, this could
/// include the URI or HTTP method.
///
/// This trait is generic over the [`Error` type] of the `Service`s used with the classifier.
/// This is necessary for [`ClassifyResponse::classify_error`].
///
/// [`Error` type]: https://docs.rs/tower/latest/tower/trait.Service.html#associatedtype.Error
pub trait MakeClassifier {
    /// The response classifier produced.
    type Classifier: ClassifyResponse<
        FailureClass = Self::FailureClass,
        ClassifyEos = Self::ClassifyEos,
    >;

    /// The type of failure classifications.
    ///
    /// This might include additional information about the error, such as
    /// whether it was a client or server error, or whether or not it should
    /// be considered retryable.
    type FailureClass;

    /// The type used to classify the response end of stream (EOS).
    type ClassifyEos: ClassifyEos<FailureClass = Self::FailureClass>;

    /// Returns a response classifier for this request
    fn make_classifier<B>(&self, req: &Request<B>) -> Self::Classifier;
}

/// A [`MakeClassifier`] that produces new classifiers by cloning an inner classifier.
///
/// When a type implementing [`ClassifyResponse`] doesn't depend on information
/// from the request, [`SharedClassifier`] can be used to turn an instance of that type
/// into a [`MakeClassifier`].
#[derive(Debug, Clone)]
pub struct SharedClassifier<C> {
    classifier: C,
}

impl<C> SharedClassifier<C> {
    /// Create a new `SharedClassifier` from the given classifier.
    pub fn new(classifier: C) -> Self
    where
        C: ClassifyResponse + Clone,
    {
        Self { classifier }
    }
}

impl<C> MakeClassifier for SharedClassifier<C>
where
    C: ClassifyResponse + Clone,
{
    type FailureClass = C::FailureClass;
    type ClassifyEos = C::ClassifyEos;
    type Classifier = C;

    fn make_classifier<B>(&self, _req: &Request<B>) -> Self::Classifier {
        self.classifier.clone()
    }
}

/// Trait for classifying responses as either success or failure. Designed to support both unary
/// requests (single request for a single response) as well as streaming responses.
///
/// Response classifiers are used in cases where middleware needs to determine
/// whether a response completed successfully or failed. For example, they may
/// be used by logging or metrics middleware to record failures differently
/// from successes.
///
/// Furthermore, when a response fails, a response classifier may provide
/// additional information about the failure. This can, for example, be used to
/// build [retry policies] by indicating whether or not a particular failure is
/// retryable.
///
/// [retry policies]: https://docs.rs/tower/latest/tower/retry/trait.Policy.html
pub trait ClassifyResponse {
    /// The type returned when a response is classified as a failure.
    ///
    /// Depending on the classifier, this may simply indicate that the
    /// request failed, or it may contain additional  information about
    /// the failure, such as whether or not it is retryable.
    type FailureClass;

    /// The type used to classify the response end of stream (EOS).
    type ClassifyEos: ClassifyEos<FailureClass = Self::FailureClass>;

    /// Attempt to classify the beginning of a response.
    ///
    /// In some cases, the response can be classified immediately, without
    /// waiting for a body to complete. This may include:
    ///
    /// - When the response has an error status code.
    /// - When a successful response does not have a streaming body.
    /// - When the classifier does not care about streaming bodies.
    ///
    /// When the response can be classified immediately, `classify_response`
    /// returns a [`ClassifiedResponse::Ready`] which indicates whether the
    /// response succeeded or failed.
    ///
    /// In other cases, however, the classifier may need to wait until the
    /// response body stream completes before it can classify the response.
    /// For example, gRPC indicates RPC failures using the `grpc-status`
    /// trailer. In this case, `classify_response` returns a
    /// [`ClassifiedResponse::RequiresEos`] containing a type which will
    /// be used to classify the response when the body stream ends.
    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos>;

    /// Classify an error.
    ///
    /// Errors are always errors (doh) but sometimes it might be useful to have multiple classes of
    /// errors. A retry policy might allow retrying some errors and not others.
    fn classify_error<E>(self, error: &E) -> Self::FailureClass
    where
        E: fmt::Display + 'static;
}

/// Trait for classifying end of streams (EOS) as either success or failure.
pub trait ClassifyEos {
    /// The type of failure classifications.
    type FailureClass;

    /// Perform the classification from response trailers.
    fn classify_eos(self, trailers: Option<&HeaderMap>) -> Result<(), Self::FailureClass>;

    /// Classify an error.
    ///
    /// Errors are always errors (doh) but sometimes it might be useful to have multiple classes of
    /// errors. A retry policy might allow retrying some errors and not others.
    fn classify_error<E>(self, error: &E) -> Self::FailureClass
    where
        E: fmt::Display + 'static;
}

/// Result of doing a classification.
#[derive(Debug)]
pub enum ClassifiedResponse<FailureClass, ClassifyEos> {
    /// The response was able to be classified immediately.
    Ready(Result<(), FailureClass>),
    /// We have to wait until the end of a streaming response to classify it.
    RequiresEos(ClassifyEos),
}

/// A [`ClassifyEos`] type that can be used in [`ClassifyResponse`] implementations that never have
/// to classify streaming responses.
///
/// `NeverClassifyEos` exists only as type.  `NeverClassifyEos` values cannot be constructed.
pub struct NeverClassifyEos<T> {
    _output_ty: PhantomData<fn() -> T>,
    _never: Infallible,
}

impl<T> ClassifyEos for NeverClassifyEos<T> {
    type FailureClass = T;

    fn classify_eos(self, _trailers: Option<&HeaderMap>) -> Result<(), Self::FailureClass> {
        // `NeverClassifyEos` contains an `Infallible` so it can never be constructed
        unreachable!()
    }

    fn classify_error<E>(self, _error: &E) -> Self::FailureClass
    where
        E: fmt::Display + 'static,
    {
        // `NeverClassifyEos` contains an `Infallible` so it can never be constructed
        unreachable!()
    }
}

/// The default classifier used for normal HTTP responses.
///
/// Responses with a `5xx` status code are considered failures, all others are considered
/// successes.
#[derive(Clone, Debug, Default)]
pub struct ServerErrorsAsFailures {
    _priv: (),
}

impl ServerErrorsAsFailures {
    /// Create a new [`ServerErrorsAsFailures`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a [`MakeClassifier`] that produces `ServerErrorsAsFailures`.
    ///
    /// This is a convenience function that simply calls `SharedClassifier::new`.
    pub fn make_classifier() -> SharedClassifier<Self> {
        SharedClassifier::new(Self::new())
    }
}

impl ClassifyResponse for ServerErrorsAsFailures {
    type FailureClass = ServerErrorsFailureClass;
    type ClassifyEos = NeverClassifyEos<ServerErrorsFailureClass>;

    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos> {
        if res.status().is_server_error() {
            ClassifiedResponse::Ready(Err(ServerErrorsFailureClass::StatusCode(res.status())))
        } else {
            ClassifiedResponse::Ready(Ok(()))
        }
    }

    fn classify_error<E>(self, error: &E) -> Self::FailureClass
    where
        E: fmt::Display + 'static,
    {
        ServerErrorsFailureClass::Error(error.to_string())
    }
}

/// The failure class for [`GrpcErrorsAsFailures`].
#[derive(Debug)]
pub enum ServerErrorsFailureClass {
    /// A response was classified as a failure with the corresponding status.
    StatusCode(StatusCode),
    /// A response was classified as an error with the corresponding error description.
    Error(String),
}

impl fmt::Display for ServerErrorsFailureClass {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::StatusCode(code) => write!(f, "Status code: {}", code),
            Self::Error(error) => write!(f, "Error: {}", error),
        }
    }
}

/// Response classifier for gRPC responses.
///
/// gRPC doesn't use normal HTTP statuses for indicating success or failure but instead a special
/// header that might appear in a trailer.
///
/// Responses are considered successful if
///
/// - `grpc-status` header value is 0.
/// - `grpc-status` header is missing.
/// - `grpc-status` header value isn't a valid `String`.
/// - `grpc-status` header value can't parsed into an `i32`.
///
/// All others are considered failures.
#[derive(Debug, Clone, Default)]
pub struct GrpcErrorsAsFailures {
    _priv: (),
}

impl GrpcErrorsAsFailures {
    /// Create a new [`GrpcErrorsAsFailures`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a [`MakeClassifier`] that produces `GrpcErrorsAsFailures`.
    ///
    /// This is a convenience function that simply calls `SharedClassifier::new`.
    pub fn make_classifier() -> SharedClassifier<Self> {
        SharedClassifier::new(Self::new())
    }
}

impl ClassifyResponse for GrpcErrorsAsFailures {
    type FailureClass = GrpcFailureClass;
    type ClassifyEos = GrpcEosErrorsAsFailures;

    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos> {
        match classify_grpc_metadata(res.headers()) {
            ParsedGrpcStatus::Success
            | ParsedGrpcStatus::HeaderNotString
            | ParsedGrpcStatus::HeaderNotInt => ClassifiedResponse::Ready(Ok(())),
            ParsedGrpcStatus::NonSuccess(status) => {
                ClassifiedResponse::Ready(Err(GrpcFailureClass::Code(status)))
            }
            ParsedGrpcStatus::GrpcStatusHeaderMissing => {
                ClassifiedResponse::RequiresEos(GrpcEosErrorsAsFailures { _priv: () })
            }
        }
    }

    fn classify_error<E>(self, error: &E) -> Self::FailureClass
    where
        E: fmt::Display + 'static,
    {
        GrpcFailureClass::Error(error.to_string())
    }
}

/// The [`ClassifyEos`] for [`GrpcErrorsAsFailures`].
#[derive(Debug, Clone)]
pub struct GrpcEosErrorsAsFailures {
    _priv: (),
}

impl ClassifyEos for GrpcEosErrorsAsFailures {
    type FailureClass = GrpcFailureClass;

    fn classify_eos(self, trailers: Option<&HeaderMap>) -> Result<(), Self::FailureClass> {
        if let Some(trailers) = trailers {
            match classify_grpc_metadata(trailers) {
                ParsedGrpcStatus::Success
                | ParsedGrpcStatus::GrpcStatusHeaderMissing
                | ParsedGrpcStatus::HeaderNotString
                | ParsedGrpcStatus::HeaderNotInt => Ok(()),
                ParsedGrpcStatus::NonSuccess(status) => Err(GrpcFailureClass::Code(status)),
            }
        } else {
            Ok(())
        }
    }

    fn classify_error<E>(self, error: &E) -> Self::FailureClass
    where
        E: fmt::Display + 'static,
    {
        GrpcFailureClass::Error(error.to_string())
    }
}

/// The failure class for [`GrpcErrorsAsFailures`].
#[derive(Debug)]
pub enum GrpcFailureClass {
    /// A gRPC response was classified as a failure with the corresponding status.
    Code(std::num::NonZeroI32),
    /// A gRPC response was classified as an error with the corresponding error description.
    Error(String),
}

impl fmt::Display for GrpcFailureClass {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Code(code) => write!(f, "Code: {}", code),
            Self::Error(error) => write!(f, "Error: {}", error),
        }
    }
}

#[allow(clippy::if_let_some_result)]
pub(crate) fn classify_grpc_metadata(headers: &HeaderMap) -> ParsedGrpcStatus {
    macro_rules! or_else {
        ($expr:expr, $other:ident) => {
            if let Some(value) = $expr {
                value
            } else {
                return ParsedGrpcStatus::$other;
            }
        };
    }

    let status = or_else!(headers.get("grpc-status"), GrpcStatusHeaderMissing);
    let status = or_else!(status.to_str().ok(), HeaderNotString);
    let status = or_else!(status.parse::<i32>().ok(), HeaderNotInt);

    if status == 0 {
        ParsedGrpcStatus::Success
    } else {
        ParsedGrpcStatus::NonSuccess(NonZeroI32::new(status).unwrap())
    }
}

pub(crate) enum ParsedGrpcStatus {
    Success,
    NonSuccess(NonZeroI32),
    GrpcStatusHeaderMissing,
    // these two are treated as `Success` but kept separate for clarity
    HeaderNotString,
    HeaderNotInt,
}

// Just verify that we can actually use this response classifier to determine retries as well
#[cfg(test)]
mod usable_for_retries {
    #[allow(unused_imports)]
    use super::*;
    use hyper::{Request, Response};
    use tower::retry::Policy;

    trait IsRetryable {
        fn is_retryable(&self) -> bool;
    }

    #[derive(Clone)]
    struct RetryBasedOnClassification<C> {
        classifier: C,
        // ...
    }

    impl<ReqB, ResB, E, C> Policy<Request<ReqB>, Response<ResB>, E> for RetryBasedOnClassification<C>
    where
        C: ClassifyResponse + Clone,
        E: fmt::Display + 'static,
        C::FailureClass: IsRetryable,
        ResB: http_body::Body,
        Request<ReqB>: Clone,
        E: std::error::Error + 'static,
    {
        type Future = futures::future::Ready<RetryBasedOnClassification<C>>;

        fn retry(
            &self,
            _req: &Request<ReqB>,
            res: Result<&Response<ResB>, &E>,
        ) -> Option<Self::Future> {
            match res {
                Ok(res) => {
                    if let ClassifiedResponse::Ready(class) =
                        self.classifier.clone().classify_response(res)
                    {
                        if class.err()?.is_retryable() {
                            return Some(futures::future::ready(self.clone()));
                        }
                    }

                    None
                }
                Err(err) => self
                    .classifier
                    .clone()
                    .classify_error(err)
                    .is_retryable()
                    .then(|| futures::future::ready(self.clone())),
            }
        }

        fn clone_request(&self, req: &Request<ReqB>) -> Option<Request<ReqB>> {
            Some(req.clone())
        }
    }
}
