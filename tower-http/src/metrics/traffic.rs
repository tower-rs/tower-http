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
//!
//! # Example
//!
//! ```rust
//! use http::{Request, Response, HeaderMap};
//! use hyper::Body;
//! use tower::{ServiceBuilder, ServiceExt, Service};
//! use tower_http::{
//!     classify::{GrpcErrorsAsFailures, ClassifiedResponse},
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
//! impl MetricsSink<i32> for MyMetricsSink {
//!     type Data = ();
//!
//!     fn prepare<B>(&mut self, request: &Request<B>) -> Self::Data {
//!         // Prepare some data that will be passed to the other callbacks
//!     }
//!
//!     fn on_response<B>(
//!         &mut self,
//!         response: &Response<B>,
//!         classification: ClassifiedResponse<i32, ()>,
//!         data: &mut Self::Data,
//!     ) {
//!         // ...
//!     }
//!
//!     fn on_eos(
//!         self,
//!         trailers: Option<&HeaderMap>,
//!         classification: Result<(), i32>,
//!         data: Self::Data,
//!     ) {
//!         // ...
//!     }
//!
//!     fn on_failure(
//!         self,
//!         failed_at: FailedAt,
//!         failure_classification: i32,
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
//! let classifier = GrpcErrorsAsFailures::make_classifier::<hyper::Error>();
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

use crate::classify::{ClassifiedResponse, ClassifyEos, ClassifyResponse, MakeClassifier};
use futures_core::ready;
use http::{HeaderMap, Request, Response};
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};
use tower_layer::Layer;
use tower_service::Service;

// ===== layer =====

/// [`Layer`] for adding high level traffic metrics to a [`Service`].
///
/// See the [module docs](crate::metrics::traffic) for more details.
///
/// [`Layer`]: tower_layer::Layer
/// [`Service`]: tower_service::Service
#[derive(Debug, Clone)]
pub struct TrafficLayer<M, MetricsSink> {
    make_classifier: M,
    sink: MetricsSink,
}

impl<M, MetricsSink> TrafficLayer<M, MetricsSink> {
    /// Create a new `TrafficLayer`.
    pub fn new(make_classifier: M, sink: MetricsSink) -> Self {
        TrafficLayer {
            make_classifier,
            sink,
        }
    }
}

impl<S, M, MetricsSink> Layer<S> for TrafficLayer<M, MetricsSink>
where
    M: Clone,
    MetricsSink: Clone,
{
    type Service = Traffic<S, M, MetricsSink>;

    fn layer(&self, inner: S) -> Self::Service {
        Traffic {
            inner,
            make_classifier: self.make_classifier.clone(),
            sink: self.sink.clone(),
        }
    }
}

// ===== service =====

/// Middleware for adding high level traffic metrics to a [`Service`].
///
/// See the [module docs](crate::metrics::traffic) for more details.
///
/// [`Layer`]: tower_layer::Layer
/// [`Service`]: tower_service::Service
#[derive(Debug, Clone)]
pub struct Traffic<S, M, MetricsSink> {
    inner: S,
    make_classifier: M,
    sink: MetricsSink,
}

impl<S, M, MetricsSink> Traffic<S, M, MetricsSink> {
    /// Create a new `Traffic`.
    pub fn new(inner: S, make_classifier: M, sink: MetricsSink) -> Self {
        Self {
            inner,
            make_classifier,
            sink,
        }
    }

    /// Returns a new [`Layer`] that wraps services with a [`Traffic`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(make_classifier: M, sink: MetricsSink) -> TrafficLayer<M, MetricsSink> {
        TrafficLayer::new(make_classifier, sink)
    }

    define_inner_service_accessors!();
}

impl<S, M, ReqBody, ResBody, MetricsSinkT> Service<Request<ReqBody>> for Traffic<S, M, MetricsSinkT>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
    M: MakeClassifier<S::Error>,
    MetricsSinkT: MetricsSink<M::FailureClass> + Clone,
{
    type Response =
        Response<ResponseBody<ResBody, M::ClassifyEos, MetricsSinkT, MetricsSinkT::Data>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M::Classifier, MetricsSinkT, MetricsSinkT::Data>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let request_received_at = Instant::now();

        let sink_data = self.sink.prepare(&req);

        let classifier = self.make_classifier.make_classifier(&req);

        ResponseFuture {
            inner: self.inner.call(req),
            classifier: Some(classifier),
            request_received_at,
            sink: Some(self.sink.clone()),
            sink_data: Some(sink_data),
        }
    }
}

// ===== future =====

/// Response future for [`Traffic`].
#[pin_project]
pub struct ResponseFuture<F, C, MetricsSink, SinkData> {
    #[pin]
    inner: F,
    classifier: Option<C>,
    request_received_at: Instant,
    sink: Option<MetricsSink>,
    sink_data: Option<SinkData>,
}

impl<F, C, ResBody, E, MetricsSinkT, SinkData> Future
    for ResponseFuture<F, C, MetricsSinkT, SinkData>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Body,
    C: ClassifyResponse<E>,
    MetricsSinkT: MetricsSink<C::FailureClass, Data = SinkData>,
{
    type Output = Result<
        Response<ResponseBody<ResBody, C::ClassifyEos, MetricsSinkT, MetricsSinkT::Data>>,
        E,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = ready!(this.inner.poll(cx));

        let classifier = this.classifier.take().unwrap();

        match result {
            Ok(res) => {
                let classification = classifier.classify_response(&res);
                let mut sink: MetricsSinkT = this.sink.take().unwrap();
                let mut sink_data = this.sink_data.take().unwrap();

                match classification {
                    ClassifiedResponse::Ready(classification) => {
                        sink.on_response(
                            &res,
                            ClassifiedResponse::Ready(classification),
                            &mut sink_data,
                        );

                        let res = res.map(|body| ResponseBody {
                            inner: body,
                            parts: None,
                        });

                        Poll::Ready(Ok(res))
                    }
                    ClassifiedResponse::RequiresEos(classify_eos) => {
                        sink.on_response(&res, ClassifiedResponse::RequiresEos(()), &mut sink_data);

                        let res = res.map(|body| ResponseBody {
                            inner: body,
                            parts: Some((classify_eos, sink, sink_data)),
                        });

                        Poll::Ready(Ok(res))
                    }
                }
            }
            Err(err) => {
                let classification = classifier.classify_error(&err);
                this.sink.take().unwrap().on_failure(
                    FailedAt::Response,
                    classification,
                    this.sink_data.take().unwrap(),
                );

                Poll::Ready(Err(err))
            }
        }
    }
}

// ===== body =====

/// Response body for [`Traffic`].
#[pin_project]
pub struct ResponseBody<B, C, MetricsSink, SinkData> {
    #[pin]
    inner: B,
    parts: Option<(C, MetricsSink, SinkData)>,
}

impl<B, C, MetricsSinkT, SinkData> Body for ResponseBody<B, C, MetricsSinkT, SinkData>
where
    B: Body,
    C: ClassifyEos<B::Error>,
    MetricsSinkT: MetricsSink<C::FailureClass, Data = SinkData>,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();

        let result = ready!(this.inner.poll_data(cx));

        match result {
            None => Poll::Ready(None),
            Some(Ok(chunk)) => Poll::Ready(Some(Ok(chunk))),
            Some(Err(err)) => {
                if let Some((classify_eos, sink, sink_data)) = this.parts.take() {
                    let classification = classify_eos.classify_error(&err);
                    sink.on_failure(FailedAt::Body, classification, sink_data);
                }

                Poll::Ready(Some(Err(err)))
            }
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let this = self.project();

        let result = ready!(this.inner.poll_trailers(cx));

        match result {
            Ok(trailers) => {
                if let Some((classify_eos, sink, sink_data)) = this.parts.take() {
                    let trailers = trailers.as_ref();
                    let classification = classify_eos.classify_eos(trailers);
                    sink.on_eos(trailers, classification, sink_data);
                }

                Poll::Ready(Ok(trailers))
            }
            Err(err) => {
                if let Some((classify_eos, sink, sink_data)) = this.parts.take() {
                    let classification = classify_eos.classify_error(&err);
                    sink.on_failure(FailedAt::Body, classification, sink_data);
                }

                Poll::Ready(Err(err))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

// ===== callbacks =====

/// Trait that defines callbacks for [`Traffic`] to call.
pub trait MetricsSink<FailureClass>: Sized {
    /// Additional data required for creating metric events.
    ///
    /// This could for example be a struct that contains the request path and HTTP method so they
    /// can be included in events.
    type Data;

    /// Create an instance of `Self::Data` from the request.
    ///
    /// This method is called immediately after the request is received by [`Service::call`].
    ///
    /// The value returned here will be passed to the other methods in this trait.
    fn prepare<B>(&mut self, request: &Request<B>) -> Self::Data;

    /// Perform some action when a response has been generated.
    ///
    /// If the response is _not_ the start of a stream (as determined by the classifier passed to
    /// [`Traffic::new`] or [`TrafficLayer::new`]) then `classification` will be
    /// [`ClassifiedResponse::Ready`], otherwise it will be [`ClassifiedResponse::RequiresEos`]
    /// with `()` as the associated data.
    ///
    /// This method is called whenever [`Service::call`] of the inner service returns
    /// `Ok(response)`, regardless if the classifier determines if the response is classified as a
    /// success or a failure. If the response is classified as a failure then `classification` will
    /// be [`ClassifiedResponse::Ready`] containing `Err(failure_class)`, otherwise `Ok(())`.
    ///
    /// In the case where the response is succesfully generated but is classified to be a failure
    /// [`on_response`] is called and `on_failure` is _not_ called.
    ///
    /// A stream that ends succesfully will trigger two callbacks. [`on_response`] will be called
    /// once the response has been generated and the stream has started and [`on_eos`] will be
    /// called once the stream has ended.
    ///
    /// The default implementation does nothing and returns immediately.
    ///
    /// [`on_response`]: MetricsSink::on_response
    /// [`on_eos`]: MetricsSink::on_eos
    #[inline]
    #[allow(unused_variables)]
    fn on_response<B>(
        &mut self,
        response: &Response<B>,
        classification: ClassifiedResponse<FailureClass, ()>,
        data: &mut Self::Data,
    ) {
    }

    /// Perform some action when a stream has ended.
    ///
    /// This is called when [`Body::poll_trailers`] completes with `Ok(trailers)` regardless if
    /// the trailers are classified as a failure.
    ///
    /// If the trailers were classified as a success then `classification` will be `Ok(())`
    /// otherwise `Err(failure_class)`.
    ///
    /// The default implementation does nothing and returns immediately.
    #[inline]
    #[allow(unused_variables)]
    fn on_eos(
        self,
        trailers: Option<&HeaderMap>,
        classification: Result<(), FailureClass>,
        data: Self::Data,
    ) {
    }

    /// Perform some action when an error has been encountered.
    ///
    /// This method is only called in these scenarios:
    ///
    /// - The inner [`Service`]'s response future resolves to an error.
    /// - [`Body::poll_data`] or [`Body::poll_trailers`] returns an error.
    ///
    /// That means this method is _not_ called if a response is classified as a failure (then
    /// [`on_response`] is called) or an end-of-stream is classified as a failure (then [`on_eos`]
    /// is called).
    ///
    /// `failed_at` specifies where the error happened.
    ///
    /// The default implementation does nothing and returns immediately.
    ///
    /// [`on_response`]: MetricsSink::on_response
    /// [`on_eos`]: MetricsSink::on_eos
    #[inline]
    #[allow(unused_variables)]
    fn on_failure(
        self,
        failed_at: FailedAt,
        failure_classification: FailureClass,
        data: Self::Data,
    ) {
    }
}

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
    use crate::classify::ServerErrorsAsFailures;
    use http::{Method, Request, Response, StatusCode, Uri, Version};
    use hyper::Body;
    use metrics_lib as metrics;
    use tower::{Service, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn unary_request() {
        let mut svc = ServiceBuilder::new()
            .layer(TrafficLayer::new(
                ServerErrorsAsFailures::make_classifier::<hyper::Error>(),
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
    impl MetricsSink<StatusCode> for MySink {
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

        fn on_response<B>(
            &mut self,
            response: &Response<B>,
            classification: ClassifiedResponse<StatusCode, ()>,
            data: &mut SinkData,
        ) {
            let duration_ms = data.request_received_at.elapsed().as_millis() as f64;

            let is_error = if let ClassifiedResponse::Ready(class) = &classification {
                class.is_err()
            } else {
                false
            };

            let is_stream = matches!(classification, ClassifiedResponse::RequiresEos(_));

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

            if is_stream {
                data.stream_start = Some(Instant::now());
            }
        }

        fn on_eos(
            self,
            _trailers: Option<&HeaderMap>,
            classification: Result<(), StatusCode>,
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
            _failure_classification: StatusCode,
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
