//! Set a header on the response.

use futures_util::ready;
use http::{header::HeaderName, HeaderValue, Request, Response};
use pin_project::pin_project;
use std::{
    fmt,
    pin::Pin,
    task::{Context, Poll},
};
use std::{future::Future, marker::PhantomData};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`SetResponseHeader`] which adds a response header.
pub struct SetResponseHeaderLayer<M, Res> {
    header_name: HeaderName,
    make: M,
    override_existing: bool,
    _marker: PhantomData<fn() -> Res>,
}

impl<M, Res> fmt::Debug for SetResponseHeaderLayer<M, Res> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetResponseHeaderLayer")
            .field("header_name", &self.header_name)
            .field("override_existing", &self.override_existing)
            .field("make", &format_args!("{}", std::any::type_name::<M>()))
            .finish()
    }
}

impl<M, Res> SetResponseHeaderLayer<M, Res> {
    /// Create a new [`SetResponseHeaderLayer`].
    pub fn new(header_name: HeaderName, make: M) -> Self
    where
        M: MakeHeaderValue<Res>,
    {
        Self {
            make,
            header_name,
            override_existing: true,
            _marker: PhantomData,
        }
    }

    /// Should the header be overriden if the response already contains it?
    ///
    /// Defaults to `true`.
    pub fn override_existing(mut self, override_existing: bool) -> Self {
        self.override_existing = override_existing;
        self
    }
}

impl<Res, S, M> Layer<S> for SetResponseHeaderLayer<M, Res>
where
    M: MakeHeaderValue<Res> + Clone,
{
    type Service = SetResponseHeader<S, M>;

    fn layer(&self, inner: S) -> Self::Service {
        SetResponseHeader {
            inner,
            header_name: self.header_name.clone(),
            make: self.make.clone(),
            override_existing: self.override_existing,
        }
    }
}

impl<M, Res> Clone for SetResponseHeaderLayer<M, Res>
where
    M: Clone,
{
    fn clone(&self) -> Self {
        Self {
            make: self.make.clone(),
            header_name: self.header_name.clone(),
            override_existing: self.override_existing,
            _marker: PhantomData,
        }
    }
}

/// Middleware that sets a header on the response.
#[derive(Clone)]
pub struct SetResponseHeader<S, M> {
    inner: S,
    header_name: HeaderName,
    make: M,
    override_existing: bool,
}

impl<S, M> fmt::Debug for SetResponseHeader<S, M>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetResponseHeaderLayer")
            .field("inner", &self.inner)
            .field("header_name", &self.header_name)
            .field("override_existing", &self.override_existing)
            .field("make", &format_args!("{}", std::any::type_name::<M>()))
            .finish()
    }
}

impl<ReqBody, ResBody, S, M> Service<Request<ReqBody>> for SetResponseHeader<S, M>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    M: MakeHeaderValue<S::Response> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        ResponseFuture {
            future: self.inner.call(req),
            header_name: self.header_name.clone(),
            make: self.make.clone(),
            override_existing: self.override_existing,
        }
    }
}

/// Response future for [`SetResponseHeader`].
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F, M> {
    #[pin]
    future: F,
    header_name: HeaderName,
    make: M,
    override_existing: bool,
}

impl<F, ResBody, E, M> Future for ResponseFuture<F, M>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    M: MakeHeaderValue<Response<ResBody>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        let value = this.make.make_header_value(&res);

        if res.headers().contains_key(&*this.header_name) {
            if *this.override_existing {
                res.headers_mut().insert(this.header_name.clone(), value);
            }
        } else {
            res.headers_mut().insert(this.header_name.clone(), value);
        }

        Poll::Ready(Ok(res))
    }
}

/// Trait for producing header values from responses.
///
/// Used by [`SetResponseHeader`].
///
/// You shouldn't normally have to implement this trait since its implemented for closures with the
/// correct type.
///
/// It is also implemented directly for `HeaderValue` so if you just want to add a fixed value you
/// can suply one directly to [`SetResponseHeaderLayer`].
pub trait MakeHeaderValue<Res> {
    fn make_header_value(&mut self, response: &Res) -> HeaderValue;
}

impl<F, Res> MakeHeaderValue<Res> for F
where
    F: FnMut(&Res) -> HeaderValue,
{
    fn make_header_value(&mut self, response: &Res) -> HeaderValue {
        self(response)
    }
}

impl<Res> MakeHeaderValue<Res> for HeaderValue {
    fn make_header_value(&mut self, _response: &Res) -> HeaderValue {
        self.clone()
    }
}
