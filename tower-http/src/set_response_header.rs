use crate::common::*;
use std::fmt;

pub struct SetResponseHeaderLayer<M, Res> {
    make: M,
    override_existing: bool,
    _marker: PhantomData<fn() -> Res>,
}

impl<M, Res> fmt::Debug for SetResponseHeaderLayer<M, Res> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetResponseHeaderLayer")
            .field("override_existing", &self.override_existing)
            .field("make", &format_args!("{}", std::any::type_name::<M>()))
            .finish()
    }
}

impl<M, Res> SetResponseHeaderLayer<M, Res> {
    pub fn new(make: M) -> Self
    where
        M: MakeHeaderPair<Res>,
    {
        Self {
            make,
            override_existing: true,
            _marker: PhantomData,
        }
    }

    pub fn override_existing(mut self, override_existing: bool) -> Self {
        self.override_existing = override_existing;
        self
    }
}

impl<Res, S, M> Layer<S> for SetResponseHeaderLayer<M, Res>
where
    M: MakeHeaderPair<Res> + Clone,
{
    type Service = SetResponseHeader<S, M>;

    fn layer(&self, inner: S) -> Self::Service {
        SetResponseHeader {
            inner,
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
            override_existing: self.override_existing,
            _marker: PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct SetResponseHeader<S, M> {
    inner: S,
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
            .field("override_existing", &self.override_existing)
            .field("make", &format_args!("{}", std::any::type_name::<M>()))
            .finish()
    }
}

impl<ReqBody, ResBody, S, M> Service<Request<ReqBody>> for SetResponseHeader<S, M>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    M: MakeHeaderPair<S::Response> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        ResponseFuture {
            future: self.inner.call(req),
            make: self.make.clone(),
            override_existing: self.override_existing,
        }
    }
}

#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F, M> {
    #[pin]
    future: F,
    make: M,
    override_existing: bool,
}

impl<F, ResBody, E, M> Future for ResponseFuture<F, M>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    M: MakeHeaderPair<Response<ResBody>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        let (header, value) = this.make.make_header_pair(&res);

        if res.headers().contains_key(&header) {
            if *this.override_existing {
                res.headers_mut().insert(header, value);
            }
        } else {
            res.headers_mut().insert(header, value);
        }

        Poll::Ready(Ok(res))
    }
}

pub trait MakeHeaderPair<Res> {
    fn make_header_pair(&mut self, response: &Res) -> (HeaderName, HeaderValue);
}

impl<F, Res> MakeHeaderPair<Res> for F
where
    F: FnMut(&Res) -> (HeaderName, HeaderValue),
{
    fn make_header_pair(&mut self, response: &Res) -> (HeaderName, HeaderValue) {
        self(response)
    }
}
