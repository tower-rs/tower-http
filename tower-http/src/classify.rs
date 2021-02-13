//! Tools for classifying responses as either success or failure.

use http::{HeaderMap, Request, Response, StatusCode};
use http_body::Body;
use std::{convert::Infallible, marker::PhantomData};

/// Trait for producing response classifiers from a request.
///
/// This is useful if your classifier depends on data from the request. Could for example be the
/// URI or HTTP method.
pub trait MakeClassifier {
    /// The response classifier produced.
    type Classifier: ClassifyResponse<
        FailureClass = Self::FailureClass,
        ClassifyEos = Self::ClassifyEos,
    >;

    /// The type of failure classifications.
    type FailureClass;

    /// The type used to classify the response end of stream (EOS).
    type ClassifyEos: ClassifyEos<FailureClass = Self::FailureClass>;

    /// Make a response classifier for this request
    fn make_classify<B>(&self, req: &Request<B>) -> Self::Classifier
    where
        B: Body;
}

/// A `MakeClassifier` that works by cloning a classifier.
#[derive(Debug, Clone)]
pub struct SharedClassifier<C> {
    classifier: C,
}

impl<C> SharedClassifier<C> {
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

    fn make_classify<B>(&self, _req: &Request<B>) -> Self::Classifier
    where
        B: Body,
    {
        self.classifier.clone()
    }
}

/// Trait for classifying responses as either success or failure.
pub trait ClassifyResponse {
    /// The type of failure classifications.
    type FailureClass;

    /// The type used to classify the response end of stream (EOS).
    type ClassifyEos: ClassifyEos<FailureClass = Self::FailureClass>;

    /// Classify a response.
    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedNowOrLater<Self::FailureClass, Self::ClassifyEos>
    where
        B: Body;

    /// Classify an error
    fn classify_error<E>(self, error: &E) -> Self::FailureClass
    where
        E: std::error::Error + 'static;
}

/// Trait for classifying end of streams (EOS) as either success or failure.
pub trait ClassifyEos {
    /// The type of failure classifications.
    type FailureClass;

    /// Perform the classification.
    fn classify_eos(self, trailers: Option<&HeaderMap>) -> Result<(), Self::FailureClass>;
}

/// Result of doing a classification.
#[derive(Debug)]
// really don't like this name... :(
pub enum ClassifiedNowOrLater<FailureClass, ClassifyEos> {
    /// The response was able to be classified immediately.
    Ready(Result<(), FailureClass>),
    /// We have to wait until the end of a streaming response to classify it.
    RequiresEos(ClassifyEos),
}

/// A [`ClassifyEos`] type that can be used in [`ClassifyResponse`] implementations that never have
/// to classify streaming responses.
///
/// `NeverClassifyEos` exists only as type. You cannot construct `NeverClassifyEos` values.
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
}

/// The default classifier used for normal HTTP responses.
///
/// Responses with a `5xx` status code are failures, all others as successes.
#[derive(Clone, Debug, Default)]
pub struct ServerErrorsAsFailures {
    _priv: (),
}

impl ServerErrorsAsFailures {
    /// Create a new [`ServerErrorsAsFailures`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClassifyResponse for ServerErrorsAsFailures {
    type FailureClass = StatusCode;
    type ClassifyEos = NeverClassifyEos<StatusCode>;

    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedNowOrLater<Self::FailureClass, Self::ClassifyEos>
    where
        B: Body,
    {
        if res.status().is_server_error() {
            ClassifiedNowOrLater::Ready(Err(res.status()))
        } else {
            ClassifiedNowOrLater::Ready(Ok(()))
        }
    }

    fn classify_error<E>(self, _error: &E) -> Self::FailureClass
    where
        E: std::error::Error + 'static,
    {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// Response classifier for gRPC responses.
///
/// gRPC doesn't use normal HTTP statuses for indicating success or failure but instead a special
/// header that might appear in a trailer.
///
/// Responses where `grpc-status` is 0 are successes, all others are failures.
#[derive(Debug, Clone, Default)]
pub struct GrpcErrorsAsFailures {
    _priv: (),
}

impl GrpcErrorsAsFailures {
    /// Create a new [`GrpcErrorsAsFailures`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClassifyResponse for GrpcErrorsAsFailures {
    type FailureClass = i32;
    type ClassifyEos = GrpcEosErrorsAsFailures;

    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedNowOrLater<Self::FailureClass, Self::ClassifyEos>
    where
        B: Body,
    {
        if let Some(classification) = classify_grpc_metadata(res.headers()) {
            ClassifiedNowOrLater::Ready(classification)
        } else {
            ClassifiedNowOrLater::RequiresEos(GrpcEosErrorsAsFailures { _priv: () })
        }
    }

    fn classify_error<E>(self, _error: &E) -> Self::FailureClass
    where
        E: std::error::Error + 'static,
    {
        13
    }
}

/// The [`ClassifyEos`] for [`GrpcErrorsAsFailures`].
#[derive(Debug, Clone)]
pub struct GrpcEosErrorsAsFailures {
    _priv: (),
}

impl ClassifyEos for GrpcEosErrorsAsFailures {
    type FailureClass = i32;

    fn classify_eos(self, trailers: Option<&HeaderMap>) -> Result<(), i32> {
        trailers.and_then(classify_grpc_metadata).unwrap_or(Ok(()))
    }
}

fn classify_grpc_metadata(headers: &HeaderMap) -> Option<Result<(), i32>> {
    let status = headers.get("grpc-status")?;
    let status = status.to_str().ok()?;
    let status = status.parse::<i32>().ok()?;

    if status == 0 {
        Some(Ok(()))
    } else {
        Some(Err(status))
    }
}
