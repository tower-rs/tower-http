//! Tools for classifying responses as either success or failure.

use http::{HeaderMap, Response, StatusCode};
use http_body::Body;
use std::{convert::Infallible, marker::PhantomData};

/// Trait for classifying responses as either success or failure.
pub trait ClassifyResponse {
    /// The output of doing the classification.
    ///
    /// This could be an HTTP status code or some other kind of status.
    type Output;

    /// The type used to classify the response end of stream (EOS).
    type ClassifyEos: ClassifyEos<Output = Self::Output>;

    /// Perform the classification.
    fn classify_response<B>(
        &mut self,
        res: &Response<B>,
    ) -> ClassifiedNowOrLater<Self::Output, Self::ClassifyEos>
    where
        B: Body;
}

/// Trait for classifying end of streams (EOS) as either success or failure.
pub trait ClassifyEos {
    /// The output of doing the classification.
    ///
    /// This could be an HTTP status code or some other kind of status.
    type Output;

    /// Perform the classification.
    fn classify_eos(&mut self, trailers: Option<&HeaderMap>) -> Classification<Self::Output>;
}

/// Result of doing a classification.
#[derive(Debug)]
// really don't like this name... :(
pub enum ClassifiedNowOrLater<T, ClassifyEos> {
    /// The response was able to be classified immediately.
    Ready(Classification<T>),
    /// We have to wait until the end of a streaming response to classify it.
    RequiresEos(ClassifyEos),
}

/// A classification of a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification<T> {
    /// The response was a success.
    Success(T),
    /// The response was a failure.
    Failure(T),
}

/// A [`ClassifyEos`] type that can be used in [`ClassifyResponse`] implementations that never have
/// to classify streaming responses.
///
/// `NeverClassifyEos` exists only as type. You cannot construct `NeverClassifyEos` values.
pub struct NeverClassifyEos<T> {
    _output_ty: PhantomData<T>,
    _never: Infallible,
}

impl<T> ClassifyEos for NeverClassifyEos<T> {
    type Output = T;

    fn classify_eos(&mut self, _trailers: Option<&HeaderMap>) -> Classification<Self::Output> {
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
    type Output = http::StatusCode;
    type ClassifyEos = NeverClassifyEos<StatusCode>;

    fn classify_response<B>(
        &mut self,
        res: &Response<B>,
    ) -> ClassifiedNowOrLater<Self::Output, Self::ClassifyEos>
    where
        B: Body,
    {
        if res.status().is_server_error() {
            ClassifiedNowOrLater::Ready(Classification::Failure(res.status()))
        } else {
            ClassifiedNowOrLater::Ready(Classification::Success(res.status()))
        }
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
    type Output = i32;
    type ClassifyEos = GrpcEosErrorsAsFailures;

    fn classify_response<B>(
        &mut self,
        res: &Response<B>,
    ) -> ClassifiedNowOrLater<Self::Output, Self::ClassifyEos>
    where
        B: Body,
    {
        if let Some(classification) = classify_grpc_metadata(res.headers()) {
            ClassifiedNowOrLater::Ready(classification)
        } else {
            ClassifiedNowOrLater::RequiresEos(GrpcEosErrorsAsFailures { _priv: () })
        }
    }
}

/// The [`ClassifyEos`] for [`GrpcErrorsAsFailures`].
#[derive(Debug, Clone)]
pub struct GrpcEosErrorsAsFailures {
    _priv: (),
}

impl ClassifyEos for GrpcEosErrorsAsFailures {
    type Output = i32;

    fn classify_eos(&mut self, trailers: Option<&HeaderMap>) -> Classification<Self::Output> {
        classify_grpc_metadata(trailers.expect("no trailers"))
            .expect("no or invalid grpc-status in trailers")
    }
}

fn classify_grpc_metadata(headers: &HeaderMap) -> Option<Classification<i32>> {
    let status = headers.get("grpc-status")?;
    let status = status.to_str().ok()?;
    let status = status.parse::<i32>().ok()?;

    if status == 0 {
        Some(Classification::Success(status))
    } else {
        Some(Classification::Failure(status))
    }
}
