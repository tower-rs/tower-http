use HttpService;
use futures::Poll;
use http::{Request, Response};
use tower_service::Service;

/// Wraps an `HttpService` reference, implementing `tower_service::Service`.
///
/// See [`as_service`] function documentation for more details.
///
/// [`as_service`]: #
pub struct AsService<'a, T: 'a> {
    inner: &'a mut T,
}

impl<'a, T> AsService<'a, T> {
    pub(crate) fn new(inner: &'a mut T) -> AsService<'a, T> {
        AsService { inner }
    }
}

impl<'a, T, ReqBody> Service<Request<ReqBody>> for AsService<'a, T>
where
    T: HttpService<ReqBody>
{
    type Response = Response<T::ResponseBody>;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.inner.poll_ready()
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        self.inner.call(request)
    }
}
