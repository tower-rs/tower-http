//! Tools for classifying responses as either success or failure.

use http::{HeaderMap, Request, Response, StatusCode};
use http_body::Body;
use std::{convert::Infallible, marker::PhantomData};

/// Trait for producing response classifiers from a request.
///
/// This is useful if your classifier depends on data from the request. Could for example be the
/// URI or HTTP method.
///
/// This trait is generic over the error type your services produce. This is necessary for
/// [`ClassifyResponse::classify_error`].
pub trait MakeClassifier<E> {
    /// The response classifier produced.
    type Classifier: ClassifyResponse<
        E,
        FailureClass = Self::FailureClass,
        ClassifyEos = Self::ClassifyEos,
    >;

    /// The type of failure classifications.
    type FailureClass;

    /// The type used to classify the response end of stream (EOS).
    type ClassifyEos: ClassifyEos<FailureClass = Self::FailureClass>;

    /// Make a response classifier for this request
    fn make_classifier<B>(&self, req: &Request<B>) -> Self::Classifier
    where
        B: Body;
}

/// A [`MakeClassifier`] that works by cloning a classifier.
///
/// If you have a [`ClassifyResponse`] that doesn't depend on the request you can use
/// [`SharedClassifier`] to get a [`MakeClassifier`] for your classifier.
#[derive(Debug, Clone)]
pub struct SharedClassifier<C> {
    classifier: C,
}

impl<C> SharedClassifier<C> {
    /// Create a new `SharedClassifier` from the given classifier.
    pub fn new<E>(classifier: C) -> Self
    where
        C: ClassifyResponse<E> + Clone,
    {
        Self { classifier }
    }
}

impl<C, E> MakeClassifier<E> for SharedClassifier<C>
where
    C: ClassifyResponse<E> + Clone,
{
    type FailureClass = C::FailureClass;
    type ClassifyEos = C::ClassifyEos;
    type Classifier = C;

    fn make_classifier<B>(&self, _req: &Request<B>) -> Self::Classifier
    where
        B: Body,
    {
        self.classifier.clone()
    }
}

/// Trait for classifying responses as either success or failure. Designed to support both unary
/// requests (single request for a single response) as well as streaming responses.
///
/// Can for example be used in logging or metrics middlewares, or to build [retry policies].
///
/// [retry policies]: https://docs.rs/tower/latest/tower/retry/trait.Policy.html
pub trait ClassifyResponse<E> {
    /// The type of failure classifications.
    type FailureClass;

    /// The type used to classify the response end of stream (EOS).
    type ClassifyEos: ClassifyEos<FailureClass = Self::FailureClass>;

    /// Classify a response.
    ///
    /// Returns a [`ClassifiedResponse`] which specifies whether the response was able to
    /// classified immediately or if we have to wait until the end of a streaming response.
    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos>
    where
        B: Body;

    /// Classify an error.
    ///
    /// Errors are always errors (doh) but sometimes it might be useful to have multiple classes of
    /// errors. A retry policy might allows retrying some errors and not others.
    fn classify_error(self, error: &E) -> Self::FailureClass;
}

/// Trait for classifying end of streams (EOS) as either success or failure.
pub trait ClassifyEos {
    /// The type of failure classifications.
    type FailureClass;

    /// Perform the classification from response trailers.
    fn classify_eos(self, trailers: Option<&HeaderMap>) -> Result<(), Self::FailureClass>;
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

    /// Returns a [`MakeClassifier`] that produces `ServerErrorsAsFailures`.
    ///
    /// This is a convenience function that simply calls `SharedClassifier::new`.
    pub fn make_classifier<E>() -> SharedClassifier<Self> {
        SharedClassifier::new::<E>(Self::new())
    }
}

impl<E> ClassifyResponse<E> for ServerErrorsAsFailures {
    type FailureClass = StatusCode;
    type ClassifyEos = NeverClassifyEos<StatusCode>;

    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos>
    where
        B: Body,
    {
        if res.status().is_server_error() {
            ClassifiedResponse::Ready(Err(res.status()))
        } else {
            ClassifiedResponse::Ready(Ok(()))
        }
    }

    fn classify_error(self, _error: &E) -> Self::FailureClass {
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

    /// Returns a [`MakeClassifier`] that produces `GrpcErrorsAsFailures`.
    ///
    /// This is a convenience function that simply calls `SharedClassifier::new`.
    pub fn make_classifier<E>() -> SharedClassifier<Self> {
        SharedClassifier::new::<E>(Self::new())
    }
}

impl<E> ClassifyResponse<E> for GrpcErrorsAsFailures {
    type FailureClass = i32;
    type ClassifyEos = GrpcEosErrorsAsFailures;

    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos>
    where
        B: Body,
    {
        if let Some(classification) = classify_grpc_metadata(res.headers()) {
            ClassifiedResponse::Ready(classification)
        } else {
            ClassifiedResponse::RequiresEos(GrpcEosErrorsAsFailures { _priv: () })
        }
    }

    fn classify_error(self, _error: &E) -> Self::FailureClass {
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
        C: ClassifyResponse<E> + Clone,
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
