use crate::common::*;

#[derive(Clone, Debug)]
pub struct PropagateHeaderLayer {
    header: HeaderName,
}

impl PropagateHeaderLayer {
    pub fn new(header: HeaderName) -> Self {
        Self { header }
    }
}

impl<S> Layer<S> for PropagateHeaderLayer {
    type Service = PropagateHeader<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PropagateHeader {
            inner,
            header: self.header.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PropagateHeader<S> {
    inner: S,
    header: HeaderName,
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for PropagateHeader<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let value = req.headers().get(&self.header).cloned();

        ResponseFuture {
            future: self.inner.call(req),
            header_and_value: Some(self.header.clone()).zip(value),
        }
    }
}

#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F> {
    #[pin]
    future: F,
    header_and_value: Option<(HeaderName, HeaderValue)>,
}

impl<F, ResBody, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        if let Some((header, value)) = this.header_and_value.take() {
            res.headers_mut().insert(header, value);
        }

        Poll::Ready(Ok(res))
    }
}
