//! Limit the max number of concurrently processed requests.
//!
//! The service sets a maximum limit to the number of concurrently processed requests. The
//! processing of a request starts when it is received by the service (`tower::Service::call` is
//! called) and is considered complete when the response body is consumed, dropped, or an error
//! happens.
//!
//! Internally, it uses semaphore to track and limit number of in-flight requests
//!
//! # Relation to `ConcurrencyLimit` from `tower` crate
//!
//! The `tower::limit::concurrency::ConcurrencyLimit` service uses a different definition of
//! 'request processing'. It starts when request is received by `tower::Service::call`, and ends
//! immediatelly after response is produced.
//!
//! In some cases it may not work properly with [`http::Response`], as it does not account for
//! process of consuming response body.
//!
//! When stream is used as response body, the process of consumig it (ie streaming to called) may
//! take longer and use more resources than just producing the response itself. And often it the
//! number of streams we are processing concurrently we want to limit.
//!
//! The service version from [`tower-http`](crate) takes response body consumption into
//! consideration and *will* limit number of concurrent streams correctly.
//!
//! ```
//! use std::convert::Infallible;
//! use bytes::Bytes;
//! use http::{Request, Response};
//! use http_body_util::Full;
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower_http::concurrency_limit::ConcurrencyLimitLayer;
//!
//! async fn handle(req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, Infallible> {
//!     // ...
//!     # Ok(Response::new(Full::default()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut service = ServiceBuilder::new()
//!     // limit number of concurrent requests to 3
//!     .layer(ConcurrencyLimitLayer::new(3))
//!     .service_fn(handle);
//!
//! // Call the service.
//! let response = service
//!     .ready()
//!     .await?
//!     .call(Request::new(Full::default()))
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!

use http::{Request, Response};
use pin_project_lite::pin_project;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::PollSemaphore;

/// Limit max number of concurrent requests (per service)
///
/// The layer enforces a same concurrency limit for each inner service separately. In other words,
/// [`ConcurrencyLimit`] middleware constructed from this layer for each service will have its own
/// semaphore and will track and limit requests separately.
///
/// To track and limit multiple services together, see [`SharedConcurrencyLimitLayer`]
///
/// See the [module docs](crate::concurrency_limit) for more details.
#[derive(Clone, Debug)]
pub struct ConcurrencyLimitLayer {
    max: usize,
}

impl ConcurrencyLimitLayer {
    /// Create new [`ConcurrencyLimitLayer`] with semaphore size
    pub fn new(max: usize) -> Self {
        Self { max }
    }
}

impl<S> tower_layer::Layer<S> for ConcurrencyLimitLayer {
    type Service = ConcurrencyLimit<S>;

    fn layer(&self, service: S) -> Self::Service {
        ConcurrencyLimit::new(service, Arc::new(Semaphore::new(self.max)))
    }
}

/// Limit max number of concurrent requests (shared)
///
/// The layer enforces a single concurrency limit for multiple inner services at once. In other
/// words, [`ConcurrencyLimit`] middleware constructed from this layer for each service will have
/// one shared semaphore and will track and limit requests together..
///
/// To track and limit each service separately, see [`ConcurrencyLimitLayer`].
///
/// See the [module docs](crate::concurrency_limit) for more details.
#[derive(Clone, Debug)]
pub struct SharedConcurrencyLimitLayer {
    semaphore: Arc<Semaphore>,
}

impl SharedConcurrencyLimitLayer {
    /// Create new [`ConcurrencyLimitLayer`] with shared semaphore
    pub fn new(max: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max)),
        }
    }

    /// Create [`ConcurrencyLimitLayer`] from semaphore
    pub fn from_semaphore(semaphore: Arc<Semaphore>) -> Self {
        Self { semaphore }
    }
}

impl<S> tower_layer::Layer<S> for SharedConcurrencyLimitLayer {
    type Service = ConcurrencyLimit<S>;

    fn layer(&self, service: S) -> Self::Service {
        ConcurrencyLimit::new(service, self.semaphore.clone())
    }
}

/// Middleware that limits max number fo concurrent in-flight requests.
///
/// See the [module docs](crate::concurrency_limit) for more details.
#[derive(Debug)]
pub struct ConcurrencyLimit<S> {
    inner: S,
    semaphore: PollSemaphore,
    permit: Option<OwnedSemaphorePermit>,
}

impl<S> ConcurrencyLimit<S> {
    /// Create new [`ConcurrencyLimit`] with associated semaphore
    pub fn new(inner: S, semaphore: Arc<Semaphore>) -> Self {
        Self {
            inner,
            semaphore: PollSemaphore::new(semaphore),
            permit: None,
        }
    }

    define_inner_service_accessors!();
}

// Since we hold an `OwnedSemaphorePermit`, we can't derive `Clone`. Instead, when cloning the
// service, create a new service with the same semaphore, but with the permit in the un-acquired
// state.
impl<T: Clone> Clone for ConcurrencyLimit<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            semaphore: self.semaphore.clone(),
            permit: None,
        }
    }
}

impl<S, R, Body> tower_service::Service<Request<R>> for ConcurrencyLimit<S>
where
    S: tower_service::Service<Request<R>, Response = Response<Body>>,
{
    type Response = Response<ResponseBody<Body>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.permit.is_none() {
            self.permit = ready!(self.semaphore.poll_acquire(cx));
        }

        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<R>) -> Self::Future {
        let permit = self
            .permit
            .take()
            .expect("max requests in-flight; poll_ready must be called first");

        let future = self.inner.call(request);
        ResponseFuture {
            inner: future,
            permit: Some(permit),
        }
    }
}

pin_project! {

    /// Response future for [`ConcurrencyLimit`]
    pub struct ResponseFuture<F> {
        #[pin]
        inner: F,

        // The permit is stored inside option, so that we can take it out from the future on its
        // completion and pass it to the ResponseBody. The permit has to be droped only after
        // ResponseBody is consumed.
        permit: Option<OwnedSemaphorePermit>,
    }
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<ResponseBody<B>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let response = ready!(this.inner.poll(cx))?;

        let permit = this.permit.take().unwrap();
        let response = response.map(move |body| ResponseBody {
            inner: body,
            permit,
        });

        Poll::Ready(Ok(response))
    }
}

pin_project! {

    /// Response body for [`ConcurrencyLimit`]
    ///
    /// It enforces limit on number of `struct` instances in concurrent existence.
    pub struct ResponseBody<B> {
        #[pin]
        inner: B,
        permit: OwnedSemaphorePermit,
    }
}

impl<B> http_body::Body for ResponseBody<B>
where
    B: http_body::Body,
{
    type Data = B::Data;
    type Error = B::Error;

    #[inline]
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        self.project().inner.poll_frame(cx)
    }

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    #[inline]
    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::Body;
    use http::Request;
    use tower::{BoxError, ServiceBuilder};
    use tower_service::Service;

    #[tokio::test]
    async fn basic() {
        let semaphore = Arc::new(Semaphore::new(1));
        assert_eq!(1, semaphore.available_permits());

        let mut service = ServiceBuilder::new()
            .layer(SharedConcurrencyLimitLayer::from_semaphore(
                semaphore.clone(),
            ))
            .service_fn(echo);

        // driving service to ready pre-acquire semaphore permit, decrease available count
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .unwrap();
        assert_eq!(0, semaphore.available_permits());

        // creating response future decreases number of permits
        let response_future = service.call(Request::new(Body::empty()));

        // awaiting response future moves permit to response, no change in available count
        let response = response_future.await.unwrap();
        assert_eq!(0, semaphore.available_permits());

        // consuming response frees permit and increase available count
        let body = response.into_body();
        crate::test_helpers::to_bytes(body).await.unwrap();
        assert_eq!(1, semaphore.available_permits());
    }

    async fn echo(req: Request<Body>) -> Result<Response<Body>, BoxError> {
        Ok(Response::new(req.into_body()))
    }
}
