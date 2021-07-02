use super::FailedAt;
use crate::classify::ClassifiedResponse;
use bytes::Buf;
use http::{HeaderMap, Request, Response};

/// Trait that defines callbacks for [`LifeCycleHooks`] to call.
///
/// The generic `FailureClass` parameter is the failure class of the classifier
/// passed to [`LifeCycleHooks::new`] or [`LifeCycleHooksLayer::new`]).
///
/// [`LifeCycleHooks`]: crate::life_cycle_hooks::LifeCycleHooks
/// [`LifeCycleHooks::new`]: crate::life_cycle_hooks::LifeCycleHooks::new
/// [`LifeCycleHooksLayer::new`]: crate::life_cycle_hooks::LifeCycleHooksLayer::new
pub trait Callbacks<FailureClass>: Sized {
    /// Additional data required for callbacks.
    type Data;

    /// Create an instance of `Self::Data` from the request.
    ///
    /// This method is called immediately after the request is received by
    /// [`Service::call`].
    ///
    /// The value returned here will be passed to the other methods in this
    /// trait.
    ///
    /// [`Service::call`]: tower::Service::call
    fn prepare<B>(&mut self, request: &Request<B>) -> Self::Data;

    /// Perform some action when a response has been generated.
    ///
    /// This method is called when the inner [`Service`]'s response future
    /// completes with `Ok(response)`, regardless if the response is classified
    /// as a success or a failure.
    ///
    /// If the response is the start of a stream (as determined by the
    /// classifier passed to [`LifeCycleHooks::new`] or [`LifeCycleHooksLayer::new`]) then
    /// `classification` will be [`ClassifiedResponse::RequiresEos(())`],
    /// otherwise it will be [`ClassifiedResponse::Ready`].
    ///
    /// The default implementation does nothing and returns immediately.
    ///
    /// [`LifeCycleHooks::new`]: crate::life_cycle_hooks::LifeCycleHooks::new
    /// [`LifeCycleHooksLayer::new`]: crate::life_cycle_hooks::LifeCycleHooksLayer::new
    /// [`ClassifiedResponse::RequiresEos(())`]: crate::classify::ClassifiedResponse::RequiresEos
    /// [`Service`]: tower::Service
    #[inline]
    fn on_response<B>(
        &mut self,
        _response: &Response<B>,
        _classification: ClassifiedResponse<FailureClass, ()>,
        _data: &mut Self::Data,
    ) {
    }

    /// Perform some action when a response body chunk has been generated.
    ///
    /// This is called when [`Body::poll_data`] completes with `Some(Ok(chunk))`
    /// regardless if the chunk is empty or not.
    ///
    /// The default implementation does nothing and returns immediately.
    ///
    /// [`Body::poll_data`]: http_body::Body::poll_data
    #[inline]
    fn on_body_chunk<B>(&self, _chunk: &B, _data: &Self::Data)
    where
        B: Buf,
    {
    }

    /// Perform some action when a stream has ended.
    ///
    /// This is called when [`Body::poll_trailers`] completes with
    /// `Ok(trailers)` regardless if the trailers are classified as a failure.
    ///
    /// A stream that ends successfully will trigger two callbacks.
    /// [`on_response`] will be called once the response has been generated and
    /// the stream has started and [`on_eos`] will be called once the stream has
    /// ended.
    ///
    /// If the trailers were classified as a success then `classification` will
    /// be `Ok(())` otherwise `Err(failure_class)`.
    ///
    /// The default implementation does nothing and returns immediately.
    ///
    /// [`on_response`]: Callbacks::on_response
    /// [`on_eos`]: Callbacks::on_eos
    /// [`Body::poll_trailers`]: http_body::Body::poll_trailers
    #[inline]
    fn on_eos(
        self,
        _trailers: Option<&HeaderMap>,
        _classification: Result<(), FailureClass>,
        _data: Self::Data,
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
    /// That means this method is _not_ called if a response is classified as a
    /// failure (then [`on_response`] is called) or an end-of-stream is
    /// classified as a failure (then [`on_eos`] is called).
    ///
    /// `failed_at` specifies where the error happened.
    ///
    /// The default implementation does nothing and returns immediately.
    ///
    /// [`Service`]: tower::Service
    /// [`on_response`]: Callbacks::on_response
    /// [`on_eos`]: Callbacks::on_eos
    /// [`Service::call`]: tower::Service::call
    /// [`Body::poll_data`]: http_body::Body::poll_data
    /// [`Body::poll_trailers`]: http_body::Body::poll_trailers
    #[inline]
    fn on_failure(
        self,
        _failed_at: FailedAt,
        _failure_classification: FailureClass,
        _data: Self::Data,
    ) {
    }
}
