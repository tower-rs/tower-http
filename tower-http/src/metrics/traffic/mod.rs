//! Middleware for adding high level traffic metrics to a [`Service`].
//!
//! The primary focus of this middleware is to enable adding request per second/minute and error
//! rate metrics.
//!
//! The middleware doesn't do any kind of aggregate but instead uses a [`MetricsSink`] which
//! contains callbacks that the middleware will call. These methods can call into your actual
//! metrics system as appropriate. See [`MetricsSink`] for details on when each callback is called.
//!
//! Additionally it uses a [classifier] to determine if responses are success or failure.
//!
//! [classifier]: crate::classify
//! [`Service`]: tower::Service
//!
//! # Example
//!
//! ```rust
//! use http::{Request, Response, HeaderMap};
//! use hyper::Body;
//! use tower::{ServiceBuilder, ServiceExt, Service};
//! use tower_http::{
//!     classify::{GrpcErrorsAsFailures, GrpcFailureClass, ClassifiedResponse},
//!     metrics::traffic::{TrafficLayer, MetricsSink, FailedAt},
//! };
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, hyper::Error> {
//!     Ok(Response::new(Body::from("foo")))
//! }
//!
//! #[derive(Clone)]
//! struct MyMetricsSink;
//!
//! impl MetricsSink<GrpcFailureClass> for MyMetricsSink {
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
//!     // on the `MetricsSink` trait.
//!     .layer(TrafficLayer::new(classifier, MyMetricsSink))
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
mod future;
mod layer;
mod metrics_sink;
mod service;

pub use self::{
    body::ResponseBody, future::ResponseFuture, layer::TrafficLayer, metrics_sink::MetricsSink,
    service::Traffic,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::{ClassifiedResponse, ServerErrorsAsFailures, ServerErrorsFailureClass};
    use http::{HeaderMap, Method, Request, Response, Uri, Version};
    use hyper::Body;
    use metrics_lib as metrics;
    use std::time::Instant;
    use tower::{Service, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn unary_request() {
        let mut svc = ServiceBuilder::new()
            .layer(TrafficLayer::new(
                ServerErrorsAsFailures::make_classifier(),
                MySink,
            ))
            .service_fn(echo);

        svc.ready()
            .await
            .unwrap()
            .call(Request::new(Body::from("foobar")))
            .await
            .unwrap();
    }

    async fn echo(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        Ok(Response::new(req.into_body()))
    }

    #[derive(Clone)]
    struct MySink;

    struct SinkData {
        uri: Uri,
        method: Method,
        version: Version,
        request_received_at: Instant,
        stream_start: Option<Instant>,
    }

    // How one might write a sink that uses the `metrics` crate as the backend.
    impl MetricsSink<ServerErrorsFailureClass> for MySink {
        type Data = SinkData;

        fn prepare<B>(&mut self, request: &Request<B>) -> Self::Data {
            SinkData {
                uri: request.uri().clone(),
                method: request.method().clone(),
                version: request.version(),
                request_received_at: Instant::now(),
                stream_start: None,
            }
        }

        #[allow(warnings)]
        fn on_response<B>(
            &mut self,
            response: &Response<B>,
            classification: ClassifiedResponse<ServerErrorsFailureClass, ()>,
            data: &mut SinkData,
        ) {
            let is_stream;
            let is_error;

            match classification {
                ClassifiedResponse::Ready(class) => {
                    is_error = class.is_err();
                    is_stream = false;
                }
                ClassifiedResponse::RequiresEos(_) => {
                    is_error = false;
                    is_stream = true;
                }
            }

            let duration_ms = data.request_received_at.elapsed().as_millis() as f64;

            metrics::increment_counter!(
                "http_requests_total",
                "path" => data.uri.path().to_string(),
                "method" => data.method.to_string(),
                "code" => response.status().to_string(),
                "version" => format!("{:?}", data.version),
                "is_error" => is_error.then(|| "true").unwrap_or("false"),
                "is_stream" => is_stream.then(|| "true").unwrap_or("false"),
            );

            metrics::histogram!(
                "request_duration_milliseconds",
                duration_ms,
                "path" => data.uri.path().to_string(),
                "method" => data.method.to_string(),
                "code" => response.status().to_string(),
                "version" => format!("{:?}", data.version),
                "is_error" => is_error.then(|| "true").unwrap_or("false"),
            );
        }

        fn on_eos(
            self,
            _trailers: Option<&HeaderMap>,
            classification: Result<(), ServerErrorsFailureClass>,
            data: SinkData,
        ) {
            let stream_duration = data.stream_start.unwrap().elapsed().as_millis() as f64;

            let is_error = classification.is_err();

            metrics::histogram!(
                "stream_duration_milliseconds",
                stream_duration,
                "path" => data.uri.path().to_string(),
                "method" => data.method.to_string(),
                "version" => format!("{:?}", data.version),
                "is_error" => is_error.then(|| "true").unwrap_or("false"),
            );
        }

        fn on_failure(
            self,
            failed_at: FailedAt,
            _failure_classification: ServerErrorsFailureClass,
            data: SinkData,
        ) {
            match failed_at {
                FailedAt::Response => {
                    metrics::increment_counter!(
                        "request_error",
                        "path" => data.uri.path().to_string(),
                        "method" => data.method.to_string(),
                        "version" => format!("{:?}", data.version),
                    );
                }
                FailedAt::Body => {
                    metrics::increment_counter!(
                        "body_error",
                        "path" => data.uri.path().to_string(),
                        "method" => data.method.to_string(),
                        "version" => format!("{:?}", data.version),
                    );
                }
                FailedAt::Trailers => {
                    metrics::increment_counter!(
                        "trailers_error",
                        "path" => data.uri.path().to_string(),
                        "method" => data.method.to_string(),
                        "version" => format!("{:?}", data.version),
                    );
                }
            }
        }
    }
}
