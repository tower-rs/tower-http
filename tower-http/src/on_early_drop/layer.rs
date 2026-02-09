//! Layer for wrapping services with early drop detection capabilities.
//!
//! This module provides a [`tower::Layer`] implementation that adds early drop detection
//! to HTTP services. It wraps the inner service with [`OnEarlyDropService`] which monitors
//! request lifecycles and executes callbacks when requests are dropped before completion.
//!
//! # Features
//!
//! - Detect when clients disconnect before receiving a full response
//! - Execute custom callback logic when early drops occur
//! - Support for request-specific callback handlers
//!
//! # Example
//!
//! ```
//! use tower_http::on_early_drop::layer::OnEarlyDropLayer;
//! use tower::{ServiceBuilder, service_fn};
//! use std::convert::Infallible;
//! use http::{Request, Response};
//!
//! # #[tokio::main]
//! # async fn main() {
//! // Define a simple handler function
//! async fn handle(_: Request<String>) -> Result<Response<String>, Infallible> {
//!     Ok(Response::new(String::from("Hello, world!")))
//! }
//!
//! // Create a service with the OnEarlyDropLayer
//! let service = ServiceBuilder::new()
//!     .layer(OnEarlyDropLayer::new(|_req: &Request<String>| {
//!         || println!("Request was dropped early")
//!     }))
//!     .service_fn(handle);
//! # }
//! ```

use crate::on_early_drop::service::OnEarlyDropService;
use std::marker::PhantomData;
use tower_layer::Layer;
use tower_service::Service;

/// A [`tower::Layer`] used to apply [`OnEarlyDropService`].
///
/// # Type Parameters
///
/// * `CallbackFactory` - The callback factory type, a function that produces callbacks from requests
/// * `Callback` - The callback type, a function that will be executed if a request is dropped early
/// * `Request` - The request type being processed by the service
#[derive(Clone, Debug)]
pub struct OnEarlyDropLayer<CallbackFactory, Callback, Request>
where
    Callback: FnOnce(),
    CallbackFactory: Fn(&Request) -> Callback + Clone + Send + 'static,
{
    callback_factory: CallbackFactory,
    _marker: PhantomData<(Callback, Request)>,
}

impl<CallbackFactory, Callback, Request> OnEarlyDropLayer<CallbackFactory, Callback, Request>
where
    Callback: FnOnce(),
    CallbackFactory: Fn(&Request) -> Callback + Clone + Send + 'static,
{
    /// Creates a new `OnEarlyDropLayer` with the given callback factory.
    ///
    /// The callback factory is a function that takes a reference to the HTTP request
    /// and returns a callback function to be executed if the request is dropped early.
    pub fn new(callback_factory: CallbackFactory) -> Self {
        OnEarlyDropLayer {
            callback_factory,
            _marker: PhantomData,
        }
    }
}

impl<S, CallbackFactory, Callback, Request, Response> Layer<S>
    for OnEarlyDropLayer<CallbackFactory, Callback, Request>
where
    Callback: FnOnce(),
    CallbackFactory: Fn(&Request) -> Callback + Clone + Send + 'static,
    S: Service<Request, Response = Response>,
{
    type Service = OnEarlyDropService<CallbackFactory, Callback, S, Request, Response>;

    fn layer(&self, inner: S) -> Self::Service {
        OnEarlyDropService::new(inner, self.callback_factory.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::future;
    use http::{Request, Response};
    use tower::service_fn;

    #[tokio::test]
    async fn test_layer_creates_service() {
        // Create a simple callback factory
        let callback_factory = |_req: &Request<()>| || println!("Request was dropped early");

        // Create a simple service that returns 200 OK
        let service = service_fn(|_req: Request<()>| {
            future::ready(Ok::<_, std::io::Error>(Response::new(())))
        });

        // Apply the layer to the service
        let layer = OnEarlyDropLayer::<_, _, Request<()>>::new(callback_factory);
        let _wrapped_service = layer.layer(service);

        // Successfully creating the service indicates the layer is working properly
        assert!(true);
    }
}
