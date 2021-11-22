//! Add authorization to requests using the [`Authorization`] header.
//!
//! [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
//!
//! # Example
//!
//! ```
//! use tower_http::auth::AddAuthorizationLayer;
//! use hyper::{Request, Response, Body, Error};
//! use http::{StatusCode, header::AUTHORIZATION};
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! # async fn handle(request: Request<Body>) -> Result<Response<Body>, Error> {
//! #     Ok(Response::new(Body::empty()))
//! # }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let service_that_requires_auth = tower_http::auth::RequireAuthorization::basic(
//! #     tower::service_fn(handle),
//! #     "username",
//! #     "password",
//! # );
//! let mut client = ServiceBuilder::new()
//!     // Use basic auth with the given username and password
//!     .layer(AddAuthorizationLayer::basic("username", "password"))
//!     .service(service_that_requires_auth);
//!
//! // Make a request, we don't have to add the `Authorization` header manually
//! let response = client
//!     .ready()
//!     .await?
//!     .call(Request::new(Body::empty()))
//!     .await?;
//!
//! assert_eq!(StatusCode::OK, response.status());
//! # Ok(())
//! # }
//! ```

use http::{HeaderValue, Request, Response};
use std::{
    convert::TryFrom,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`AddAuthorization`] which adds authorization to all requests using the
/// [`Authorization`] header.
///
/// See the [module docs](crate::auth::add_authorization) for an example.
///
/// You can also use [`SetRequestHeader`] if you have a use case that isn't supported by this
/// middleware.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
/// [`SetRequestHeader`]: crate::set_header::SetRequestHeader
#[derive(Debug, Clone)]
pub struct AddAuthorizationLayer {
    value: HeaderValue,
}

impl AddAuthorizationLayer {
    /// Authorize requests using a username and password pair.
    ///
    /// The `Authorization` header will be set to `Basic {credentials}` where `credentials` is
    /// `base64_encode("{username}:{password}")`.
    ///
    /// Since the username and password is sent in clear text it is recommended to use HTTPS/TLS
    /// with this method. However use of HTTPS/TLS is not enforced by this middleware.
    pub fn basic(username: &str, password: &str) -> Self {
        let encoded = base64::encode(format!("{}:{}", username, password));
        let value = HeaderValue::try_from(format!("Basic {}", encoded)).unwrap();
        Self { value }
    }

    /// Authorize requests using a "bearer token". Commonly used for OAuth 2.
    ///
    /// The `Authorization` header will be set to `Bearer {token}`.
    ///
    /// # Panics
    ///
    /// Panics if the token is not a valid [`HeaderValue`](http::header::HeaderValue).
    pub fn bearer(token: &str) -> Self {
        let value =
            HeaderValue::try_from(format!("Bearer {}", token)).expect("token is not valid header");
        Self { value }
    }

    /// Mark the header as [sensitive].
    ///
    /// This can for example be used to hide the header value from logs.
    ///
    /// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
    #[allow(clippy::wrong_self_convention)]
    pub fn as_sensitive(mut self, sensitive: bool) -> Self {
        self.value.set_sensitive(sensitive);
        self
    }
}

impl<S> Layer<S> for AddAuthorizationLayer {
    type Service = AddAuthorization<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AddAuthorization {
            inner,
            value: self.value.clone(),
        }
    }
}

/// Middleware that adds authorization all requests using the [`Authorization`] header.
///
/// See the [module docs](crate::auth::add_authorization) for an example.
///
/// You can also use [`SetRequestHeader`] if you have a use case that isn't supported by this
/// middleware.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
/// [`SetRequestHeader`]: crate::set_header::SetRequestHeader
#[derive(Debug, Clone)]
pub struct AddAuthorization<S> {
    inner: S,
    value: HeaderValue,
}

impl<S> AddAuthorization<S> {
    /// Authorize requests using a username and password pair.
    ///
    /// The `Authorization` header will be set to `Basic {credentials}` where `credentials` is
    /// `base64_encode("{username}:{password}")`.
    ///
    /// Since the username and password is sent in clear text it is recommended to use HTTPS/TLS
    /// with this method. However use of HTTPS/TLS is not enforced by this middleware.
    pub fn basic(inner: S, username: &str, password: &str) -> Self {
        AddAuthorizationLayer::basic(username, password).layer(inner)
    }

    /// Authorize requests using a "bearer token". Commonly used for OAuth 2.
    ///
    /// The `Authorization` header will be set to `Bearer {token}`.
    ///
    /// # Panics
    ///
    /// Panics if the token is not a valid [`HeaderValue`](http::header::HeaderValue).
    pub fn bearer(inner: S, token: &str) -> Self {
        AddAuthorizationLayer::bearer(token).layer(inner)
    }

    define_inner_service_accessors!();

    /// Mark the header as [sensitive].
    ///
    /// This can for example be used to hide the header value from logs.
    ///
    /// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
    #[allow(clippy::wrong_self_convention)]
    pub fn as_sensitive(mut self, sensitive: bool) -> Self {
        self.value.set_sensitive(sensitive);
        self
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for AddAuthorization<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        req.headers_mut()
            .insert(http::header::AUTHORIZATION, self.value.clone());
        self.inner.call(req)
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use crate::auth::RequireAuthorizationLayer;
    use http::{Response, StatusCode};
    use hyper::Body;
    use tower::{BoxError, Service, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn basic() {
        // service that requires auth for all requests
        let svc = ServiceBuilder::new()
            .layer(RequireAuthorizationLayer::basic("foo", "bar"))
            .service_fn(echo);

        // make a client that adds auth
        let mut client = AddAuthorization::basic(svc, "foo", "bar");

        let res = client
            .ready()
            .await
            .unwrap()
            .call(Request::new(Body::empty()))
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn token() {
        // service that requires auth for all requests
        let svc = ServiceBuilder::new()
            .layer(RequireAuthorizationLayer::bearer("foo"))
            .service_fn(echo);

        // make a client that adds auth
        let mut client = AddAuthorization::bearer(svc, "foo");

        let res = client
            .ready()
            .await
            .unwrap()
            .call(Request::new(Body::empty()))
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn making_header_sensitive() {
        let svc = ServiceBuilder::new()
            .layer(RequireAuthorizationLayer::bearer("foo"))
            .service_fn(|request: Request<Body>| async move {
                let auth = request.headers().get(http::header::AUTHORIZATION).unwrap();
                assert!(auth.is_sensitive());

                Ok::<_, hyper::Error>(Response::new(Body::empty()))
            });

        let mut client = AddAuthorization::bearer(svc, "foo").as_sensitive(true);

        let res = client
            .ready()
            .await
            .unwrap()
            .call(Request::new(Body::empty()))
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    async fn echo(req: Request<Body>) -> Result<Response<Body>, BoxError> {
        Ok(Response::new(req.into_body()))
    }
}
