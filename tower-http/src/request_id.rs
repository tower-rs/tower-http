// NOTE: when uuid 1.0 is shipped we can include a `MakeRequestId` that uses that
// See https://github.com/uuid-rs/uuid/issues/113

use http::{
    header::{HeaderName, HeaderValue},
    Request, Response,
};
use pin_project_lite::pin_project;
use std::task::{Context, Poll};
use std::{future::Future, pin::Pin};
use tower_layer::Layer;
use tower_service::Service;

pub(crate) const X_REQUEST_ID: &str = "x-request-id";

pub trait MakeRequestId {
    fn make_request_id<B>(&mut self, request: &Request<B>) -> RequestId;
}

#[derive(Debug, Clone)]
pub struct RequestId(HeaderValue);

impl RequestId {
    pub fn new(header_value: HeaderValue) -> Self {
        Self(header_value)
    }

    pub fn header_value(&self) -> &HeaderValue {
        &self.0
    }

    pub fn into_header_value(self) -> HeaderValue {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct SetRequestIdLayer<M> {
    header_name: HeaderName,
    make_request_id: M,
}

impl<M> SetRequestIdLayer<M> {
    pub fn new(header_name: HeaderName, make_request_id: M) -> Self {
        SetRequestIdLayer {
            header_name,
            make_request_id,
        }
    }

    pub fn x_request_id(make_request_id: M) -> Self {
        SetRequestIdLayer {
            header_name: HeaderName::from_static(X_REQUEST_ID),
            make_request_id,
        }
    }
}

impl<S, M> Layer<S> for SetRequestIdLayer<M>
where
    M: Clone,
{
    type Service = SetRequestId<S, M>;

    fn layer(&self, inner: S) -> Self::Service {
        SetRequestId::new(
            inner,
            self.header_name.clone(),
            self.make_request_id.clone(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct SetRequestId<S, M> {
    inner: S,
    header_name: HeaderName,
    make_request_id: M,
}

impl<S, M> SetRequestId<S, M> {
    pub fn new(inner: S, header_name: HeaderName, make_request_id: M) -> Self {
        Self {
            inner,
            header_name,
            make_request_id,
        }
    }

    pub fn x_request_id(inner: S, make_request_id: M) -> Self {
        Self::new(
            inner,
            HeaderName::from_static(X_REQUEST_ID),
            make_request_id,
        )
    }

    define_inner_service_accessors!();

    pub fn layer(header_name: HeaderName, make_request_id: M) -> SetRequestIdLayer<M> {
        SetRequestIdLayer::new(header_name, make_request_id)
    }
}

impl<S, M, ReqBody, ResBody> Service<Request<ReqBody>> for SetRequestId<S, M>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    M: MakeRequestId,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        if let Some(request_id) = req.headers().get(&self.header_name).cloned() {
            if req.extensions().get::<RequestId>().is_none() {
                req.extensions_mut().insert(RequestId::new(request_id));
            }
        } else {
            let request_id = self.make_request_id.make_request_id(&req);
            req.extensions_mut().insert(request_id.clone());
            req.headers_mut()
                .insert(self.header_name.clone(), request_id.0);
        }

        self.inner.call(req)
    }
}

#[derive(Debug, Clone)]
pub struct PropagateRequestIdLayer {
    header_name: HeaderName,
}

impl PropagateRequestIdLayer {
    pub fn new(header_name: HeaderName) -> Self {
        PropagateRequestIdLayer { header_name }
    }

    pub fn x_request_id() -> Self {
        Self::new(HeaderName::from_static(X_REQUEST_ID))
    }
}

impl<S> Layer<S> for PropagateRequestIdLayer {
    type Service = PropagateRequestId<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PropagateRequestId::new(inner, self.header_name.clone())
    }
}

#[derive(Debug, Clone)]
pub struct PropagateRequestId<S> {
    inner: S,
    header_name: HeaderName,
}

impl<S> PropagateRequestId<S> {
    pub fn new(inner: S, header_name: HeaderName) -> Self {
        Self { inner, header_name }
    }

    pub fn x_request_id(inner: S) -> Self {
        Self::new(inner, HeaderName::from_static(X_REQUEST_ID))
    }

    define_inner_service_accessors!();

    pub fn layer(header_name: HeaderName) -> PropagateRequestIdLayer {
        PropagateRequestIdLayer::new(header_name)
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for PropagateRequestId<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = PropagateRequestIdResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let request_id = req
            .headers()
            .get(&self.header_name)
            .cloned()
            .map(RequestId::new);

        PropagateRequestIdResponseFuture {
            inner: self.inner.call(req),
            header_name: self.header_name.clone(),
            request_id,
        }
    }
}

pin_project! {
    /// Response future for [`PropagateRequestId`].
    pub struct PropagateRequestIdResponseFuture<F> {
        #[pin]
        inner: F,
        header_name: HeaderName,
        request_id: Option<RequestId>,
    }
}

impl<F, B, E> Future for PropagateRequestIdResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut response = futures_core::ready!(this.inner.poll(cx))?;

        if let Some(current_id) = response.headers().get(&*this.header_name) {
            if response.extensions().get::<RequestId>().is_none() {
                let current_id = current_id.clone();
                response.extensions_mut().insert(current_id);
            }
        } else if let Some(request_id) = this.request_id.take() {
            response
                .headers_mut()
                .insert(this.header_name.clone(), request_id.0.clone());
            response.extensions_mut().insert(request_id);
        }

        Poll::Ready(Ok(response))
    }
}

#[cfg(test)]
mod tests {
    use crate::ServiceBuilderExt as _;
    use hyper::{Body, Response};
    use std::{
        convert::Infallible,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    };
    use tower::{ServiceBuilder, ServiceExt};

    #[allow(unused_imports)]
    use super::*;

    #[tokio::test]
    async fn test_something() {
        #[derive(Clone, Default)]
        struct Counter(Arc<AtomicU64>);

        impl MakeRequestId for Counter {
            fn make_request_id<B>(&mut self, _request: &Request<B>) -> RequestId {
                let id = HeaderValue::from_str(&self.0.fetch_add(1, Ordering::SeqCst).to_string())
                    .unwrap();
                RequestId::new(id)
            }
        }

        let svc = ServiceBuilder::new()
            .set_x_request_id(Counter::default())
            .propagate_x_request_id()
            .service_fn(handler);

        // header on response
        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "0");

        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "1");

        // doesn't override if header is already there
        let req = Request::builder()
            .header("x-request-id", "foo")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "foo");

        // extension propagated
        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.extensions().get::<RequestId>().unwrap().0, "2");
    }

    async fn handler(_: Request<Body>) -> Result<Response<Body>, Infallible> {
        Ok(Response::new(Body::empty()))
    }
}
