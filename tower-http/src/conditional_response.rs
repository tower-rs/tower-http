//!
//! Conditionally provide a response instead of calling the inner service.
//! 
//! This middleware provides a way to conditionally skip calling the inner service
//! if a response is already available for the request.
//! 
//! Probably the simplest visual for this is providing a cached response, though it
//! is unlikely that this middleware is suitable for a robust response cache interface
//! (or, more accurately, it's not the motivation for developing this so I haven't
//! looked into it adequately enough to provide a robust argument for it being so!).
//! 
//! The premise is simple - write a (non-async) function that assesses the current request
//! for the possibility of providing a response before invoking the inner service. Return
//! the "early" response if that is possible, otherwise return the request.
//! 
//! The differences between using this and returning an error from a pre-inner layer are.
//! 
//! 1. The response will still pass through any _post_-inner layer processing
//! 2. You aren't "hacking" the idea of an error when all you are trying to do is avoid
//!    calling the inner service when it isn't necessary.
//! 
//! Possible uses:
//! 
//! * A pre-inner layer produces a successful response before the inner service is called
//! * Caching (though see above - this could, however, be the layer that skips the inner
//!   call while a more robust pre-inner layer implements the actual caching)
//! * Mocking
//! * Debugging
//! * ...
//! 
//! # Example
//! 
//! ```rust
//! use http::{Request, Response};
//! use std::convert::Infallible;
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower::conditional_response::ConditionalResponseLayer;
//! 
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! 
//! //
//! // The responder function here decides whether to return an early response based
//! // upon the presence and value of a specific header.L
//! //
//! fn responder(request: Request<String>) -> ConditionalResponse<Request<String>,Response<String>> {
//!     match request.headers().get("x-so-we-skip") {
//!         Some(a) if a.to_str().unwrap() == "true" => ConditionalResponse::Response(Response::new("We skipped it".to_string())),
//!         _ => ConditionalResponse::Request(request)
//!     }
//! }
//!
//! async fn handle(_req: Request<String>) -> Result<Response<String>, Infallible> {
//!     // ...
//! 	Ok(Response::new("We ran it".to_string()))
//! }
//! 
//! let mut svc = ServiceBuilder::new()
//!     //
//!     // Directly wrap the target service with the conditional responder layer
//!     //
//!     .layer(ConditionalResponseLayer::new(responder))
//!     .service_fn(handle);
//! 
//! let request = Request::builder().header("x-so-we-skip","true").body("".to_string()).expect("Expected an empty body");

//! // Call the service.
//! let ready = futures::executor::block_on(svc.ready()).expect("Expected the service to be ready");
//! let response = futures::executor::block_on(ready.call(request)).expect("Expected the service to be successful");
//! assert_eq!(response.body(), "We skipped it");
//! #
//! # Ok(())
//! # }
//! ```

use http::{Request, Response};
use std::future::Future;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;
use pin_project::pin_project;

/// Layer that applies [`ConditionalResponseService`] which allows the caller to generate/return a response instead of calling the
/// inner service - useful for stacks where a default response (rather than an error) is determined by a pre-service
/// filter.
///
/// See the [module docs](crate::conditional_response) for more details.
#[derive(Clone, Debug)]
pub struct ConditionalResponseLayer<P> {
    responder: P
}

impl<P> ConditionalResponseLayer<P> 
{
    /// Create a new [`ConditionalResponseLayer`].
    pub fn new(responder:P) -> Self {
        Self { responder }
    }
}

impl<S,P> Layer<S> for ConditionalResponseLayer<P>
where
    P: Clone
{
    type Service = ConditionalResponseService<S,P>;

    fn layer(&self, inner: S) -> Self::Service {
        ConditionalResponseService::<S,P> {
            inner,
            responder: self.responder.clone(),
        }
    }
}

/// Middleware that conditionally provides a response to a request in lieu of calling the inner service.
///
/// See the [module docs](crate::conditional_response) for more details.
#[derive(Clone,Debug)]
pub struct ConditionalResponseService<S,P> {
    inner: S,
    responder: P,
}

impl<S,P> ConditionalResponseService<S,P> 
{
    /// Create a new [`ConditionalResponseService`] with the inner service and the "responder" function.
    pub fn new(inner: S, responder: P) -> Self {
        Self { inner, responder }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `ConditionalResponseService` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(responder: P) -> ConditionalResponseLayer<P> {
        ConditionalResponseLayer::<P>::new(responder)
    }
}

impl<ReqBody, ResBody, S,P> Service<Request<ReqBody>> for ConditionalResponseService<S,P>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    P: ConditionalResponder<Request<ReqBody>,Response<ResBody>>,
    ReqBody: Send + Sync + Clone
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future,S::Response>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        match self.responder.has_response(req) {
            ConditionalResponse::Request(t) => ResponseFuture::<S::Future,S::Response>::Future(self.inner.call(t)),
            ConditionalResponse::Response(r) => ResponseFuture::<S::Future,S::Response>::Response(Some(r))
        }
    }
}


/// Response future for [`ConditionalResponseService`].
/// 
/// We use an enum because the inner content may be a future or
/// or may be a direct response.
/// 
/// We use an option for the direct response so that ownership can be taken.
/// 
#[derive(Debug)]
#[pin_project(project = ResponseFutureProj)]
pub enum ResponseFuture<F,R> {
    Response(Option<R>),
    Future(#[pin] F),
}

impl<F, ResBody, E> Future for ResponseFuture<F,Response<ResBody>>
where
    F: Future<Output = Result<Response<ResBody>, E>>, 
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            ResponseFutureProj::Response(r) => Poll::Ready(Ok(r.take().unwrap())),
            ResponseFutureProj::Future(ref mut future) => future.as_mut().poll(cx)
        }
    }
}

/////////////////////////////////////////////////////////////////////////

///
/// The response required from the responder function.
/// 
pub enum ConditionalResponse<T,R>  {
    ///
    /// No response is available, so return the request
    /// 
    Request(T),
    ///
    /// A response is available, so return the response
    /// 
    Response(R)
}

///
/// Fn trait for functions that consume a request and return a
/// ConditionalResponse variant.
/// 

pub trait ConditionalResponder<T,R> {
    /// The type of requests returned by [`has_response`].
    ///
    /// This request is forwarded to the inner service if the responder
    /// succeeds.
    ///
    /// [`has_response`]: crate::filter::responder::has_response
    /// has_response whether the given request should be forwarded.
    ///
    /// If the future resolves with [`Ok`], the request is forwarded to the inner service.
    fn has_response(&mut self, request: T) -> ConditionalResponse<T,R>;
}

impl<F, T, R> ConditionalResponder<T,R> for F
where
    F: FnMut(T) -> ConditionalResponse<T,R>,
{
    fn has_response(&mut self, request: T) -> ConditionalResponse<T,R> {
        self(request)
    }
}

#[cfg(test)]
 mod tests {
    use super::*;

 	use http::{Request, Response};
 	use std::convert::Infallible;
 	use tower::{Service, ServiceExt, ServiceBuilder};
    use crate::builder::ServiceBuilderExt;
 	use crate::conditional_response::ConditionalResponseLayer;

    fn responder(request: Request<String>) -> ConditionalResponse<Request<String>,Response<String>> {
        match request.headers().get("x-so-we-skip") {
            Some(a) if a.to_str().unwrap() == "true" => ConditionalResponse::Response(Response::new("We skipped it".to_string())),
            _ => ConditionalResponse::Request(request)
        }
    }

    async fn handle(_req: Request<String>) -> Result<Response<String>, Infallible> {
		Ok(Response::new("We ran it".to_string()))
	}

    #[test]
    fn skip_test() {
		let mut svc = ServiceBuilder::new()
			.layer(ConditionalResponseLayer::new(responder))
			.service_fn(handle);

		let request = Request::builder().header("x-so-we-skip","true").body("".to_string()).expect("Expected an empty body");

		// Call the service.
		let ready = futures::executor::block_on(svc.ready()).expect("Expected the service to be ready");
		let response = futures::executor::block_on(ready.call(request)).expect("Expected the service to be successful");
		assert_eq!(response.body(), "We skipped it");
    }

    #[test]
    fn no_skip_test_header() {
		let mut svc = ServiceBuilder::new()
			.layer(ConditionalResponseLayer::new(responder))
			.service_fn(handle);

		let request = Request::builder().header("x-so-we-skip","not true").body("".to_string()).expect("Expected an empty body");

		// Call the service.
		let ready = futures::executor::block_on(svc.ready()).expect("Expected the service to be ready");
		let response = futures::executor::block_on(ready.call(request)).expect("Expected the service to be successful");
		assert_eq!(response.body(), "We ran it");
    }

    #[test]
    fn no_skip_test_no_header() {
		let mut svc = ServiceBuilder::new()
			.conditional_response(responder)
			.service_fn(handle);

		let request = Request::builder().body("".to_string()).expect("Expected an empty body");

		// Call the service.
		let ready = futures::executor::block_on(svc.ready()).expect("Expected the service to be ready");
		let response = futures::executor::block_on(ready.call(request)).expect("Expected the service to be successful");
		assert_eq!(response.body(), "We ran it");
    }
}
