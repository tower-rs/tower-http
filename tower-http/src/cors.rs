//! Middleware which adds headers for [CORS][mdn].
//!
//! # Example
//!
//! ```
//! use http::{Request, Response, Method, header};
//! use hyper::Body;
//! use tower::{ServiceBuilder, ServiceExt, Service};
//! use tower_http::cors::{CorsLayer, Any};
//! use std::convert::Infallible;
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, Infallible> {
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let cors = CorsLayer::new()
//!     // allow `GET` and `POST` when accessing the resource
//!     .allow_methods(vec![Method::GET, Method::POST])
//!     // allow requests from any origin
//!     .allow_origin(Any);
//!
//! let mut service = ServiceBuilder::new()
//!     .layer(cors)
//!     .service_fn(handle);
//!
//! let request = Request::builder()
//!     .header(header::ORIGIN, "https://example.com")
//!     .body(Body::empty())
//!     .unwrap();
//!
//! let response = service
//!     .ready()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(
//!     response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
//!     "*",
//! );
//! # Ok(())
//! # }
//! ```
//!
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS

use futures_core::ready;
use http::{
    header::{self, HeaderName, HeaderValue},
    request::Parts,
    Method, Request, Response, StatusCode,
};
use pin_project::pin_project;
use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`Cors`] which adds headers for [CORS][mdn].
///
/// See the [module docs](crate::cors) for an example.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS
#[derive(Debug, Clone)]
pub struct CorsLayer {
    allow_credentials: Option<HeaderValue>,
    allow_headers: HeaderValue,
    allow_methods: HeaderValue,
    allow_origin: AnyOr<Origin>,
    expose_headers: Option<HeaderValue>,
    max_age: Option<HeaderValue>,
}

const DEFAULT_METHODS: &str = "GET, POST, OPTIONS";
const WILDCARD: &str = "*";

impl CorsLayer {
    /// Create a new `CorsLayer`.
    pub fn new() -> Self {
        Self {
            allow_credentials: None,
            allow_headers: WILDCARD.parse().unwrap(),
            allow_methods: DEFAULT_METHODS.parse().unwrap(),
            allow_origin: AnyOr(AnyOrInner::Any),
            expose_headers: None,
            max_age: None,
        }
    }

    /// Set the value of the [`Access-Control-Allow-Credentials`][mdn] header.
    ///
    /// ```
    /// use tower_http::cors::CorsLayer;
    ///
    /// let layer = CorsLayer::new().allow_credentials(true);
    /// ```
    ///
    /// By default the header will not be set.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Credentials
    pub fn allow_credentials(mut self, allow_credentials: bool) -> Self {
        self.allow_credentials = Some(allow_credentials.to_string().parse().unwrap());
        self
    }

    /// Set the value of the [`Access-Control-Allow-Headers`][mdn] header.
    ///
    /// ```
    /// use tower_http::cors::CorsLayer;
    /// use http::header::HeaderValue;
    ///
    /// let layer = CorsLayer::new().allow_headers(vec![
    ///     "*".parse().unwrap(),
    /// ]);
    /// ```
    ///
    /// By default the header will be set to `*`.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Headers
    pub fn allow_headers<I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderValue>,
    {
        self.allow_headers = separated_by_commas(
            headers
                .into_iter()
                .map(|value| value.to_str().unwrap().to_string()),
        );
        self
    }

    /// Set the value of the [`Access-Control-Max-Age`][mdn] header.
    ///
    /// ```
    /// use tower_http::cors::CorsLayer;
    /// use std::time::Duration;
    ///
    /// let layer = CorsLayer::new().max_age(Duration::from_secs(60) * 10);
    /// ```
    ///
    /// By default the header will not be set will disables caching and will require a preflight
    /// call for all requests.
    ///
    /// Note that each browser has a maximum internal value that takes precedence when the
    /// Access-Control-Max-Age is greater. For more details see [mdn].
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Max-Age
    pub fn max_age(mut self, max_age: Duration) -> Self {
        self.max_age = Some(max_age.as_secs().into());
        self
    }

    /// Set the value of the [`Access-Control-Allow-Methods`][mdn] header.
    ///
    /// ```
    /// use tower_http::cors::CorsLayer;
    /// use http::Method;
    ///
    /// let layer = CorsLayer::new().allow_methods(vec![Method::GET, Method::POST]);
    /// ```
    ///
    /// All methods can be allowed with
    ///
    /// ```
    /// use tower_http::cors::{CorsLayer, Any};
    ///
    /// let layer = CorsLayer::new().allow_methods(Any);
    /// ```
    ///
    /// By default the header will be set to `GET, POST, OPTIONS`.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Methods
    pub fn allow_methods<T>(mut self, methods: T) -> Self
    where
        T: Into<AnyOr<Vec<Method>>>,
    {
        self.allow_methods = match methods.into().0 {
            AnyOrInner::Any => "*".parse().unwrap(),
            AnyOrInner::Value(methods) => separated_by_commas(methods),
        };
        self
    }

    /// Set the value of the [`Access-Control-Allow-Origin`][mdn] header.
    ///
    /// ```
    /// use tower_http::cors::{CorsLayer, Origin};
    ///
    /// let layer = CorsLayer::new().allow_origin(Origin::exact(
    ///     "http://example.com".parse().unwrap(),
    /// ));
    /// ```
    ///
    /// All origins can be allowed with
    ///
    /// ```
    /// use tower_http::cors::{CorsLayer, Any};
    ///
    /// let layer = CorsLayer::new().allow_origin(Any);
    /// ```
    ///
    /// You can also use a closure
    ///
    /// ```
    /// use tower_http::cors::{CorsLayer, Any};
    /// use http::{HeaderValue, request::Parts};
    ///
    /// let layer = CorsLayer::new().allow_origin(|origin: &HeaderValue, _request_head: &Parts| {
    ///     origin.as_bytes().ends_with(b".rust-lang.org")
    /// });
    /// ```
    ///
    /// By default the header will be set to `*`.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin
    pub fn allow_origin<T>(mut self, origin: T) -> Self
    where
        T: Into<AnyOr<Origin>>,
    {
        self.allow_origin = origin.into();
        self
    }

    /// Set the value of the [`Access-Control-Expose-Headers`][mdn] header.
    ///
    /// ```
    /// use tower_http::cors::CorsLayer;
    /// use http::header::CONTENT_ENCODING;
    ///
    /// let layer = CorsLayer::new().expose_headers(vec![CONTENT_ENCODING]);
    /// ```
    ///
    /// All headers can be allowed with
    ///
    /// ```
    /// use tower_http::cors::{CorsLayer, Any};
    ///
    /// let layer = CorsLayer::new().expose_headers(Any);
    /// ```
    ///
    /// By default the header will not be set.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Expose-Headers
    pub fn expose_headers<I>(mut self, headers: I) -> Self
    where
        I: Into<AnyOr<Vec<HeaderName>>>,
    {
        self.expose_headers = Some(match headers.into().0 {
            AnyOrInner::Any => "*".parse().unwrap(),
            AnyOrInner::Value(headers) => separated_by_commas(headers),
        });
        self
    }
}

/// Represents a wildcard value (`*`) used with some CORS headers such as
/// [`CorsLayer::allow_methods`].
#[derive(Debug, Clone, Copy)]
pub struct Any;

/// Used to make methods like [`CorsLayer::allow_methods`] more convenient to call.
///
/// You shouldn't have to use this type directly.
#[derive(Debug, Clone, Copy)]
pub struct AnyOr<T>(AnyOrInner<T>);

#[derive(Debug, Clone, Copy)]
enum AnyOrInner<T> {
    Any,
    Value(T),
}

impl From<Origin> for AnyOr<Origin> {
    fn from(origin: Origin) -> Self {
        AnyOr(AnyOrInner::Value(origin))
    }
}

impl<F> From<F> for AnyOr<Origin>
where
    F: Fn(&HeaderValue, &Parts) -> bool + Send + Sync + 'static,
{
    fn from(f: F) -> Self {
        let inner = OriginInner::Closure(Arc::new(f));
        AnyOr(AnyOrInner::Value(Origin(inner)))
    }
}

impl<T> From<Any> for AnyOr<T> {
    fn from(_: Any) -> Self {
        AnyOr(AnyOrInner::Any)
    }
}

impl<I> From<I> for AnyOr<Vec<Method>>
where
    I: IntoIterator<Item = Method>,
{
    fn from(methods: I) -> Self {
        AnyOr(AnyOrInner::Value(methods.into_iter().collect()))
    }
}

impl<I> From<I> for AnyOr<Vec<HeaderName>>
where
    I: IntoIterator<Item = HeaderName>,
{
    fn from(headers: I) -> Self {
        AnyOr(AnyOrInner::Value(headers.into_iter().collect()))
    }
}

fn separated_by_commas<I>(into_iter: I) -> HeaderValue
where
    I: IntoIterator,
    I::Item: ToString,
{
    let methods = into_iter
        .into_iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(",");
    HeaderValue::from_str(&methods).unwrap()
}

impl Default for CorsLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for CorsLayer {
    type Service = Cors<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Cors {
            inner,
            layer: self.clone(),
        }
    }
}

/// Middleware which adds headers for [CORS][mdn].
///
/// See the [module docs](crate::cors) for an example.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS
#[derive(Debug, Clone)]
pub struct Cors<S> {
    inner: S,
    layer: CorsLayer,
}

impl<S> Cors<S> {
    /// Create a new `Cors`.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            layer: CorsLayer::new(),
        }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a [`Cors`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer() -> CorsLayer {
        CorsLayer::new()
    }

    /// Set the value of the [`Access-Control-Allow-Credentials`][mdn] header.
    ///
    /// See [`CorsLayer::allow_credentials`] for more details.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Credentials
    pub fn allow_credentials(self, allow_credentials: bool) -> Self {
        self.map_layer(|layer| layer.allow_credentials(allow_credentials))
    }

    /// Set the value of the [`Access-Control-Allow-Headers`][mdn] header.
    ///
    /// See [`CorsLayer::allow_headers`] for more details.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Headers
    pub fn allow_headers<I>(self, headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderValue>,
    {
        self.map_layer(|layer| layer.allow_headers(headers))
    }

    /// Set the value of the [`Access-Control-Max-Age`][mdn] header.
    ///
    /// See [`CorsLayer::max_age`] for more details.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Max-Age
    pub fn max_age(self, max_age: Duration) -> Self {
        self.map_layer(|layer| layer.max_age(max_age))
    }

    /// Set the value of the [`Access-Control-Allow-Methods`][mdn] header.
    ///
    /// See [`CorsLayer::allow_methods`] for more details.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Methods
    pub fn allow_methods<T>(self, methods: T) -> Self
    where
        T: Into<AnyOr<Vec<Method>>>,
    {
        self.map_layer(|layer| layer.allow_methods(methods))
    }

    /// Set the value of the [`Access-Control-Allow-Origin`][mdn] header.
    ///
    /// See [`CorsLayer::allow_origin`] for more details.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin
    pub fn allow_origin<T>(self, origin: T) -> Self
    where
        T: Into<AnyOr<Origin>>,
    {
        self.map_layer(|layer| layer.allow_origin(origin))
    }

    /// Set the value of the [`Access-Control-Expose-Headers`][mdn] header.
    ///
    /// See [`CorsLayer::expose_headers`] for more details.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Expose-Headers
    pub fn expose_headers<I>(self, headers: I) -> Self
    where
        I: Into<AnyOr<Vec<HeaderName>>>,
    {
        self.map_layer(|layer| layer.expose_headers(headers))
    }

    fn map_layer<F>(mut self, f: F) -> Self
    where
        F: FnOnce(CorsLayer) -> CorsLayer,
    {
        self.layer = f(self.layer);
        self
    }

    fn is_valid_origin(&self, origin: &HeaderValue, parts: &Parts) -> bool {
        match &self.layer.allow_origin.0 {
            AnyOrInner::Any => true,
            AnyOrInner::Value(allow_origin) => match &allow_origin.0 {
                OriginInner::Exact(s) => s == origin,
                OriginInner::List(list) => list.contains(origin),
                OriginInner::Closure(f) => f(origin, parts),
            },
        }
    }

    fn is_valid_request_method(&self, method: &HeaderValue) -> bool {
        self.layer.allow_methods == method
    }

    fn build_preflight_response<B>(&self, origin: HeaderValue) -> Response<B>
    where
        B: Default,
    {
        let mut response = Response::new(B::default());
        response
            .headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);

        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            self.layer.allow_methods.clone(),
        );

        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            self.layer.allow_headers.clone(),
        );

        if let Some(max_age) = self.layer.max_age.clone() {
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_MAX_AGE, max_age);
        }

        if let Some(allow_credentials) = self.layer.allow_credentials.clone() {
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, allow_credentials);
        }

        if let Some(expose_headers) = self.layer.expose_headers.clone() {
            response
                .headers_mut()
                .insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, expose_headers);
        }

        response
    }
}

/// Represents a [`Access-Control-Allow-Origin`][mdn] header.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin
#[derive(Clone, Debug)]
pub struct Origin(OriginInner);

impl Origin {
    /// Set a single allow origin target
    pub fn exact(origin: HeaderValue) -> Self {
        Self(OriginInner::Exact(origin))
    }

    /// Set multiple allow origin targets
    pub fn list<I>(origins: I) -> Self
    where
        I: IntoIterator<Item = HeaderValue>,
    {
        let origins = origins.into_iter().collect::<Vec<_>>().into();
        Self(OriginInner::List(origins))
    }
}

#[derive(Clone)]
enum OriginInner {
    Exact(HeaderValue),
    List(Arc<[HeaderValue]>),
    Closure(Arc<dyn for<'a> Fn(&'a HeaderValue, &'a Parts) -> bool + Send + Sync + 'static>),
}

impl fmt::Debug for OriginInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(inner) => f.debug_tuple("Exact").field(inner).finish(),
            Self::List(inner) => f.debug_tuple("List").field(inner).finish(),
            Self::Closure(_) => f.debug_tuple("Closure").finish(),
        }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for Cors<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let origin = req.headers().get(&header::ORIGIN).cloned();

        let origin = if let Some(origin) = origin {
            origin
        } else {
            // This is not a CORS request if there is no Origin header
            return ResponseFuture {
                inner: Kind::NonCorsCall(self.inner.call(req)),
            };
        };

        let (parts, body) = req.into_parts();

        if !self.is_valid_origin(&origin, &parts) {
            return ResponseFuture {
                inner: Kind::Error(Some(
                    Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(ResBody::default())
                        .unwrap(),
                )),
            };
        }

        let req = Request::from_parts(parts, body);

        // Return results immediately upon preflight request
        if req.method() == Method::OPTIONS {
            // the method the real request will be made with
            match req.headers().get(header::ACCESS_CONTROL_REQUEST_METHOD) {
                Some(request_method) => {
                    if !self.is_valid_request_method(request_method) {
                        return ResponseFuture {
                            inner: Kind::Error(Some(
                                Response::builder()
                                    .status(StatusCode::METHOD_NOT_ALLOWED)
                                    .body(ResBody::default())
                                    .unwrap(),
                            )),
                        };
                    }
                }
                None => {
                    return ResponseFuture {
                        inner: Kind::Error(Some(
                            Response::builder()
                                .status(StatusCode::UNAUTHORIZED)
                                .body(ResBody::default())
                                .unwrap(),
                        )),
                    };
                }
            }

            return ResponseFuture {
                inner: Kind::Error(Some(self.build_preflight_response(origin))),
            };
        }

        ResponseFuture {
            inner: Kind::CorsCall {
                future: self.inner.call(req),
                allow_origin: Some(self.layer.allow_origin.clone()),
                origin,
                allow_credentials: self.layer.allow_credentials.clone(),
                expose_headers: self.layer.expose_headers.clone(),
            },
        }
    }
}

/// Response future for [`Cors`].
#[pin_project]
pub struct ResponseFuture<F, B> {
    #[pin]
    inner: Kind<F, B>,
}

#[pin_project(project = KindProj)]
enum Kind<F, B> {
    NonCorsCall(#[pin] F),
    CorsCall {
        #[pin]
        future: F,
        allow_origin: Option<AnyOr<Origin>>,
        origin: HeaderValue,
        allow_credentials: Option<HeaderValue>,
        expose_headers: Option<HeaderValue>,
    },
    Error(Option<Response<B>>),
}

impl<F, B, E> Future for ResponseFuture<F, B>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().inner.project() {
            KindProj::CorsCall {
                future,
                allow_origin,
                origin,
                allow_credentials,
                expose_headers,
            } => {
                let mut response: Response<B> = ready!(future.poll(cx))?;

                response.headers_mut().insert(
                    header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    response_origin(allow_origin.take().unwrap(), origin),
                );

                if let Some(allow_credentials) = allow_credentials {
                    response.headers_mut().insert(
                        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                        allow_credentials.clone(),
                    );
                }

                if let Some(expose_headers) = expose_headers {
                    response.headers_mut().insert(
                        header::ACCESS_CONTROL_EXPOSE_HEADERS,
                        expose_headers.clone(),
                    );
                }

                Poll::Ready(Ok(response))
            }
            KindProj::NonCorsCall(future) => future.poll(cx),
            KindProj::Error(res) => Poll::Ready(Ok(res.take().unwrap())),
        }
    }
}

fn response_origin(allow_origin: AnyOr<Origin>, origin: &HeaderValue) -> HeaderValue {
    if let AnyOrInner::Any = allow_origin.0 {
        WILDCARD.parse().unwrap()
    } else {
        origin.clone()
    }
}
