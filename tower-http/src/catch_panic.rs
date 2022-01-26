//! TODO(david): docs

use bytes::Bytes;
use futures_core::ready;
use futures_util::future::{CatchUnwind, FutureExt};
use http::{Request, Response, StatusCode};
use http_body::{combinators::UnsyncBoxBody, Body, Full};
use pin_project_lite::pin_project;
use std::{
    any::Any,
    borrow::Cow,
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies the [`CatchPanic`] middleware that catches panics and converts them into
/// `500 Internal Server` responses.
///
/// See the [module docs](self) for an example.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct CatchPanicLayer;

impl CatchPanicLayer {
    /// Create a new `CatchPanicLayer`.
    pub fn new() -> Self {
        CatchPanicLayer {}
    }
}

impl<S> Layer<S> for CatchPanicLayer {
    type Service = CatchPanic<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CatchPanic::new(inner)
    }
}

/// Middleware that catches panics and converts them into `500 Internal Server` responses.
///
/// See the [module docs](self) for an example.
#[derive(Debug, Clone, Copy)]
pub struct CatchPanic<S> {
    inner: S,
}

impl<S> CatchPanic<S> {
    /// Create a new `CatchPanic`.
    pub fn new(inner: S) -> Self {
        Self { inner }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `CatchPanic` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer() -> CatchPanicLayer {
        CatchPanicLayer::new()
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for CatchPanic<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body<Data = Bytes> + Send + 'static,
{
    type Response = Response<UnsyncBoxBody<Bytes, ResBody::Error>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        match std::panic::catch_unwind(AssertUnwindSafe(|| self.inner.call(req))) {
            Ok(future) => ResponseFuture {
                kind: Kind::Future {
                    future: AssertUnwindSafe(future).catch_unwind(),
                },
            },
            Err(panic_err) => ResponseFuture {
                kind: Kind::Panicked {
                    panic_err: Some(panic_err),
                },
            },
        }
    }
}

pin_project! {
    /// Response future for [`CatchPanic`].
    pub struct ResponseFuture<F> {
        #[pin]
        kind: Kind<F>,
    }
}

pin_project! {
    #[project = KindProj]
    enum Kind<F> {
        Panicked {
            panic_err: Option<Box<dyn Any + Send + 'static>>,
        },
        Future {
            #[pin]
            future: CatchUnwind<AssertUnwindSafe<F>>,
        }
    }
}

impl<F, ResBody, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Body<Data = Bytes> + Send + 'static,
{
    type Output = Result<Response<UnsyncBoxBody<Bytes, ResBody::Error>>, E>;

    #[allow(warnings)]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().kind.project() {
            KindProj::Panicked { panic_err } => Poll::Ready(Ok(panic_err_to_response(
                panic_err.take().expect("future polled after completion"),
            ))),
            KindProj::Future { future } => match ready!(future.poll(cx)) {
                Ok(Ok(res)) => Poll::Ready(Ok(res.map(|body| body.boxed_unsync()))),
                Ok(Err(svc_err)) => Poll::Ready(Err(svc_err)),
                Err(panic_err) => Poll::Ready(Ok(panic_err_to_response(panic_err))),
            },
        }
    }
}

fn panic_err_to_response<E>(
    err: Box<dyn Any + Send + 'static>,
) -> Response<UnsyncBoxBody<Bytes, E>> {
    let message: Cow<_> = if let Some(s) = err.downcast_ref::<String>() {
        s.to_owned().into()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        (*s).into()
    } else {
        "`CatchPanic` middleware was unable to obtain panic info".into()
    };
    let body = format!("Service panicked: {}", message);

    let mut res = Response::new(Full::from(body).map_err(|err| match err {}).boxed_unsync());
    *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;

    res
}

#[cfg(test)]
mod tests {
    #![allow(unreachable_code)]

    use super::*;
    use hyper::{Body, Response};
    use std::convert::Infallible;
    use tower::{ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn panic_before_returning_future() {
        let svc = ServiceBuilder::new()
            .layer(CatchPanicLayer::new())
            .service_fn(|_: Request<Body>| {
                panic!("service panic");
                async { Ok::<_, Infallible>(Response::new(Body::empty())) }
            });

        let req = Request::new(Body::empty());

        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = hyper::body::to_bytes(res).await.unwrap();
        assert_eq!(&body[..], b"Service panicked: service panic");
    }

    #[tokio::test]
    async fn panic_in_future() {
        let svc = ServiceBuilder::new()
            .layer(CatchPanicLayer::new())
            .service_fn(|_: Request<Body>| async {
                panic!("future panic");
                Ok::<_, Infallible>(Response::new(Body::empty()))
            });

        let req = Request::new(Body::empty());

        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = hyper::body::to_bytes(res).await.unwrap();
        assert_eq!(&body[..], b"Service panicked: future panic");
    }
}
