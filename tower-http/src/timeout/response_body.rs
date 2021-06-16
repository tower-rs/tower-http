//! Middleware that adds timeouts to response bodies.
//!
//! Be careful using this with streaming responses as it might abort a stream
//! earlier than othwerwise intended.
//!
//! # Example
//!
//! ```
//! use tower_http::timeout::ResponseBodyTimeoutLayer;
//! use hyper::{Request, Response, Body, Error};
//! use tower::{Service, ServiceBuilder};
//! use std::time::Duration;
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let service = ServiceBuilder::new()
//!     // Make sure the response body completes with 10 seconds
//!     .layer(ResponseBodyTimeoutLayer::new(Duration::from_secs(10)))
//!     .service_fn(handle);
//! # Ok(())
//! # }
//! ```

use futures_core::ready;
use http::Response;
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::time::Sleep;
use tower::BoxError;
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`ResponseBodyTimeout`] which adds a timeout to the
/// response bodies.
///
/// If generating the response body doesn't complete within the specified time,
/// an error is returned.
///
/// See the [module docs](crate::timeout::response_body) for an example.
#[derive(Debug, Clone)]
pub struct ResponseBodyTimeoutLayer {
    timeout: Duration,
}

impl ResponseBodyTimeoutLayer {
    /// Create a new `ResponseBodyTimeoutLayer`.
    pub fn new(timeout: Duration) -> Self {
        ResponseBodyTimeoutLayer { timeout }
    }
}

impl<S> Layer<S> for ResponseBodyTimeoutLayer {
    type Service = ResponseBodyTimeout<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseBodyTimeout::new(inner, self.timeout)
    }
}

/// Middleware that adds a timeout to the response bodies.
///
/// If generating the response body doesn't complete within the specified time,
/// an error is returned.
///
/// See the [module docs](crate::timeout::response_body) for an example.
#[derive(Debug, Clone)]
pub struct ResponseBodyTimeout<S> {
    inner: S,
    timeout: Duration,
}

impl<S> ResponseBodyTimeout<S> {
    /// Create a new `ResponseBodyTimeout`.
    pub fn new(inner: S, timeout: Duration) -> Self {
        Self { inner, timeout }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a [`ResponseBodyTimeout`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(timeout: Duration) -> ResponseBodyTimeoutLayer {
        ResponseBodyTimeoutLayer::new(timeout)
    }
}

impl<S, R, ResBody> Service<R> for ResponseBodyTimeout<S>
where
    S: Service<R, Response = Response<ResBody>>,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        ResponseFuture {
            timeout: self.timeout,
            inner: self.inner.call(req),
        }
    }
}

/// Response future for [`ResponseBodyTimeout`].
#[pin_project]
pub struct ResponseFuture<F> {
    #[pin]
    inner: F,
    timeout: Duration,
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<ResponseBody<B>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = ready!(this.inner.poll(cx));

        match result {
            Ok(res) => {
                let sleep = tokio::time::sleep(*this.timeout);
                let res = res.map(|body| ResponseBody { inner: body, sleep });
                Poll::Ready(Ok(res))
            }
            Err(err) => Poll::Ready(Err(err)),
        }
    }
}

/// Response body for [`ResponseBodyTimeout`].
#[pin_project]
#[derive(Debug)]
pub struct ResponseBody<B> {
    #[pin]
    inner: B,
    #[pin]
    sleep: Sleep,
}

impl<B> Body for ResponseBody<B>
where
    B: Body,
    B::Error: Into<BoxError>,
{
    type Data = B::Data;
    type Error = BoxError;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();

        if let Poll::Ready(chunk) = this.inner.poll_data(cx) {
            let chunk = chunk.map(|chunk| chunk.map_err(Into::into));
            return Poll::Ready(chunk);
        }

        if this.sleep.poll(cx).is_ready() {
            let err = tower::timeout::error::Elapsed::new().into();
            return Poll::Ready(Some(Err(err)));
        }

        Poll::Pending
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let this = self.project();

        if let Poll::Ready(trailers) = this.inner.poll_trailers(cx) {
            let trailers = trailers.map_err(Into::into);
            return Poll::Ready(trailers);
        }

        if this.sleep.poll(cx).is_ready() {
            let err = tower::timeout::error::Elapsed::new().into();
            return Poll::Ready(Err(err));
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::echo;
    use bytes::Bytes;
    use http::{Request, Response};
    use http_body::Body as _;
    use hyper::Body;
    use std::time::Duration;
    use tower::{ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn works_for_poll_data() {
        let timeout = Duration::from_millis(10);

        let mut svc = ServiceBuilder::new()
            .timeout(timeout)
            .layer(ResponseBodyTimeoutLayer::new(timeout))
            .map_response(|res: Response<Body>| res.map(|_| BodyThatHangs))
            .service_fn(echo);

        let req = Request::new(Body::empty());

        let res = svc.ready().await.unwrap().call(req).await.unwrap();
        let mut body = res.into_body();

        let poll_data_err = futures::future::poll_fn(|cx| {
            // Required since `Sleep` isn't `Unpin`.
            unsafe { Pin::new_unchecked(&mut body) }.poll_data(cx)
        })
        .await
        .expect("body was empty")
        .unwrap_err();
        assert!(poll_data_err.is::<tower::timeout::error::Elapsed>());

        let poll_trailers_err = futures::future::poll_fn(|cx| {
            unsafe { Pin::new_unchecked(&mut body) }.poll_trailers(cx)
        })
        .await
        .unwrap_err();
        assert!(poll_trailers_err.is::<tower::timeout::error::Elapsed>());
    }

    struct BodyThatHangs;

    impl http_body::Body for BodyThatHangs {
        type Data = Bytes;
        type Error = tower::BoxError;

        fn poll_data(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
            Poll::Pending
        }

        fn poll_trailers(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
            Poll::Pending
        }
    }
}
