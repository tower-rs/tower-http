//! Middleware that applies a timeout to bodies.
//!
//! Bodies must produce data at most within the specified timeout.
//! If they are inactive, an error will be generated.
//!
//! # Differences from `tower_http::timeout::service::Timeout`
//!
//! `tower_http::timeout::service::Timeout` applies a timeout on the full request.
//! That timeout is not reset when bytes are handled, whether the request is active or not.
//!
//! This middleware will return a [`TimeoutError`].
//!
//! # Example
//!
//! ```
//! use http::{Request, Response};
//! use hyper::Body;
//! use std::time::Duration;
//! use tower::ServiceBuilder;
//! use tower_http::timeout::body::RequestBodyTimeoutLayer;
//!
//! async fn handle(_: Request<Body>) -> Result<Response<Body>, std::convert::Infallible> {
//!     // ...
//!     # todo!()
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let svc = ServiceBuilder::new()
//!     // Timeout bodies after 30 seconds of inactivity
//!     .layer(RequestBodyTimeoutLayer::new(Duration::from_secs(30)))
//!     .service_fn(handle);
//! # Ok(())
//! # }
//! ```

use futures_core::{ready, Future};
use http::{Request, Response};
use http_body::Body;
use pin_project_lite::pin_project;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::time::{sleep, Sleep};
use tower::BoxError;
use tower_layer::Layer;
use tower_service::Service;

pin_project! {
    /// Wrapper around a [`http_body::Body`] to time out if data is not ready within the specified duration.
    pub struct TimeoutBody<B> {
        timeout: Duration,
        #[pin]
        sleep: Option<Sleep>,
        #[pin]
        body: B,
    }
}

impl<B> TimeoutBody<B> {
    /// Creates a new [`TimeoutBody`].
    pub fn new(timeout: Duration, body: B) -> Self {
        TimeoutBody {
            timeout,
            sleep: None,
            body,
        }
    }
}

impl<B> Body for TimeoutBody<B>
where
    B: Body,
    B::Error: Into<BoxError>,
{
    type Data = B::Data;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let mut this = self.project();

        // Start the `Sleep` if not active.
        let sleep_pinned = if let Some(some) = this.sleep.as_mut().as_pin_mut() {
            some
        } else {
            this.sleep.set(Some(sleep(*this.timeout)));
            this.sleep.as_mut().as_pin_mut().unwrap()
        };

        // Error if the timeout has expired.
        if let Poll::Ready(()) = sleep_pinned.poll(cx) {
            return Poll::Ready(Some(Err(Box::new(TimeoutError(())))));
        }

        // Check for body data.
        let data = ready!(this.body.poll_data(cx));
        // Some data is ready. Reset the `Sleep`...
        this.sleep.set(None);

        Poll::Ready(data.transpose().map_err(Into::into).transpose())
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let mut this = self.project();

        let sleep_pinned = if let Some(some) = this.sleep.as_mut().as_pin_mut() {
            some
        } else {
            this.sleep.set(Some(sleep(*this.timeout)));
            this.sleep.as_mut().as_pin_mut().unwrap()
        };

        // Error if the timeout has expired.
        if let Poll::Ready(()) = sleep_pinned.poll(cx) {
            return Poll::Ready(Err(Box::new(TimeoutError(()))));
        }

        this.body.poll_trailers(cx).map_err(Into::into)
    }
}

/// Error for [`TimeoutBody`].
#[derive(Debug)]
pub struct TimeoutError(());

impl std::error::Error for TimeoutError {}

impl std::fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "data was not received within the designated timeout")
    }
}

/// Applies a [`TimeoutBody`] to the request body.
#[derive(Clone, Debug)]
pub struct RequestBodyTimeoutLayer {
    timeout: Duration,
}

impl RequestBodyTimeoutLayer {
    /// Creates a new [`RequestBodyTimeoutLayer`].
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl<S> Layer<S> for RequestBodyTimeoutLayer {
    type Service = RequestBodyTimeout<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestBodyTimeout::new(inner, self.timeout)
    }
}

/// Applies a [`TimeoutBody`] to the request body.
#[derive(Clone, Debug)]
pub struct RequestBodyTimeout<S> {
    inner: S,
    timeout: Duration,
}

impl<S> RequestBodyTimeout<S> {
    /// Creates a new [`RequestBodyTimeout`].
    pub fn new(service: S, timeout: Duration) -> Self {
        Self {
            inner: service,
            timeout,
        }
    }

    /// Returns a new [`Layer`] that wraps services with a [`RequestBodyTimeoutLayer`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(timeout: Duration) -> RequestBodyTimeoutLayer {
        RequestBodyTimeoutLayer::new(timeout)
    }

    define_inner_service_accessors!();
}

impl<S, ReqBody> Service<Request<ReqBody>> for RequestBodyTimeout<S>
where
    S: Service<Request<TimeoutBody<ReqBody>>>,
    S::Error: Into<Box<dyn std::error::Error>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let req = req.map(|body| TimeoutBody::new(self.timeout, body));
        self.inner.call(req)
    }
}

/// Applies a [`TimeoutBody`] to the response body.
#[derive(Clone)]
pub struct ResponseBodyTimeoutLayer {
    timeout: Duration,
}

impl ResponseBodyTimeoutLayer {
    /// Creates a new [`ResponseBodyTimeoutLayer`].
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl<S> Layer<S> for ResponseBodyTimeoutLayer {
    type Service = ResponseBodyTimeout<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseBodyTimeout::new(inner, self.timeout)
    }
}

/// Applies a [`TimeoutBody`] to the response body.
#[derive(Clone)]
pub struct ResponseBodyTimeout<S> {
    inner: S,
    timeout: Duration,
}

impl<S> ResponseBodyTimeout<S> {
    /// Creates a new [`ResponseBodyTimeout`].
    pub fn new(service: S, timeout: Duration) -> Self {
        Self {
            inner: service,
            timeout,
        }
    }

    /// Returns a new [`Layer`] that wraps services with a [`ResponseBodyTimeoutLayer`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(timeout: Duration) -> ResponseBodyTimeoutLayer {
        ResponseBodyTimeoutLayer::new(timeout)
    }

    define_inner_service_accessors!();
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for ResponseBodyTimeout<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    S::Error: Into<Box<dyn std::error::Error>>,
{
    type Response = Response<TimeoutBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseBodyTimeoutFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        ResponseBodyTimeoutFuture {
            inner: self.inner.call(req),
            timeout: self.timeout,
        }
    }
}

pin_project! {
    /// Response future for [`ResponseBodyTimeout`].
    pub struct ResponseBodyTimeoutFuture<Fut> {
        #[pin]
        inner: Fut,
        timeout: Duration,
    }
}

impl<Fut, ResBody, E> Future for ResponseBodyTimeoutFuture<Fut>
where
    Fut: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<TimeoutBody<ResBody>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let timeout = self.timeout;
        let this = self.project();
        let res = ready!(this.inner.poll(cx))?;
        Poll::Ready(Ok(res.map(|body| TimeoutBody::new(timeout, body))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bytes::Bytes;
    use pin_project_lite::pin_project;
    use std::{error::Error, fmt::Display};

    #[derive(Debug)]
    struct MockError;

    impl Error for MockError {}
    impl Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            todo!()
        }
    }

    pin_project! {
        struct MockBody {
            #[pin]
            sleep: Sleep
        }
    }

    impl Body for MockBody {
        type Data = Bytes;
        type Error = MockError;

        fn poll_data(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
            let this = self.project();
            this.sleep.poll(cx).map(|_| Some(Ok(vec![].into())))
        }

        fn poll_trailers(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
            todo!()
        }
    }

    #[tokio::test]
    async fn test_body_available_within_timeout() {
        let mock_sleep = Duration::from_secs(1);
        let timeout_sleep = Duration::from_secs(2);

        let mock_body = MockBody {
            sleep: sleep(mock_sleep),
        };
        let timeout_body = TimeoutBody::new(timeout_sleep, mock_body);

        assert!(timeout_body.boxed().data().await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn test_body_unavailable_within_timeout_error() {
        let mock_sleep = Duration::from_secs(2);
        let timeout_sleep = Duration::from_secs(1);

        let mock_body = MockBody {
            sleep: sleep(mock_sleep),
        };
        let timeout_body = TimeoutBody::new(timeout_sleep, mock_body);

        assert!(timeout_body.boxed().data().await.unwrap().is_err());
    }
}
