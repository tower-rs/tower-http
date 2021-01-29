use http::{Response, StatusCode};
use std::fmt;

/// Classify requests results as either success or failure.
///
/// Not all services can rely entirely on the status code to determine if its a success or failure.
/// gRPC for example has its own way of communicating that and always uses `200 OK`.
pub trait ClassifyResponse<Body, Err> {
    /// The type used to classify successful responses.
    type OkClass;

    /// The type used to classify failed responses.
    type ErrClass;

    /// Do the classification.
    fn classify_request_result(
        &self,
        request_result: &Result<Response<Body>, Err>,
    ) -> ResponseClassification<Self::OkClass, Self::ErrClass>;
}

/// Was a response a success or a failure?
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResponseClassification<T, E> {
    /// The response was a success.
    ///
    /// `T` is a value produced by doing the classification. Could for example be the status code
    /// that a middleware uses.
    Ok(T),

    /// The response was a failure.
    ///
    /// `E` is a value produced by doing the classification. Could for example be an error.
    Err(E),
}

/// The default [`ClassifyResponse`] that some middlewares use.
///
/// It classifies `Ok(response)` as success unless the status code is `5xx`. Errors are always
/// classified as errors.
#[derive(Debug, Clone, Default)]
pub struct DefaultHttpResponseClassifier {
    _priv: (),
}

impl DefaultHttpResponseClassifier {
    /// Create a new [`DefaultHttpResponseClassifier`].
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl<B, E> ClassifyResponse<B, E> for DefaultHttpResponseClassifier
where
    E: fmt::Display,
{
    type OkClass = StatusCode;
    type ErrClass = DefaultErrorClassification;

    fn classify_request_result(
        &self,
        request_result: &Result<Response<B>, E>,
    ) -> ResponseClassification<StatusCode, DefaultErrorClassification> {
        match request_result {
            Ok(res) if res.status().is_server_error() => {
                let err = format!("Server error. status={}", res.status());
                let err = DefaultErrorClassification { err };
                ResponseClassification::Err(err)
            }
            Ok(res) => ResponseClassification::Ok(res.status()),
            Err(err) => ResponseClassification::Err(DefaultErrorClassification {
                err: err.to_string(),
            }),
        }
    }
}

/// The error classification used by [`DefaultHttpResponseClassifier`].
#[derive(Debug, Clone)]
pub struct DefaultErrorClassification {
    pub(crate) err: String,
}
