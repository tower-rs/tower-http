//! Types and utilities for working with `HttpService`

mod as_service;
mod into_service;

pub use self::as_service::AsService;
pub use self::into_service::IntoService;

use http::{Request, Response};
use http_body::Body;
use std::future::Future;
use std::task::{Context, Poll};
use tower_service::Service;

/// An HTTP service
///
/// This is not intended to be implemented directly. Instead, it is a trait
/// alias of sorts. Implements the [`tower_service::Service`] trait using
/// [`http::Request`] and [`http::Response`] types.
pub trait HttpService<RequestBody>: sealed::Sealed<RequestBody> {
    /// Response payload.
    type ResponseBody: Body;

    /// Errors produced by the service.
    type Error;

    /// The future response value.
    type Future: Future<Output = Result<Response<Self::ResponseBody>, Self::Error>>;

    /// Returns `Ready` when the service is able to process requests.
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;

    /// Process the request and return the response asynchronously.
    fn call(&mut self, request: Request<RequestBody>) -> Self::Future;

    /// Wrap the `HttpService` so that it implements `tower_service::Service`
    /// directly.
    ///
    /// Since `HttpService` does not directly implement `Service`, if an
    /// `HttpService` instance needs to be used where a `T: Service` is
    /// required, it must be wrapped with a type that provides that
    /// implementation. `IntoService` does this.
    fn into_service(self) -> IntoService<Self>
    where
        Self: Sized,
    {
        IntoService::new(self)
    }

    /// Same as `into_service` but operates on an `HttpService` reference.
    fn as_service(&mut self) -> AsService<'_, Self>
    where
        Self: Sized,
    {
        AsService::new(self)
    }
}

impl<T, ReqBody, ResBody> HttpService<ReqBody> for T
where
    T: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
{
    type ResponseBody = ResBody;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Service::poll_ready(self, cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        Service::call(self, request)
    }
}

impl<T, ReqBody, ResBody> sealed::Sealed<ReqBody> for T
where
    T: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
{
}

mod sealed {
    pub trait Sealed<B> {}
}
