//! Authorize requests using the [`Authorization`] header asynchronously.
//!
//! [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
//!
//! # Example
//!
//! ```
//! use tower_http::auth::{RequireAuthorizationAsyncLayer, AuthorizeRequestAsync};
//! use hyper::{Request, Response, Body, Error};
//! use http::{StatusCode, header::AUTHORIZATION};
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use futures_util::future::BoxFuture;
//!
//! #[derive(Clone, Copy)]
//! struct MyAuth;
//!
//! impl AuthorizeRequestAsync for MyAuth {
//!     type Output = UserId;
//!     type Future = BoxFuture<'static, Option<UserId>>;
//!     type ResponseBody = Body;
//!
//!     fn authorize<B>(&mut self, request: &Request<B>) -> Self::Future {
//!         # Box::pin(async {
//!             // ...
//!             # None
//!         })
//!     }
//!
//!     fn on_authorized<B>(&mut self, request: &mut Request<B>, user_id: UserId) {
//!         // Set `user_id` as a request extension so it can be accessed by other
//!         // services down the stack.
//!         request.extensions_mut().insert(user_id);
//!     }
//!
//!     fn unauthorized_response<B>(&mut self, request: &Request<B>) -> Response<Body> {
//!         Response::builder()
//!             .status(StatusCode::UNAUTHORIZED)
//!             .body(Body::empty())
//!             .unwrap()
//!     }
//! }
//!
//! #[derive(Debug)]
//! struct UserId(String);
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     // Access the `UserId` that was set in `on_authorized`. If `handle` gets called the
//!     // request was authorized and `UserId` will be present.
//!     let user_id = request
//!         .extensions()
//!         .get::<UserId>()
//!         .expect("UserId will be there if request was authorized");
//!
//!     println!("request from {:?}", user_id);
//!
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let service = ServiceBuilder::new()
//!     // Authorize requests using `MyAuth`
//!     .layer(RequireAuthorizationAsyncLayer::custom(MyAuth))
//!     .service_fn(handle);
//! # Ok(())
//! # }
//! ```

use futures_core::ready;
use http::{Request, Response};
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`RequireAuthorizationAsync`] which authorizes all requests using the
/// [`Authorization`] header.
///
/// See the [module docs](crate::auth::require_authorization_async) for an example.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
#[derive(Debug, Clone)]
pub struct RequireAuthorizationAsyncLayer<T> {
    auth: T,
}

impl<T> RequireAuthorizationAsyncLayer<T>
where
    T: AuthorizeRequestAsync,
{
    /// Authorize requests using a custom scheme.
    pub fn custom(auth: T) -> RequireAuthorizationAsyncLayer<T> {
        Self { auth }
    }
}

impl<S, T> Layer<S> for RequireAuthorizationAsyncLayer<T>
where
    T: Clone,
{
    type Service = RequireAuthorizationAsync<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        RequireAuthorizationAsync::new(inner, self.auth.clone())
    }
}

/// Middleware that authorizes all requests using the [`Authorization`] header.
///
/// See the [module docs](crate::auth::require_authorization_async) for an example.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
#[derive(Clone, Debug)]
pub struct RequireAuthorizationAsync<S, T> {
    inner: S,
    auth: T,
}

impl<S, T> RequireAuthorizationAsync<S, T> {
    fn new(inner: S, auth: T) -> Self {
        Self { inner, auth }
    }

    define_inner_service_accessors!();
}

impl<S, T> RequireAuthorizationAsync<S, T>
where
    T: AuthorizeRequestAsync,
{
    /// Authorize requests using a custom scheme.
    ///
    /// The `Authorization` header is required to have the value provided.
    pub fn custom(inner: S, auth: T) -> RequireAuthorizationAsync<S, T> {
        Self { inner, auth }
    }
}

impl<ReqBody, ResBody, S, T> Service<Request<ReqBody>> for RequireAuthorizationAsync<S, T>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ResBody: Default,
    T: AuthorizeRequestAsync<ResponseBody = ResBody> + Clone,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = ResponseFuture<T, S, ReqBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let auth = self.auth.clone();
        let inner = self.inner.clone();
        let authorize = self.auth.authorize(&req);

        ResponseFuture {
            auth,
            state: State::Authorize {
                authorize,
                req: Some(req),
            },
            service: inner,
        }
    }
}

#[pin_project(project = StateProj)]
enum State<A, ReqBody, SFut> {
    Authorize {
        #[pin]
        authorize: A,
        req: Option<Request<ReqBody>>,
    },
    Authorized {
        #[pin]
        fut: SFut,
    },
}

/// Response future for [`RequireAuthorizationAsync`].
#[pin_project]
pub struct ResponseFuture<Auth, S, ReqBody>
where
    Auth: AuthorizeRequestAsync,
    S: Service<Request<ReqBody>>,
{
    auth: Auth,
    #[pin]
    state: State<Auth::Future, ReqBody, S::Future>,
    service: S,
}

impl<Auth, S, ReqBody, B> Future for ResponseFuture<Auth, S, ReqBody>
where
    Auth: AuthorizeRequestAsync<ResponseBody = B>,
    S: Service<Request<ReqBody>, Response = Response<B>>,
{
    type Output = Result<Response<B>, S::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        loop {
            match this.state.as_mut().project() {
                StateProj::Authorize { authorize, req } => {
                    let auth = ready!(authorize.poll(cx));
                    let mut req = req.take().expect("future polled after completion");
                    match auth {
                        Some(output) => {
                            this.auth.on_authorized(&mut req, output);
                            let fut = this.service.call(req);
                            this.state.set(State::Authorized { fut })
                        }
                        None => {
                            let res = this.auth.unauthorized_response(&req);
                            return Poll::Ready(Ok(res));
                        }
                    };
                }
                StateProj::Authorized { fut } => {
                    return fut.poll(cx);
                }
            }
        }
    }
}

/// Trait for authorizing requests.
pub trait AuthorizeRequestAsync {
    /// The output type of doing the authorization.
    ///
    /// Use `()` if authorization doesn't produce any meaningful output.
    type Output;

    /// The Future type returned by `authorize`
    type Future: Future<Output = Option<Self::Output>>;

    /// The body type used for responses to unauthorized requests.
    type ResponseBody: Body;

    /// Authorize the request.
    ///
    /// If `Some(_)` is returned then the request is allowed through, otherwise not.
    fn authorize<B>(&mut self, request: &Request<B>) -> Self::Future;

    /// Callback for when a request has been successfully authorized.
    ///
    /// For example this allows you to save `Self::Output` in a [request extension][] to make it
    /// available to services further down the stack. This could for example be the "claims" for a
    /// valid [JWT].
    ///
    /// Defaults to doing nothing.
    ///
    /// See the [module docs](crate::auth::require_authorization_async) for an example.
    ///
    /// [request extension]: https://docs.rs/http/latest/http/struct.Extensions.html
    /// [JWT]: https://jwt.io
    #[inline]
    fn on_authorized<B>(&mut self, _request: &mut Request<B>, _output: Self::Output) {}

    /// Create the response for an unauthorized request.
    fn unauthorized_response<B>(&mut self, request: &Request<B>) -> Response<Self::ResponseBody>;
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use futures_util::future::BoxFuture;
    use http::{header, StatusCode};
    use hyper::Body;
    use tower::{BoxError, ServiceBuilder, ServiceExt};

    #[derive(Clone, Copy)]
    struct MyAuth;

    impl AuthorizeRequestAsync for MyAuth {
        type Output = UserId;
        type Future = BoxFuture<'static, Option<UserId>>;
        type ResponseBody = Body;

        fn authorize<B>(&mut self, request: &Request<B>) -> Self::Future {
            let authorized = request
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|it| it.to_str().ok())
                .and_then(|it| it.strip_prefix("Bearer "))
                .map(|it| it == "69420")
                .unwrap_or(false);

            Box::pin(async move {
                if authorized {
                    Some(UserId(String::from("6969")))
                } else {
                    None
                }
            })
        }

        fn on_authorized<B>(&mut self, request: &mut Request<B>, user_id: UserId) {
            request.extensions_mut().insert(user_id);
        }

        fn unauthorized_response<B>(&mut self, _request: &Request<B>) -> Response<Body> {
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::empty())
                .unwrap()
        }
    }

    #[derive(Debug)]
    struct UserId(String);

    #[tokio::test]
    async fn require_async_auth_works() {
        let mut service = ServiceBuilder::new()
            .layer(RequireAuthorizationAsyncLayer::custom(MyAuth))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::AUTHORIZATION, "Bearer 69420")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn require_async_auth_401() {
        let mut service = ServiceBuilder::new()
            .layer(RequireAuthorizationAsyncLayer::custom(MyAuth))
            .service_fn(echo);

        let request = Request::get("/")
            .header(header::AUTHORIZATION, "Bearer deez")
            .body(Body::empty())
            .unwrap();

        let res = service.ready().await.unwrap().call(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    async fn echo(req: Request<Body>) -> Result<Response<Body>, BoxError> {
        Ok(Response::new(req.into_body()))
    }
}
