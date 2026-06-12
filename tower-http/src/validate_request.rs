//! Middleware that validates requests.
//!
//! # Example
//!
//! Validation of the `Accept` header can be made by using [`ValidateRequestHeaderLayer::accept()`]:
//!
//! ```
//! use tower_http::validate_request::ValidateRequestHeaderLayer;
//! use http::{Request, Response, StatusCode, header::ACCEPT};
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn, BoxError};
//!
//! async fn handle(request: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, BoxError> {
//!     Ok(Response::new(Full::default()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! let mut service = ServiceBuilder::new()
//!     // Require the `Accept` header to be `application/json`, `*/*` or `application/*`
//!     .layer(ValidateRequestHeaderLayer::accept("application/json"))
//!     .service_fn(handle);
//!
//! // Requests with the correct value are allowed through
//! let request = Request::builder()
//!     .header(ACCEPT, "application/json")
//!     .body(Full::default())
//!     .unwrap();
//!
//! let response = service
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(StatusCode::OK, response.status());
//!
//! // Requests with an invalid value get a `406 Not Acceptable` response
//! let request = Request::builder()
//!     .header(ACCEPT, "text/strings")
//!     .body(Full::default())
//!     .unwrap();
//!
//! let response = service
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(StatusCode::NOT_ACCEPTABLE, response.status());
//! # Ok(())
//! # }
//! ```
//!
//! Validation of a custom header can be made by using [`ValidateRequestHeaderLayer::has_header_value()`]:
//!
//! ```
//! use tower_http::validate_request::ValidateRequestHeaderLayer;
//! use http::{Request, Response, StatusCode};
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn, BoxError};
//!
//! async fn handle(request: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, BoxError> {
//!     Ok(Response::new(Full::default()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! let mut service = ServiceBuilder::new()
//!     // Require a `X-Custom-Header` header to have the value `random-value-1234567890` or reject with a `403 Forbidden` response
//!     .layer(ValidateRequestHeaderLayer::has_header_value(
//!         "x-custom-header",
//!         "random-value-1234567890",
//!     ).expect("invalid validate header"))
//!     .service_fn(handle);
//!
//! // Requests with the correct value are allowed through
//! let request = Request::builder()
//!     .header("x-custom-header", "random-value-1234567890")
//!     .body(Full::default())
//!     .unwrap();
//!
//! let response = service
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(StatusCode::OK, response.status());
//!
//! // Requests with an invalid value get a `403 Forbidden` response
//! let request = Request::builder()
//!     .header("x-custom-header", "wrong-value")
//!     .body(Full::default())
//!     .unwrap();
//!
//! let response = service
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(StatusCode::FORBIDDEN, response.status());
//! # Ok(())
//! # }
//! ```
//!
//! To require only that a header is present, use [`ValidateRequestHeaderLayer::custom()`]:
//!
//! ```
//! use tower_http::validate_request::ValidateRequestHeaderLayer;
//! use http::{Request, Response, StatusCode};
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use tower::{ServiceBuilder, service_fn, BoxError};
//!
//! async fn handle(request: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, BoxError> {
//!     Ok(Response::new(Full::default()))
//! }
//!
//! # fn main() {
//! let service = ServiceBuilder::new()
//!     .layer(ValidateRequestHeaderLayer::custom(|req: &mut Request<Full<Bytes>>| {
//!         if req.headers().contains_key("x-custom-header") {
//!             Ok(())
//!         } else {
//!             let mut res = Response::new(Full::<Bytes>::default());
//!             *res.status_mut() = StatusCode::FORBIDDEN;
//!             Err(res)
//!         }
//!     }))
//!     .service_fn(handle);
//! # }
//! ```
//!
//! To serve a custom response when validation fails, also use [`ValidateRequestHeaderLayer::custom()`]:
//!
//! ```
//! use tower_http::validate_request::ValidateRequestHeaderLayer;
//! use http::{Request, Response, StatusCode};
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use tower::{ServiceBuilder, service_fn, BoxError};
//!
//! async fn handle(request: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, BoxError> {
//!     Ok(Response::new(Full::default()))
//! }
//!
//! # fn main() {
//! let service = ServiceBuilder::new()
//!     .layer(ValidateRequestHeaderLayer::custom(|req: &mut Request<Full<Bytes>>| {
//!         match req.headers().get("x-custom-header").map(|v| v.as_bytes()) {
//!             Some(b"random-value-1234567890") => Ok(()),
//!             _ => Err(Response::builder()
//!                 .status(StatusCode::FORBIDDEN)
//!                 .body(Full::<Bytes>::default())
//!                 .unwrap()),
//!         }
//!     }))
//!     .service_fn(handle);
//! # }
//! ```
//!
//! Custom validation can be made by implementing [`ValidateRequest`]:
//!
//! ```
//! use tower_http::validate_request::{ValidateRequestHeaderLayer, ValidateRequest};
//! use http::{Request, Response, StatusCode, header::ACCEPT};
//! use http_body_util::Full;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn, BoxError};
//! use bytes::Bytes;
//!
//! #[derive(Clone, Copy)]
//! pub struct MyHeader { /* ...  */ }
//!
//! impl<B> ValidateRequest<B> for MyHeader {
//!     type ResponseBody = Full<Bytes>;
//!
//!     fn validate(
//!         &mut self,
//!         request: &mut Request<B>,
//!     ) -> Result<(), Response<Self::ResponseBody>> {
//!         // validate the request...
//!         # unimplemented!()
//!     }
//! }
//!
//! async fn handle(request: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, BoxError> {
//!     Ok(Response::new(Full::default()))
//! }
//!
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), BoxError> {
//! let service = ServiceBuilder::new()
//!     // Validate requests using `MyHeader`
//!     .layer(ValidateRequestHeaderLayer::custom(MyHeader { /* ... */ }))
//!     .service_fn(handle);
//! # Ok(())
//! # }
//! ```
//!
//! [`Accept`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Accept

use http::header::InvalidHeaderName;
use http::{header, header::HeaderName, Request, Response, StatusCode};
use mime::{Mime, MimeIter};
use pin_project_lite::pin_project;
use std::{
    fmt,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`ValidateRequestHeader`] which validates all requests.
///
/// See the [module docs](crate::validate_request) for an example.
#[derive(Debug, Clone)]
pub struct ValidateRequestHeaderLayer<T> {
    validate: T,
}

impl<ResBody> ValidateRequestHeaderLayer<AcceptHeader<ResBody>> {
    /// Validate requests have the required Accept header.
    ///
    /// The `Accept` header is required to be `*/*`, `type/*` or `type/subtype`,
    /// as configured.
    ///
    /// # Panics
    ///
    /// Panics if `header_value` is not in the form: `type/subtype`, such as `application/json`
    /// See `AcceptHeader::new` for when this method panics.
    ///
    /// # Example
    ///
    /// ```
    /// use http_body_util::Full;
    /// use bytes::Bytes;
    /// use tower_http::validate_request::{AcceptHeader, ValidateRequestHeaderLayer};
    ///
    /// let layer = ValidateRequestHeaderLayer::<AcceptHeader<Full<Bytes>>>::accept("application/json");
    /// ```
    ///
    /// [`Accept`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Accept
    pub fn accept(value: &str) -> Self
    where
        ResBody: Default,
    {
        Self::custom(AcceptHeader::new(value))
    }
}

impl<ResBody> ValidateRequestHeaderLayer<RequiredHeaderValue<ResBody>> {
    /// Validate requests have a required header with a specific value.
    ///
    /// Rejects with `403 Forbidden` if the header is missing or does not have the expected value.
    /// Header values that are not valid UTF-8 are treated as non-matching.
    ///
    /// If the request contains multiple values for the header, only the first occurrence is
    /// checked.
    ///
    /// # Errors
    ///
    /// Returns an error if `expected_header_name` is not a valid HTTP header name per RFC 7230
    /// (non-empty, at most 32,768 bytes, containing only valid token characters).
    ///
    /// # Example
    ///
    /// ```
    /// use http::{Request, Response, StatusCode};
    /// use http_body_util::Full;
    /// use bytes::Bytes;
    /// use tower::{Service, ServiceBuilder, ServiceExt, service_fn};
    /// use tower_http::validate_request::ValidateRequestHeaderLayer;
    ///
    /// async fn handle(request: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    ///     Ok(Response::new(request.into_body()))
    /// }
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// let mut service = ServiceBuilder::new()
    ///     .layer(ValidateRequestHeaderLayer::has_header_value(
    ///         "x-custom-header",
    ///         "random-value-1234567890",
    ///     ).expect("invalid validate header"))
    ///     .service_fn(handle);
    ///
    /// let request = Request::builder()
    ///     .header("x-custom-header", "random-value-1234567890")
    ///     .body(Full::default())
    ///     .unwrap();
    ///
    /// let response = service.ready().await.unwrap().call(request).await.unwrap();
    /// assert_eq!(response.status(), StatusCode::OK);
    /// # }
    /// ```
    pub fn has_header_value(
        expected_header_name: &str,
        expected_header_value: &str,
    ) -> Result<Self, InvalidHeaderName>
    where
        ResBody: Default,
    {
        Ok(Self::custom(RequiredHeaderValue::new(
            expected_header_name.parse::<HeaderName>()?,
            expected_header_value,
        )))
    }
}

impl<T> ValidateRequestHeaderLayer<T> {
    /// Validate requests using a custom method.
    pub fn custom(validate: T) -> ValidateRequestHeaderLayer<T> {
        Self { validate }
    }
}

impl<S, T> Layer<S> for ValidateRequestHeaderLayer<T>
where
    T: Clone,
{
    type Service = ValidateRequestHeader<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        ValidateRequestHeader::new(inner, self.validate.clone())
    }
}

/// Middleware that validates requests.
///
/// See the [module docs](crate::validate_request) for an example.
#[derive(Clone, Debug)]
pub struct ValidateRequestHeader<S, T> {
    inner: S,
    validate: T,
}

impl<S, T> ValidateRequestHeader<S, T> {
    fn new(inner: S, validate: T) -> Self {
        Self::custom(inner, validate)
    }

    define_inner_service_accessors!();
}

impl<S, ResBody> ValidateRequestHeader<S, AcceptHeader<ResBody>> {
    /// Validate requests have the required Accept header.
    ///
    /// The `Accept` header is required to be `*/*`, `type/*` or `type/subtype`,
    /// as configured.
    ///
    /// # Panics
    ///
    /// See `AcceptHeader::new` for when this method panics.
    pub fn accept(inner: S, value: &str) -> Self
    where
        ResBody: Default,
    {
        Self::custom(inner, AcceptHeader::new(value))
    }
}

impl<S, T> ValidateRequestHeader<S, T> {
    /// Validate requests using a custom method.
    pub fn custom(inner: S, validate: T) -> ValidateRequestHeader<S, T> {
        Self { inner, validate }
    }
}

impl<ReqBody, ResBody, S, V> Service<Request<ReqBody>> for ValidateRequestHeader<S, V>
where
    V: ValidateRequest<ReqBody, ResponseBody = ResBody>,
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        match self.validate.validate(&mut req) {
            Ok(_) => ResponseFuture::future(self.inner.call(req)),
            Err(res) => ResponseFuture::invalid_header_value(res),
        }
    }
}

pin_project! {
    /// Response future for [`ValidateRequestHeader`].
    pub struct ResponseFuture<F, B> {
        #[pin]
        kind: Kind<F, B>,
    }
}

impl<F, B> ResponseFuture<F, B> {
    fn future(future: F) -> Self {
        Self {
            kind: Kind::Future { future },
        }
    }

    fn invalid_header_value(res: Response<B>) -> Self {
        Self {
            kind: Kind::Error {
                response: Some(res),
            },
        }
    }
}

pin_project! {
    #[project = KindProj]
    enum Kind<F, B> {
        Future {
            #[pin]
            future: F,
        },
        Error {
            response: Option<Response<B>>,
        },
    }
}

impl<F, B, E> Future for ResponseFuture<F, B>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().kind.project() {
            KindProj::Future { future } => future.poll(cx),
            KindProj::Error { response } => {
                let response = response.take().expect("future polled after completion");
                Poll::Ready(Ok(response))
            }
        }
    }
}

/// Trait for validating requests.
pub trait ValidateRequest<B> {
    /// The body type used for responses to unvalidated requests.
    type ResponseBody;

    /// Validate the request.
    ///
    /// If `Ok(())` is returned then the request is allowed through, otherwise not.
    fn validate(&mut self, request: &mut Request<B>) -> Result<(), Response<Self::ResponseBody>>;
}

impl<B, F, ResBody> ValidateRequest<B> for F
where
    F: FnMut(&mut Request<B>) -> Result<(), Response<ResBody>>,
{
    type ResponseBody = ResBody;

    fn validate(&mut self, request: &mut Request<B>) -> Result<(), Response<Self::ResponseBody>> {
        self(request)
    }
}

/// Type that performs validation of the Accept header.
pub struct AcceptHeader<ResBody> {
    header_value: Arc<Mime>,
    _ty: PhantomData<fn() -> ResBody>,
}

impl<ResBody> AcceptHeader<ResBody> {
    /// Create a new `AcceptHeader`.
    ///
    /// # Panics
    ///
    /// Panics if `header_value` is not in the form: `type/subtype`, such as `application/json`
    fn new(header_value: &str) -> Self
    where
        ResBody: Default,
    {
        Self {
            header_value: Arc::new(
                header_value
                    .parse::<Mime>()
                    .expect("value is not a valid header value"),
            ),
            _ty: PhantomData,
        }
    }
}

impl<ResBody> Clone for AcceptHeader<ResBody> {
    fn clone(&self) -> Self {
        Self {
            header_value: self.header_value.clone(),
            _ty: PhantomData,
        }
    }
}

impl<ResBody> fmt::Debug for AcceptHeader<ResBody> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcceptHeader")
            .field("header_value", &self.header_value)
            .finish()
    }
}

impl<B, ResBody> ValidateRequest<B> for AcceptHeader<ResBody>
where
    ResBody: Default,
{
    type ResponseBody = ResBody;

    fn validate(&mut self, req: &mut Request<B>) -> Result<(), Response<Self::ResponseBody>> {
        if !req.headers().contains_key(header::ACCEPT) {
            return Ok(());
        }
        if req
            .headers()
            .get_all(header::ACCEPT)
            .into_iter()
            .filter_map(|header| header.to_str().ok())
            .any(|h| {
                MimeIter::new(h)
                    .map(|mim| {
                        if let Ok(mim) = mim {
                            let typ = self.header_value.type_();
                            let subtype = self.header_value.subtype();
                            match (mim.type_(), mim.subtype()) {
                                (t, s) if t == typ && s == subtype => true,
                                (t, mime::STAR) if t == typ => true,
                                (mime::STAR, mime::STAR) => true,
                                _ => false,
                            }
                        } else {
                            false
                        }
                    })
                    .reduce(|acc, mim| acc || mim)
                    .unwrap_or(false)
            })
        {
            return Ok(());
        }
        let mut res = Response::new(ResBody::default());
        *res.status_mut() = StatusCode::NOT_ACCEPTABLE;
        Err(res)
    }
}

/// Type that rejects requests if a header is not present or does not have an expected value.
pub struct RequiredHeaderValue<ResBody> {
    expected_header_name: HeaderName,
    expected_header_value: Arc<str>,
    _ty: PhantomData<fn() -> ResBody>,
}

impl<ResBody> RequiredHeaderValue<ResBody> {
    fn new(expected_header_name: HeaderName, expected_header_value: &str) -> Self
    where
        ResBody: Default,
    {
        Self {
            expected_header_name,
            expected_header_value: expected_header_value.into(),
            _ty: PhantomData,
        }
    }
}

impl<ResBody> Clone for RequiredHeaderValue<ResBody> {
    fn clone(&self) -> Self {
        Self {
            expected_header_name: self.expected_header_name.clone(),
            expected_header_value: self.expected_header_value.clone(),
            _ty: PhantomData,
        }
    }
}

impl<ResBody> fmt::Debug for RequiredHeaderValue<ResBody> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequiredHeaderValue")
            .field("expected_header_name", &self.expected_header_name)
            .field("expected_header_value", &self.expected_header_value)
            .finish()
    }
}

impl<B, ResBody> ValidateRequest<B> for RequiredHeaderValue<ResBody>
where
    ResBody: Default,
{
    type ResponseBody = ResBody;

    fn validate(&mut self, req: &mut Request<B>) -> Result<(), Response<Self::ResponseBody>> {
        let request_header_value = req
            .headers()
            .get(&self.expected_header_name)
            .and_then(|v| v.to_str().ok());

        if request_header_value != Some(&*self.expected_header_value) {
            let mut res = Response::new(ResBody::default());
            *res.status_mut() = StatusCode::FORBIDDEN;
            return Err(res);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use crate::test_helpers::Body;
    use http::header;
    use tower::{BoxError, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn valid_accept_header() {
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("application/json"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, "application/json")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn valid_accept_header_accept_all_json() {
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("application/json"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, "application/*")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn valid_accept_header_accept_all() {
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("application/json"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, "*/*")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn invalid_accept_header() {
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("application/json"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, "invalid")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_ACCEPTABLE);
    }
    #[tokio::test]
    async fn not_accepted_accept_header_subtype() {
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("application/json"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, "application/strings")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_ACCEPTABLE);
    }

    #[tokio::test]
    async fn not_accepted_accept_header() {
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("application/json"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, "text/strings")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_ACCEPTABLE);
    }

    #[tokio::test]
    async fn accepted_multiple_header_value() {
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("application/json"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, "text/strings")
            .header(header::ACCEPT, "invalid, application/json")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn accepted_inner_header_value() {
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("application/json"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, "text/strings, invalid, application/json")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn accepted_header_with_quotes_valid() {
        let value = "foo/bar; parisien=\"baguette, text/html, jambon, fromage\", application/*";
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("application/xml"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, value)
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn accepted_header_with_quotes_invalid() {
        let value = "foo/bar; parisien=\"baguette, text/html, jambon, fromage\"";
        let mut service = ServiceBuilder::new()
            .layer(ValidateRequestHeaderLayer::accept("text/html"))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::ACCEPT, value)
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_ACCEPTABLE);
    }

    #[tokio::test]
    async fn valid_custom_header() {
        let mut service = ServiceBuilder::new()
            .layer(
                ValidateRequestHeaderLayer::has_header_value(
                    "x-custom-header",
                    "random-value-1234567890",
                )
                .expect("invalid validate header"),
            )
            .service_fn(echo);

        let request = Request::get("/")
            .header("x-custom-header", "random-value-1234567890")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn invalid_custom_header() {
        let mut service = ServiceBuilder::new()
            .layer(
                ValidateRequestHeaderLayer::has_header_value(
                    "x-custom-header",
                    "random-value-1234567890",
                )
                .expect("invalid validate header"),
            )
            .service_fn(echo);

        let request = Request::get("/")
            .header("x-custom-header", "wrong-value")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn missing_custom_header() {
        let mut service = ServiceBuilder::new()
            .layer(
                ValidateRequestHeaderLayer::has_header_value(
                    "x-custom-header",
                    "random-value-1234567890",
                )
                .expect("invalid validate header"),
            )
            .service_fn(echo);

        let request = Request::get("/").body(Body::empty()).unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn custom_header_multiple_values_uses_first() {
        let mut service = ServiceBuilder::new()
            .layer(
                ValidateRequestHeaderLayer::has_header_value("x-custom-header", "correct-value")
                    .expect("invalid validate header"),
            )
            .service_fn(echo);

        // First value matches: should pass
        let request = Request::get("/")
            .header("x-custom-header", "correct-value")
            .header("x-custom-header", "other-value")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // First value does not match: should reject even if second matches
        let request = Request::get("/")
            .header("x-custom-header", "wrong-value")
            .header("x-custom-header", "correct-value")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn invalid_header_name_returns_error() {
        let result = ValidateRequestHeaderLayer::<RequiredHeaderValue<Body>>::has_header_value(
            "invalid header name with spaces",
            "value",
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn custom_header_non_utf8_value_rejects() {
        let mut service = ServiceBuilder::new()
            .layer(
                ValidateRequestHeaderLayer::has_header_value("x-custom-header", "expected-value")
                    .expect("invalid validate header"),
            )
            .service_fn(echo);

        let request = Request::get("/")
            .header("x-custom-header", b"\xff\xfe".as_slice())
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn custom_header_name_is_case_insensitive() {
        let mut service = ServiceBuilder::new()
            .layer(
                ValidateRequestHeaderLayer::has_header_value("x-custom-header", "my-value")
                    .expect("invalid validate header"),
            )
            .service_fn(echo);

        let request = Request::get("/")
            .header("X-Custom-Header", "my-value")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn custom_header_value_is_case_sensitive() {
        let mut service = ServiceBuilder::new()
            .layer(
                ValidateRequestHeaderLayer::has_header_value("x-custom-header", "My-Value")
                    .expect("invalid validate header"),
            )
            .service_fn(echo);

        let request = Request::get("/")
            .header("x-custom-header", "my-value")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    async fn echo(req: Request<Body>) -> Result<Response<Body>, BoxError> {
        Ok(Response::new(req.into_body()))
    }
}
