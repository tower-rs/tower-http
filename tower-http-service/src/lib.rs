extern crate bytes;
extern crate futures;
extern crate http;
extern crate tokio_buf;
extern crate tower_service;

mod body;
mod sealed;
pub mod util;

pub use body::Body;

use http::{Request, Response};
use futures::{Future, Poll};
use tower_service::Service;

use sealed::Sealed;

/// An HTTP service
///
/// This is not intended to be implemented directly. Instead, it is a trait
/// alias of sorts. Implements the `tower_service::Service` trait using
/// `http::Request` and `http::Response` types.
pub trait HttpService<RequestBody>: Sealed<RequestBody> {
    /// Response payload.
    type ResponseBody: Body;

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
    fn into_service(self) -> util::IntoService<Self> where Self: Sized {
        util::IntoService::new(self)
    }

    /// Same as `lift` but operates on an HttpService reference.
    fn as_service(&mut self) -> util::AsService<Self> where Self: Sized {
        util::AsService::new(self)
    }
}

impl<T, B1, B2> HttpService<B1> for T
where
    T: Service<Request<B1>, Response = Response<B2>>,
    B2: Body,
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
    B2: Body,
{}
