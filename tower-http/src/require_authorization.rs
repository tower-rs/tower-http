//! Authorize requests using the [`Authorization`] header.
//!
//! [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
//!
//! # Example
//!
//! ```
//! use tower_http::require_authorization::RequireAuthorizationLayer;
//! use hyper::{Request, Response, Body, Error};
//! use http::{StatusCode, header::AUTHORIZATION};
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut service = ServiceBuilder::new()
//!     // Require the `Authorization` header to be `Bearer passwordlol`
//!     .layer(RequireAuthorizationLayer::bearer("passwordlol"))
//!     .service(service_fn(handle));
//!
//! // Requests with the correct token are allowed through
//! let request = Request::builder()
//!     .header(AUTHORIZATION, "Bearer passwordlol")
//!     .body(Body::empty())
//!     .unwrap();
//!
//! let response = service
//!     .ready_and()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(StatusCode::OK, response.status());
//!
//! // Requests with invalid token get a `401 Unauthorized` response
//! let request = Request::builder()
//!     .body(Body::empty())
//!     .unwrap();
//!
//! let response = service
//!     .ready_and()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(StatusCode::UNAUTHORIZED, response.status());
//! # Ok(())
//! # }
//! ```

use http::{
    header::{self, HeaderValue},
    HeaderMap, Request, Response, StatusCode,
};
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`RequireAuthorization`] which authorizes all requests using the
/// [`Authorization`] header.
///
/// See the [module docs](crate::require_authorization) for an example.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
#[derive(Debug, Clone)]
pub struct RequireAuthorizationLayer<T> {
    auth: T,
}

impl RequireAuthorizationLayer<Bearer> {
    /// Authorize requests using a "bearer token". Commonly used for OAuth 2.
    ///
    /// The `Authorization` header is required to be `Bearer {token}`.
    ///
    /// # Panics
    ///
    /// Panics if the token is not a valid [`HeaderValue`](http::header::HeaderValue).
    pub fn bearer(token: &str) -> Self {
        Self::custom(Bearer::new(token))
    }
}

impl RequireAuthorizationLayer<Basic> {
    /// Authorize requests using a username and password pair.
    ///
    /// The `Authorization` header is required to be `Basic {credentials}` where `credentials` is
    /// `base64_encode("{username}:{password}")`.
    ///
    /// Since the username and password is sent in clear text it is recommended to use HTTPS/TLS
    /// with this method. However use of HTTPS/TLS is not enforced by this middleware.
    pub fn basic(username: &str, password: &str) -> Self {
        Self::custom(Basic::new(username, password))
    }
}

impl<T> RequireAuthorizationLayer<T>
where
    T: AuthorizeRequest,
{
    /// Authorize requests using a custom scheme.
    ///
    /// The `Authorization` header is required to have the value provided.
    pub fn custom(auth: T) -> RequireAuthorizationLayer<T> {
        Self { auth }
    }
}

impl<S, T> Layer<S> for RequireAuthorizationLayer<T>
where
    T: Clone,
{
    type Service = RequireAuthorization<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        RequireAuthorization::new(inner, self.auth.clone())
    }
}

/// Middleware that authorizes all requests using the [`Authorization`] header.
///
/// See the [module docs](crate::require_authorization) for an example.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
#[derive(Clone, Debug)]
pub struct RequireAuthorization<S, T> {
    inner: S,
    auth: T,
}

impl<S, T> RequireAuthorization<S, T> {
    fn new(inner: S, auth: T) -> Self {
        Self { inner, auth }
    }

    define_inner_service_accessors!();
}

impl<S> RequireAuthorization<S, Bearer> {
    /// Authorize requests using a "bearer token". Commonly used for OAuth 2.
    ///
    /// The `Authorization` header is required to be `Bearer {token}`.
    ///
    /// # Panics
    ///
    /// Panics if the token is not a valid [`HeaderValue`](http::header::HeaderValue).
    pub fn bearer(inner: S, token: &str) -> Self {
        Self::custom(inner, Bearer::new(token))
    }
}

impl<S> RequireAuthorization<S, Basic> {
    /// Authorize requests using a username and password pair.
    ///
    /// The `Authorization` header is required to be `Basic {credentials}` where `credentials` is
    /// `base64_encode("{username}:{password}")`.
    ///
    /// Since the username and password is sent in clear text it is recommended to use HTTPS/TLS
    /// with this method. However use of HTTPS/TLS is not enforced by this middleware.
    pub fn basic(inner: S, username: &str, password: &str) -> Self {
        Self::custom(inner, Basic::new(username, password))
    }
}

impl<S, T> RequireAuthorization<S, T>
where
    T: AuthorizeRequest,
{
    /// Authorize requests using a custom scheme.
    ///
    /// The `Authorization` header is required to have the value provided.
    pub fn custom(inner: S, auth: T) -> RequireAuthorization<S, T> {
        Self { inner, auth }
    }
}

impl<ReqBody, ResBody, S, T> Service<Request<ReqBody>> for RequireAuthorization<S, T>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
    T: AuthorizeRequest,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if self.auth.authorize(&req) {
            ResponseFuture {
                kind: Kind::Future(self.inner.call(req)),
            }
        } else {
            let body = ResBody::default();
            let status_code = self.auth.status_code(&req);
            let headers = self.auth.response_headers(&req);
            ResponseFuture {
                kind: Kind::Error(Some((body, status_code, headers))),
            }
        }
    }
}

/// Response future for [`RequireAuthorization`].
#[pin_project]
pub struct ResponseFuture<F, B> {
    #[pin]
    kind: Kind<F, B>,
}

#[pin_project(project = KindProj)]
enum Kind<F, B> {
    Future(#[pin] F),
    Error(Option<(B, StatusCode, HeaderMap)>),
}

impl<F, B, E> Future for ResponseFuture<F, B>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().kind.project() {
            KindProj::Future(future) => future.poll(cx),
            KindProj::Error(data) => {
                let (body, status_code, headers) = data.take().unwrap();
                let mut response = Response::new(body);
                *response.status_mut() = status_code;
                *response.headers_mut() = headers;
                Poll::Ready(Ok(response))
            }
        }
    }
}

/// Trait for authorizing requests.
pub trait AuthorizeRequest {
    /// Authorize the request.
    ///
    /// If `true` is returned then the request is allowed through, otherwise not.
    fn authorize<B>(&mut self, request: &Request<B>) -> bool;

    /// The status code to use for the response of unauthorized requests.
    ///
    /// Defaults to [`401 Unauthorized`].
    ///
    /// [`401 Unauthorized`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/401
    #[allow(unused_variables)]
    fn status_code<B>(&mut self, request: &Request<B>) -> StatusCode {
        StatusCode::UNAUTHORIZED
    }

    /// Headers to add to the response of unauthorized requests.
    ///
    /// [`Basic`] uses this to set `WWW-Authenticate` on responses.
    ///
    /// Defaults to an empty [`HeaderMap`].
    #[allow(unused_variables)]
    fn response_headers<B>(&mut self, request: &Request<B>) -> HeaderMap {
        HeaderMap::new()
    }
}

/// Type that performs "bearer token" authorization.
///
/// See [`RequireAuthorization::bearer`] for more details.
#[derive(Debug, Clone)]
pub struct Bearer(HeaderValue);

impl Bearer {
    fn new(token: &str) -> Self {
        Self(
            format!("Bearer {}", token)
                .parse()
                .expect("token is not a valid header value"),
        )
    }
}

impl AuthorizeRequest for Bearer {
    fn authorize<B>(&mut self, request: &Request<B>) -> bool {
        if let Some(actual) = request.headers().get(header::AUTHORIZATION) {
            actual == self.0
        } else {
            false
        }
    }
}

/// Type that performs basic authorization.
///
/// See [`RequireAuthorization::basic`] for more details.
#[derive(Debug, Clone)]
pub struct Basic(HeaderValue);

impl Basic {
    fn new(username: &str, password: &str) -> Self {
        let encoded = base64::encode(format!("{}:{}", username, password));
        let header_value = format!("Basic {}", encoded).parse().unwrap();
        Self(header_value)
    }
}

impl AuthorizeRequest for Basic {
    fn authorize<B>(&mut self, request: &Request<B>) -> bool {
        if let Some(actual) = request.headers().get(header::AUTHORIZATION) {
            actual == self.0
        } else {
            false
        }
    }

    fn response_headers<B>(&mut self, _request: &Request<B>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::WWW_AUTHENTICATE, "Basic".parse().unwrap());
        headers
    }
}

impl AuthorizeRequest for HeaderValue {
    fn authorize<B>(&mut self, request: &Request<B>) -> bool {
        if let Some(actual) = request.headers().get(header::AUTHORIZATION) {
            actual == self
        } else {
            false
        }
    }
}
