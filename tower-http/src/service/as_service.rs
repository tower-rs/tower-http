use super::HttpService;
use http::{Request, Response};
use std::task::{Context, Poll};
use tower_service::Service;

/// Wraps an `HttpService` reference, implementing `tower_service::Service`.
///
/// See [`as_service`] method documentation for more details.
///
/// [`as_service`]: HttpService::as_service
#[derive(Debug)]
pub struct AsService<'a, T> {
    inner: &'a mut T,
}

impl<'a, T> AsService<'a, T> {
    pub(crate) fn new(inner: &'a mut T) -> AsService<'a, T> {
        AsService { inner }
    }
}

impl<'a, T, ReqBody> Service<Request<ReqBody>> for AsService<'a, T>
where
    T: HttpService<ReqBody>,
{
    type Response = Response<T::ResponseBody>;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        self.inner.call(request)
    }
}
