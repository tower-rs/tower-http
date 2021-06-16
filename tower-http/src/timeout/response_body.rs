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

use super::TimeoutBody;
use futures_core::ready;
use http::Response;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`ResponseBodyTimeout`] which adds a timeout to the
/// response body.
///
/// If generating the response body doesn't complete within the specified time,
/// an error is returned.
///
/// See the [module docs](crate::timeout::response_body) for an example.
#[derive(Debug, Copy, Clone)]
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
#[derive(Debug, Copy, Clone)]
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
    type Response = Response<TimeoutBody<ResBody>>;
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
    type Output = Result<Response<TimeoutBody<B>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = ready!(this.inner.poll(cx));

        match result {
            Ok(res) => {
                let timeout = *this.timeout;
                let res = res.map(|body| TimeoutBody::new(body, timeout));
                Poll::Ready(Ok(res))
            }
            Err(err) => Poll::Ready(Err(err)),
        }
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

    #[tokio::test]
    async fn doesnt_start_counting_down_until_first_poll() {
        let timeout = Duration::from_millis(10);

        let svc = ServiceBuilder::new()
            .layer(ResponseBodyTimeoutLayer::new(timeout))
            .service_fn(|_: Request<Body>| async {
                let (mut tx, body) = Body::channel();
                tokio::spawn(async move {
                    tokio::time::sleep(timeout).await;
                    tx.send_data(Bytes::from("hi")).await.unwrap();
                });
                Ok::<_, tower::BoxError>(Response::new(body))
            });

        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        tokio::time::sleep(timeout).await;

        let bytes = hyper::body::to_bytes(res.into_body()).await.unwrap();
        assert_eq!(&bytes[..], b"hi");
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
