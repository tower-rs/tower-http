use super::HttpService;
use http::{Request, Response};
use std::task::{Context, Poll};
use tower_service::Service;

/// Wraps an `HttpService` instance, implementing `tower_service::Service`.
///
/// See [`into_service`] function documentation for more details.
///
/// [`into_service`]: #
#[derive(Debug)]
pub struct IntoService<T> {
    inner: T,
}

impl<T> IntoService<T> {
    pub(crate) fn new(inner: T) -> IntoService<T> {
        IntoService { inner }
    }
}

impl<T, ReqBody> Service<Request<ReqBody>> for IntoService<T>
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
