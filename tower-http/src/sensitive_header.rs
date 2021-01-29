use futures_util::ready;
use http::{header::HeaderName, Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

#[derive(Clone, Debug)]
pub struct SetSensitiveHeaderLayer {
    header: HeaderName,
}

impl SetSensitiveHeaderLayer {
    pub fn new(header: HeaderName) -> Self {
        Self { header }
    }
}

impl<S> Layer<S> for SetSensitiveHeaderLayer {
    type Service = SetSensitiveRequestHeader<SetSensitiveResponseHeader<S>>;

    fn layer(&self, inner: S) -> Self::Service {
        SetSensitiveRequestHeader::new(
            SetSensitiveResponseHeader::new(inner, self.header.clone()),
            self.header.clone(),
        )
    }
}

#[derive(Clone, Debug)]
pub struct SetSensitiveRequestHeaderLayer {
    header: HeaderName,
}

impl SetSensitiveRequestHeaderLayer {
    pub fn new(header: HeaderName) -> Self {
        Self { header }
    }
}

impl<S> Layer<S> for SetSensitiveRequestHeaderLayer {
    type Service = SetSensitiveRequestHeader<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SetSensitiveRequestHeader {
            inner,
            header: self.header.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SetSensitiveRequestHeader<S> {
    inner: S,
    header: HeaderName,
}

impl<S> SetSensitiveRequestHeader<S> {
    pub fn new(inner: S, header: HeaderName) -> Self {
        Self { inner, header }
    }
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for SetSensitiveRequestHeader<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        if let Some(value) = req.headers_mut().get_mut(&self.header) {
            value.set_sensitive(true);
        }

        self.inner.call(req)
    }
}

#[derive(Clone, Debug)]
pub struct SetSensitiveResponseHeaderLayer {
    header: HeaderName,
}

impl SetSensitiveResponseHeaderLayer {
    pub fn new(header: HeaderName) -> Self {
        Self { header }
    }
}

impl<S> Layer<S> for SetSensitiveResponseHeaderLayer {
    type Service = SetSensitiveResponseHeader<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SetSensitiveResponseHeader {
            inner,
            header: self.header.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SetSensitiveResponseHeader<S> {
    inner: S,
    header: HeaderName,
}

impl<S> SetSensitiveResponseHeader<S> {
    pub fn new(inner: S, header: HeaderName) -> Self {
        Self { inner, header }
    }
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for SetSensitiveResponseHeader<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        ResponseFuture {
            future: self.inner.call(req),
            header: Some(self.header.clone()),
        }
    }
}

#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F> {
    #[pin]
    future: F,
    header: Option<HeaderName>,
}

impl<F, ResBody, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        if let Some(value) = res.headers_mut().get_mut(this.header.take().unwrap()) {
            value.set_sensitive(true);
        }

        Poll::Ready(Ok(res))
    }
}
