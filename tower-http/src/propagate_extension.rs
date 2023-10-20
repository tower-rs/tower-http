//! Propagate an extension from the request to the response.
//!
//! This middleware is intended to wrap a Request->Response service handler that is _unaware_ of the
//! extension. Consequently it _removes_ the extension from the request before forwarding the request, and then
//! inserts it into the response when the response is ready. As a usage example, if you have pre-service mappers 
//! that need to share state with post-service mappers, you can store the state in the Request extensions, 
//! and this middleware will ensure that it is available to the post service mappers via the Response extensions.
//!
//! # Example
//!
//! ```rust
//! use http::{Request, Response};
//! use std::convert::Infallible;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use tower_http::add_extension::AddExtensionLayer;
//! use tower_http::propagate_extension::PropagateExtensionLayer;
//! use hyper::Body;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
//!     // ...
//!     # Ok(Response::new(Body::empty()))
//! }
//!
//! //
//! // Note that while the state object must _implement_ Clone, it should never actually
//! // _be_ cloned due to the manner in which it is used within the middleware.
//! //
//! #[derive(Clone)]
//! struct MyState {
//!     state_message: String
//! };
//!
//! let my_state = MyState { state_message: "propagated state".to_string() };
//! 
//! let mut svc = ServiceBuilder::new()
//!     .layer(AddExtensionLayer::new(my_state)) // any other way of adding the extension to the request is OK too
//!     .layer(PropagateExtensionLayer::<MyState>::new())
//!     .service_fn(handle);
//!
//! // Call the service.
//! let request = Request::builder()
//!     .body(Body::empty())?;
//!
//! let response = svc.ready().await?.call(request).await?;
//!
//! assert_eq!(response.extensions().get::<MyState>().unwrap().state_message, "propagated state");
//! #
//! # Ok(())
//! # }
//! ```

use futures_util::ready;
use http::{Request, Response};
use pin_project_lite::pin_project;
use std::future::Future;
use std::{
    pin::Pin,
    task::{Context, Poll},
	marker::PhantomData,
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`PropagateExtension`] which propagates an extension from the request to the response.
///
/// This middleware is intended to wrap a Request->Response service handler that is _unaware_ of the
/// extension. Consequently it _removes_ the extension from the request before forwarding the request, and then
/// inserts it into the response when the response is ready. As a usage example, if you have pre-service mappers 
/// that need to share state with post-service mappers, you can store the state in the Request extensions, 
/// and this middleware will ensure that it is available to the post service mappers via the Response extensions.
///
/// See the [module docs](crate::propagate_extension) for more details.
#[derive(Clone, Debug)]
pub struct PropagateExtensionLayer<X> {
	_phantom: PhantomData<X>
}

impl<X> PropagateExtensionLayer<X> {
    /// Create a new [`PropagateExtensionLayer`].
    pub fn new() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl<S,X> Layer<S> for PropagateExtensionLayer<X> {
    type Service = PropagateExtension<S,X>;

    fn layer(&self, inner: S) -> Self::Service {
        PropagateExtension::<S,X> {
            inner,
			_phantom: PhantomData
        }
    }
}

/// Middleware that propagates extensions from requests to responses.
///
/// If the extension is present on the request it'll be removed from the request and
/// inserted into the response.
///
/// See the [module docs](crate::propagate_extension) for more details.
#[derive(Clone,Debug)]
pub struct PropagateExtension<S,X> {
    inner: S,
	_phantom: PhantomData<X>
}

impl<S,X> PropagateExtension<S,X> {
    /// Create a new [`PropagateExtension`] that propagates the given extension type.
    pub fn new(inner: S) -> Self {
        Self { inner, _phantom: PhantomData }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `PropagateExtension` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer() -> PropagateExtensionLayer<X> {
        PropagateExtensionLayer::<X>::new()
    }
}

impl<ReqBody, ResBody, S, X> Service<Request<ReqBody>> for PropagateExtension<S,X>
where
	X: Sync + Send + 'static,
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future,X>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let extension: Option<X> = req.extensions_mut().remove();

        ResponseFuture {
            future: self.inner.call(req),
            extension,
        }
    }
}

pin_project! {
    /// Response future for [`PropagateExtension`].
    #[derive(Debug)]
    pub struct ResponseFuture<F,X> {
        #[pin]
        future: F,
        extension: Option<X>,
    }
}

impl<F, ResBody, E, X> Future for ResponseFuture<F,X>
where
	X: Sync + Send + 'static,
    F: Future<Output = Result<Response<ResBody>, E>>, 
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        if let Some(extension) = this.extension.take() {
            res.extensions_mut().insert(extension);
        }

        Poll::Ready(Ok(res))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

	use http::{Request, Response};
	use std::convert::Infallible;
	use tower::{Service, ServiceExt, ServiceBuilder};
	use crate::add_extension::AddExtensionLayer;
	//use tower_http::propagate_extension::PropagateExtensionLayer;
	use hyper::Body;

	async fn handle(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
		Ok(Response::new(Body::empty()))
	}

    #[derive(Clone)]
	struct MyState {
		state_message: String
	}

	#[test]
	fn basic_test() {

		let my_state = MyState { state_message: "propagated state".to_string() };

		let mut svc = ServiceBuilder::new()
			.layer(AddExtensionLayer::new(my_state)) // any other way of adding the extension to the request is OK too
			.layer(PropagateExtensionLayer::<MyState>::new())
			.service_fn(handle);

		let request = Request::builder().body(Body::empty()).expect("Expected an empty body");

		// Call the service.
		let ready = futures::executor::block_on(svc.ready()).expect("Expected the service to be ready");
		let response = futures::executor::block_on(ready.call(request)).expect("Expected the service to be successful");
		assert_eq!(response.extensions().get::<MyState>().unwrap().state_message, "propagated state");
	}
}
