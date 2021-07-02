//! Middleware for adding callbacks to the life cycle of request.
//!
//! # Example
//!
//! ```rust
//! use http::{Request, Response, HeaderMap};
//! use hyper::Body;
//! use tower::{ServiceBuilder, ServiceExt, Service};
//! use tower_http::{
//!     classify::{GrpcErrorsAsFailures, GrpcFailureClass, ClassifiedResponse},
//!     life_cycle_hooks::{LifeCycleHooksLayer, Callbacks, FailedAt},
//! };
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, hyper::Error> {
//!     Ok(Response::new(Body::from("foo")))
//! }
//!
//! #[derive(Clone)]
//! struct MyCallbacks;
//!
//! impl Callbacks<GrpcFailureClass> for MyCallbacks {
//!     type Data = ();
//!
//!     fn prepare<B>(&mut self, request: &Request<B>) -> Self::Data {
//!         // Prepare some data that will be passed to the other callbacks
//!     }
//!
//!     fn on_response<B>(
//!         &mut self,
//!         response: &Response<B>,
//!         classification: ClassifiedResponse<GrpcFailureClass, ()>,
//!         data: &mut Self::Data,
//!     ) {
//!         // ...
//!     }
//!
//!     fn on_eos(
//!         self,
//!         trailers: Option<&HeaderMap>,
//!         classification: Result<(), GrpcFailureClass>,
//!         data: Self::Data,
//!     ) {
//!         // ...
//!     }
//!
//!     fn on_failure(
//!         self,
//!         failed_at: FailedAt,
//!         failure_classification: GrpcFailureClass,
//!         data: Self::Data,
//!     ) {
//!         // ...
//!     }
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Classifier that supports gRPC. Use `ServerErrorsAsFailures` for regular
//! // non-streaming HTTP requests or build your own by implementing `MakeClassifier`.
//! let classifier = GrpcErrorsAsFailures::make_classifier();
//!
//! let mut service = ServiceBuilder::new()
//!     // Add the middleware to our service. It will automatically call the callbacks
//!     // on the `Callbacks` trait.
//!     .layer(LifeCycleHooksLayer::new(classifier, MyCallbacks))
//!     .service_fn(handle);
//!
//! // Send a request.
//! let request = Request::new(Body::from("foo"));
//!
//! let response = service
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! // Consume the response body.
//! let body = response.into_body();
//! let bytes = hyper::body::to_bytes(body).await.unwrap();
//! # Ok(())
//! # }
//! ```

mod body;
mod callbacks;
mod future;
mod layer;
mod service;

pub use self::{
    body::ResponseBody, callbacks::Callbacks, future::ResponseFuture, layer::LifeCycleHooksLayer,
    service::LifeCycleHooks,
};

/// Enum used to specify where an error was encountered.
#[derive(Debug)]
pub enum FailedAt {
    /// Generating the response failed.
    Response,
    /// Generating the response body failed.
    Body,
    /// Generating the response trailers failed.
    Trailers,
}
