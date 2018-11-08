use tower_service::Service;

use http::{Request, Response};
use futures::{Future, Poll};

use self::sealed::Sealed;

/// An HTTP service
///
/// This is not intended to be implemented directly. Instead, it is a trait
/// alias of sorts. Implements the `tower_service::Service` trait using
/// `http::Request` and `http::Response` types.
pub trait HttpService<RequestBody>: Sealed<RequestBody> {
    /// Response payload.
    type ResponseBody;

    /// Errors produced by the service.
    type Error;

    /// The future response value.
    type Future: Future<Item = Response<Self::ResponseBody>, Error = Self::Error>;

    /// Returns `Ready` when the service is able to process requests.
    fn poll_ready(&mut self) -> Poll<(), Self::Error>;

    /// Process the request and return the response asynchronously.
    fn call(&mut self, request: Request<RequestBody>) -> Self::Future;

    /// Wrap the HttpService so that it implements tower_service::Service
    /// directly.
    ///
    /// Since `HttpService` does not directly implement `Service`, if an
    /// `HttpService` instance needs to be used where a `T: Service` is
    /// required, it must be wrapped with a type that provides that
    /// implementation. `LiftService` does this.
    fn lift(self) -> LiftService<Self> where Self: Sized {
        LiftService { inner: self }
    }

    /// Same as `lift` but operates on an HttpService reference.
    fn lift_ref(&mut self) -> LiftServiceRef<Self> where Self: Sized {
        LiftServiceRef { inner: self }
    }
}

/// Wraps an `HttpService` instance, implementing `tower_service::Service`.
///
/// See [`lift`] function documentation for more details.
///
/// [`lift`]: #
pub struct LiftService<T> {
    inner: T,
}

/// Wraps an `HttpService` reference, implementing `tower_service::Service`.
///
/// See [`lift`] function documentation for more details.
///
/// [`lift`]: #
pub struct LiftServiceRef<'a, T: 'a> {
    inner: &'a mut T,
}

impl<T, B1, B2> HttpService<B1> for T
where
    T: Service<Request<B1>, Response = Response<B2>>,
{
    type ResponseBody = B2;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Service::poll_ready(self)
    }

    fn call(&mut self, request: Request<B1>) -> Self::Future {
        Service::call(self, request)
    }
}

impl<T, B1, B2> Sealed<B1> for T
where
    T: Service<Request<B1>, Response = Response<B2>>,
{}

impl<T, ReqBody> Service<Request<ReqBody>> for LiftService<T>
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

impl<'a, T, ReqBody> Service<Request<ReqBody>> for LiftServiceRef<'a, T>
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

mod sealed {
    pub trait Sealed<B> {}
}

