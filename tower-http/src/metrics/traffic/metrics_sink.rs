use super::FailedAt;
use crate::classify::ClassifiedResponse;
use http::{HeaderMap, Request, Response};

/// Trait that defines callbacks for [`Traffic`] to call.
///
/// The generic `FailureClass` parameter is the failure class of the classifier passed to
/// [`Traffic::new`] or [`TrafficLayer::new`]).
pub trait MetricsSink<FailureClass>: Sized {
    /// Additional data required for creating metric events.
    ///
    /// This could for example be a struct that contains the request path and HTTP method so they
    /// can be included in events.
    type Data;

    /// Create an instance of `Self::Data` from the request.
    ///
    /// This method is called immediately after the request is received by [`Service::call`].
    ///
    /// The value returned here will be passed to the other methods in this trait.
    fn prepare<B>(&mut self, request: &Request<B>) -> Self::Data;

    /// Perform some action when a response has been generated.
    ///
    /// This method is called when the inner [`Service`]'s response future completes with
    /// `Ok(response)`, regardless if the response is classified as a success or a failure.
    ///
    /// If the response is the start of a stream (as determined by the classifier passed to
    /// [`Traffic::new`] or [`TrafficLayer::new`]) then `classification` will be
    /// [`ClassifiedResponse::RequiresEos(())`], otherwise it will be
    /// [`ClassifiedResponse::Ready`].
    ///
    /// The default implementation does nothing and returns immediately.
    #[inline]
    #[allow(unused_variables)]
    fn on_response<B>(
        &mut self,
        response: &Response<B>,
        classification: ClassifiedResponse<FailureClass, ()>,
        data: &mut Self::Data,
    ) {
    }

    /// Perform some action when a stream has ended.
    ///
    /// This is called when [`Body::poll_trailers`] completes with `Ok(trailers)` regardless if
    /// the trailers are classified as a failure.
    ///
    /// A stream that ends succesfully will trigger two callbacks. [`on_response`] will be called
    /// once the response has been generated and the stream has started and [`on_eos`] will be
    /// called once the stream has ended.
    ///
    /// If the trailers were classified as a success then `classification` will be `Ok(())`
    /// otherwise `Err(failure_class)`.
    ///
    /// The default implementation does nothing and returns immediately.
    ///
    /// [`on_response`]: MetricsSink::on_response
    /// [`on_eos`]: MetricsSink::on_eos
    #[inline]
    #[allow(unused_variables)]
    fn on_eos(
        self,
        trailers: Option<&HeaderMap>,
        classification: Result<(), FailureClass>,
        data: Self::Data,
    ) {
    }

    /// Perform some action when an error has been encountered.
    ///
    /// This method is only called in these scenarios:
    ///
    /// - The inner [`Service`]'s response future resolves to an error.
    /// - [`Body::poll_data`] returns an error.
    /// - [`Body::poll_trailers`] returns an error.
    ///
    /// That means this method is _not_ called if a response is classified as a failure (then
    /// [`on_response`] is called) or an end-of-stream is classified as a failure (then [`on_eos`]
    /// is called).
    ///
    /// `failed_at` specifies where the error happened.
    ///
    /// The default implementation does nothing and returns immediately.
    ///
    /// [`on_response`]: MetricsSink::on_response
    /// [`on_eos`]: MetricsSink::on_eos
    #[inline]
    #[allow(unused_variables)]
    fn on_failure(
        self,
        failed_at: FailedAt,
        failure_classification: FailureClass,
        data: Self::Data,
    ) {
    }
}
