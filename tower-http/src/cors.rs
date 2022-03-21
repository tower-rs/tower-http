//! Middleware which adds headers for [CORS][mdn].
//!
//! # Example
//!
//! ```
//! use http::{Request, Response, Method, header};
//! use hyper::Body;
//! use tower::{ServiceBuilder, ServiceExt, Service};
//! use tower_http::cors::{Any, CorsLayer};
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

#![allow(clippy::enum_variant_names)]

use bytes::{BufMut, BytesMut};
use futures_core::ready;
use http::{
    header::{self, HeaderName, HeaderValue},
    request::Parts,
    HeaderMap, Method, Request, Response,
};
use pin_project_lite::pin_project;
use std::{
    fmt,
    future::Future,
    mem,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies the [`Cors`] middleware which adds headers for [CORS][mdn].
///
/// See the [module docs](crate::cors) for an example.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS
#[derive(Debug, Clone)]
pub struct CorsLayer {
    allow_credentials: bool,
    allow_headers: Option<HeaderValue>,
    allow_methods: Option<HeaderValue>,
    allow_origin: Option<AnyOr<Origin>>,
    expose_headers: Option<HeaderValue>,
    max_age: Option<HeaderValue>,
}

#[allow(clippy::declare_interior_mutable_const)]
const WILDCARD: HeaderValue = HeaderValue::from_static("*");

impl CorsLayer {
    /// Create a new `CorsLayer`.
    ///
    /// This creates a restrictive configuration. Use the builder methods to
    /// customize the behavior.
    pub fn new() -> Self {
        Self {
            allow_credentials: false,
            allow_headers: None,
            allow_methods: None,
            allow_origin: None,
            expose_headers: None,
            max_age: None,
        }
    }

    /// A very permissive configuration suitable for development:
    ///
    /// - Credentials allowed.
    /// - All request headers allowed.
    /// - All methods allowed.
    /// - All origins allowed.
    /// - All headers exposed.
    /// - Max age set to 1 hour.
    ///
    /// Note this is not recommended for production use.
    pub fn permissive() -> Self {
        Self::new()
            .allow_credentials(true)
            .allow_headers(Any)
            .allow_methods(Any)
            .allow_origin(Any)
            .expose_headers(Any)
            .max_age(Duration::from_secs(60 * 60))
    }

    /// Set the [`Access-Control-Allow-Credentials`][mdn] header.
    ///
    /// ```
    /// use tower_http::cors::CorsLayer;
    ///
    /// let layer = CorsLayer::new().allow_credentials(true);
    /// ```
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Credentials
    pub fn allow_credentials(mut self, allow_credentials: bool) -> Self {
        self.allow_credentials = allow_credentials;
        self
    }

    /// Set the value of the [`Access-Control-Allow-Headers`][mdn] header.
    ///
    /// ```
    /// use tower_http::cors::CorsLayer;
    /// use http::header::{AUTHORIZATION, ACCEPT};
    ///
    /// let layer = CorsLayer::new().allow_headers(vec![AUTHORIZATION, ACCEPT]);
    /// ```
    ///
    /// All headers can be allowed with
    ///
    /// ```
    /// use tower_http::cors::{Any, CorsLayer};
    ///
    /// let layer = CorsLayer::new().allow_headers(Any);
    /// ```
    ///
    /// Note that multiple calls to this method will override any previous
    /// calls.
    ///
    /// Also note that `Access-Control-Allow-Headers` is required for requests that have
    /// `Access-Control-Request-Headers`.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Headers
    pub fn allow_headers<I>(mut self, headers: I) -> Self
    where
        I: Into<AnyOr<Vec<HeaderName>>>,
    {
        self.allow_headers = match headers.into().0 {
            AnyOrInner::Any => Some(WILDCARD),
            AnyOrInner::Value(headers) => separated_by_commas(headers.into_iter().map(Into::into)),
        };
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
    /// By default the header will not be set which disables caching and will
    /// require a preflight call for all requests.
    ///
    /// Note that each browser has a maximum internal value that takes
    /// precedence when the Access-Control-Max-Age is greater. For more details
    /// see [mdn].
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
    /// use tower_http::cors::{Any, CorsLayer};
    ///
    /// let layer = CorsLayer::new().allow_methods(Any);
    /// ```
    ///
    /// Note that multiple calls to this method will override any previous
    /// calls.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Methods
    pub fn allow_methods<T>(mut self, methods: T) -> Self
    where
        T: Into<AnyOr<Vec<Method>>>,
    {
        self.allow_methods = match methods.into().0 {
            AnyOrInner::Any => Some(WILDCARD),
            AnyOrInner::Value(methods) => separated_by_commas(
                methods
                    .into_iter()
                    .map(|m| HeaderValue::from_str(m.as_str()).unwrap()),
            ),
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
    /// Multiple origins can be allowed with
    ///
    /// ```
    /// use tower_http::cors::{CorsLayer, Origin};
    ///
    /// let origins = vec![
    ///     "http://example.com".parse().unwrap(),
    ///     "http://api.example.com".parse().unwrap(),
    /// ];
    ///
    /// let layer = CorsLayer::new().allow_origin(Origin::list(origins));
    /// ```
    ///
    /// All origins can be allowed with
    ///
    /// ```
    /// use tower_http::cors::{Any, CorsLayer};
    ///
    /// let layer = CorsLayer::new().allow_origin(Any);
    /// ```
    ///
    /// You can also use a closure
    ///
    /// ```
    /// use tower_http::cors::{CorsLayer, Origin};
    /// use http::{HeaderValue, request::Parts};
    ///
    /// let layer = CorsLayer::new().allow_origin(
    ///     Origin::predicate(|origin: &HeaderValue, _request_head: &Parts| {
    ///         origin.as_bytes().ends_with(b".rust-lang.org")
    ///     })
    /// );
    /// ```
    ///
    /// Note that multiple calls to this method will override any previous
    /// calls.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin
    pub fn allow_origin<T>(mut self, origin: T) -> Self
    where
        T: Into<AnyOr<Origin>>,
    {
        self.allow_origin = Some(origin.into());
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
    /// use tower_http::cors::{Any, CorsLayer};
    ///
    /// let layer = CorsLayer::new().expose_headers(Any);
    /// ```
    ///
    /// Note that multiple calls to this method will override any previous
    /// calls.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Expose-Headers
    pub fn expose_headers<I>(mut self, headers: I) -> Self
    where
        I: Into<AnyOr<Vec<HeaderName>>>,
    {
        self.expose_headers = match headers.into().0 {
            AnyOrInner::Any => Some(WILDCARD),
            AnyOrInner::Value(headers) => separated_by_commas(headers.into_iter().map(Into::into)),
        };
        self
    }
}

/// Represents a wildcard value (`*`) used with some CORS headers such as
/// [`CorsLayer::allow_methods`].
#[derive(Debug, Clone, Copy)]
pub struct Any;

/// Represents a wildcard value (`*`) used with some CORS headers such as
/// [`CorsLayer::allow_methods`].
#[deprecated = "Use Any as a unit struct literal instead"]
pub fn any() -> Any {
    Any
}

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

fn separated_by_commas<I>(mut iter: I) -> Option<HeaderValue>
where
    I: Iterator<Item = HeaderValue>,
{
    match iter.next() {
        Some(fst) => {
            let mut result = BytesMut::from(fst.as_bytes());
            for val in iter {
                result.reserve(val.len() + 1);
                result.put_u8(b',');
                result.extend_from_slice(val.as_bytes());
            }

            Some(HeaderValue::from_maybe_shared(result.freeze()).unwrap())
        }
        None => None,
    }
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
    ///
    /// This creates a restrictive configuration. Use the builder methods to
    /// customize the behavior.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            layer: CorsLayer::new(),
        }
    }

    /// A very permissive configuration suitable for development.
    ///
    /// See [`CorsLayer::permissive`] for more details.
    pub fn permissive(inner: S) -> Self {
        Self {
            inner,
            layer: CorsLayer::permissive(),
        }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a [`Cors`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer() -> CorsLayer {
        CorsLayer::new()
    }

    /// Set the [`Access-Control-Allow-Credentials`][mdn] header.
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
        I: Into<AnyOr<Vec<HeaderName>>>,
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

    fn make_response_header_map(&self) -> HeaderMap {
        #[allow(clippy::declare_interior_mutable_const)]
        const TRUE: HeaderValue = HeaderValue::from_static("true");

        let mut headers = HeaderMap::new();

        if self.layer.allow_credentials {
            headers.insert(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, TRUE);
        }

        if let Some(expose_headers) = self.layer.expose_headers.clone() {
            headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, expose_headers);
        }

        apply_vary_headers(&mut headers);

        headers
    }

    fn make_preflight_header_map(&self, origin: HeaderValue, parts: &Parts) -> HeaderMap {
        let mut headers = self.make_response_header_map();

        if let Some(allow_origin) = &self.layer.allow_origin {
            if let Some(origin) = allow_origin.to_header_val(origin, parts) {
                headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
            }
        }

        if let Some(allow_methods) = &self.layer.allow_methods {
            headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, allow_methods.clone());
        }

        if let Some(allow_headers) = &self.layer.allow_headers {
            headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, allow_headers.clone());
        }

        if let Some(max_age) = self.layer.max_age.clone() {
            headers.insert(header::ACCESS_CONTROL_MAX_AGE, max_age);
        }

        headers
    }
}

/// Represents a [`Access-Control-Allow-Origin`][mdn] header.
///
/// See [`CorsLayer::allow_origin`] for more details.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin
#[derive(Clone)]
pub struct Origin(OriginInner);

impl Origin {
    /// Set a single allow origin target
    ///
    /// See [`CorsLayer::allow_origin`] for more details.
    pub fn exact(origin: HeaderValue) -> Self {
        Self(OriginInner::Const(Some(origin)))
    }

    /// Set multiple allow origin targets
    ///
    /// See [`CorsLayer::allow_origin`] for more details.
    pub fn list<I>(origins: I) -> Self
    where
        I: IntoIterator<Item = HeaderValue>,
    {
        Self(OriginInner::Const(separated_by_commas(
            origins.into_iter().map(Into::into),
        )))
    }

    /// Set the allowed origins from a predicate
    ///
    /// See [`CorsLayer::allow_origin`] for more details.
    pub fn predicate<F>(f: F) -> Self
    where
        F: Fn(&HeaderValue, &Parts) -> bool + Send + Sync + 'static,
    {
        Self(OriginInner::Closure(Arc::new(f)))
    }

    fn to_header_val(&self, origin: HeaderValue, parts: &Parts) -> Option<HeaderValue> {
        match &self.0 {
            OriginInner::Const(v) => v.clone(),
            OriginInner::Closure(c) => {
                if c(&origin, parts) {
                    Some(origin)
                } else {
                    None
                }
            }
        }
    }
}

impl AnyOr<Origin> {
    fn to_header_val(&self, origin: HeaderValue, parts: &Parts) -> Option<HeaderValue> {
        match &self.0 {
            AnyOrInner::Any => Some(WILDCARD),
            AnyOrInner::Value(o) => o.to_header_val(origin, parts),
        }
    }
}

impl fmt::Debug for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            OriginInner::Const(inner) => f.debug_tuple("Const").field(inner).finish(),
            OriginInner::Closure(_) => f.debug_tuple("Closure").finish(),
        }
    }
}

#[derive(Clone)]
enum OriginInner {
    Const(Option<HeaderValue>),
    Closure(Arc<dyn for<'a> Fn(&'a HeaderValue, &'a Parts) -> bool + Send + Sync + 'static>),
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for Cors<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

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
                inner: Kind::NonCorsCall {
                    future: self.inner.call(req),
                },
            };
        };

        let (parts, body) = req.into_parts();

        // Return results immediately upon preflight request
        if parts.method == Method::OPTIONS {
            return ResponseFuture {
                inner: Kind::PreflightCall {
                    headers: self.make_preflight_header_map(origin, &parts),
                },
            };
        }

        let req = Request::from_parts(parts, body);

        let mut headers = self.make_response_header_map();
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            response_origin(self.layer.allow_origin.as_ref().unwrap(), &origin),
        );

        ResponseFuture {
            inner: Kind::CorsCall {
                future: self.inner.call(req),
                headers,
            },
        }
    }
}

pin_project! {
    /// Response future for [`Cors`].
    pub struct ResponseFuture<F> {
        #[pin]
        inner: Kind<F>,
    }
}

pin_project! {
    #[project = KindProj]
    enum Kind<F> {
        NonCorsCall {
            #[pin]
            future: F,
        },
        CorsCall {
            #[pin]
            future: F,
            headers: HeaderMap,
        },
        PreflightCall {
            headers: HeaderMap,
        },
    }
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: Default,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().inner.project() {
            KindProj::CorsCall { future, headers } => {
                let mut response: Response<B> = ready!(future.poll(cx))?;
                response.headers_mut().extend(headers.drain());

                Poll::Ready(Ok(response))
            }
            KindProj::NonCorsCall { future } => future.poll(cx),
            KindProj::PreflightCall { headers } => {
                let mut response = Response::new(B::default());
                mem::swap(response.headers_mut(), headers);

                Poll::Ready(Ok(response))
            }
        }
    }
}

fn apply_vary_headers(headers: &mut http::HeaderMap) {
    const VARY_HEADERS: [HeaderName; 3] = [
        header::ORIGIN,
        header::ACCESS_CONTROL_REQUEST_METHOD,
        header::ACCESS_CONTROL_REQUEST_HEADERS,
    ];

    for h in &VARY_HEADERS {
        headers.append(header::VARY, HeaderValue::from_static(h.as_str()));
    }
}

fn response_origin(allow_origin: &AnyOr<Origin>, origin: &HeaderValue) -> HeaderValue {
    if let AnyOrInner::Any = &allow_origin.0 {
        WILDCARD
    } else {
        origin.clone()
    }
}
