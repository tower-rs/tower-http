//! Middleware for following redirections.
//!
//! # Overview
//!
//! The [`FollowRedirect`] middleware retries requests with the inner [`Service`] to follow HTTP
//! redirections.
//!
//! The middleware tries to clone the original [`Request`] when making a redirected request.
//! However, since [`Extensions`][http::Extensions] are `!Clone`, any extensions set by outer
//! middleware will be discarded. Also, the request body cannot always be cloned. When the
//! original body is known to be empty by [`Body::size_hint`], the middleware uses `Default`
//! implementation of the body type to create a new request body. If you know that the body can be
//! cloned in some way, you can tell the middleware to clone it by configuring a [`policy`].
//!
//! # Examples
//!
//! ## Basic usage
//!
//! ```
//! use http::{Request, Response};
//! use bytes::Bytes;
//! use http_body_util::Full;
//! use tower::{Service, ServiceBuilder, ServiceExt};
//! use tower_http::follow_redirect::{FollowRedirectLayer, RequestUri};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), std::convert::Infallible> {
//! # let http_client = tower::service_fn(|req: Request<_>| async move {
//! #     let dest = "https://www.rust-lang.org/";
//! #     let mut res = http::Response::builder();
//! #     if req.uri() != dest {
//! #         res = res
//! #             .status(http::StatusCode::MOVED_PERMANENTLY)
//! #             .header(http::header::LOCATION, dest);
//! #     }
//! #     Ok::<_, std::convert::Infallible>(res.body(Full::<Bytes>::default()).unwrap())
//! # });
//! let mut client = ServiceBuilder::new()
//!     .layer(FollowRedirectLayer::new())
//!     .service(http_client);
//!
//! let request = Request::builder()
//!     .uri("https://rust-lang.org/")
//!     .body(Full::<Bytes>::default())
//!     .unwrap();
//!
//! let response = client.ready().await?.call(request).await?;
//! // Get the final request URI.
//! assert_eq!(response.extensions().get::<RequestUri>().unwrap().0, "https://www.rust-lang.org/");
//! # Ok(())
//! # }
//! ```
//!
//! ## Customizing the `Policy`
//!
//! You can use a [`Policy`] value to customize how the middleware handles redirections.
//!
//! ```
//! use http::{Request, Response};
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use tower::{Service, ServiceBuilder, ServiceExt};
//! use tower_http::follow_redirect::{
//!     policy::{self, PolicyExt},
//!     FollowRedirectLayer,
//! };
//!
//! #[derive(Debug)]
//! enum MyError {
//!     TooManyRedirects,
//!     Other(tower::BoxError),
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), MyError> {
//! # let http_client =
//! #     tower::service_fn(|_: Request<Full<Bytes>>| async { Ok(Response::new(Full::<Bytes>::default())) });
//! let policy = policy::Limited::new(10) // Set the maximum number of redirections to 10.
//!     // Return an error when the limit was reached.
//!     .or::<_, (), _>(policy::redirect_fn(|_| Err(MyError::TooManyRedirects)))
//!     // Do not follow cross-origin redirections, and return the redirection responses as-is.
//!     .and::<_, (), _>(policy::SameOrigin::new());
//!
//! let mut client = ServiceBuilder::new()
//!     .layer(FollowRedirectLayer::with_policy(policy))
//!     .map_err(MyError::Other)
//!     .service(http_client);
//!
//! // ...
//! # let _ = client.ready().await?.call(Request::default()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Customizing extensions
//!
//! You can use [`FollowRedirectLayer::with_policy_extension()`]
//! to also set the [`FollowedPolicy`] extension on the response.
//!
//! ```
//! use http::{Request, Response};
//! use bytes::Bytes;
//! use http_body_util::Full;
//! use tower::{Service, ServiceBuilder, ServiceExt};
//! use tower_http::follow_redirect::{FollowRedirectLayer, FollowedPolicy, policy};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), std::convert::Infallible> {
//! # let http_client =
//! #     tower::service_fn(|_: Request<Full<Bytes>>| async { Ok::<_, std::convert::Infallible>(Response::new(Full::<Bytes>::default())) });
//! let mut client = ServiceBuilder::new()
//!     .layer(FollowRedirectLayer::with_policy_extension(policy::Limited::new(10)))
//!     .service(http_client);
//!
//! let res = client.ready().await?.call(Request::default()).await?;
//! assert_eq!(
//!     res.extensions()
//!         .get::<FollowedPolicy<policy::Limited>>()
//!         .unwrap()
//!         .0
//!         .remaining,
//!     10
//! );
//! # Ok(())
//! # }
//! ```

pub mod policy;

use self::policy::{Action, Attempt, Policy, Standard};
use futures_util::future::Either;
use http::{
    header::CONTENT_ENCODING, header::CONTENT_LENGTH, header::CONTENT_TYPE, header::LOCATION,
    header::TRANSFER_ENCODING, HeaderMap, HeaderValue, Method, Request, Response, StatusCode, Uri,
    Version,
};
use http_body::Body;
use iri_string::types::{UriAbsoluteString, UriReferenceStr};
use pin_project_lite::pin_project;
use std::{
    convert::TryFrom,
    future::Future,
    mem,
    pin::Pin,
    str,
    task::{ready, Context, Poll},
};
use tower::util::Oneshot;
use tower_layer::Layer;
use tower_service::Service;

/// [`Layer`] for retrying requests with a [`Service`] to follow redirection responses.
///
/// See the [module docs](self) for more details.
#[derive(Clone, Copy, Debug, Default)]
pub struct FollowRedirectLayer<P = Standard, H = UriExtension> {
    policy: P,
    handler: H,
}

impl FollowRedirectLayer {
    /// Create a new [`FollowRedirectLayer`] with a [`Standard`] redirection policy.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<P> FollowRedirectLayer<P> {
    /// Create a new [`FollowRedirectLayer`] with the given redirection [`Policy`].
    pub fn with_policy(policy: P) -> Self {
        Self {
            policy,
            handler: UriExtension::default(),
        }
    }
}

impl<P> FollowRedirectLayer<P, UriAndPolicyExtensions>
where
    P: Send + Sync + 'static,
{
    /// Create a new [`FollowRedirectLayer`] with the given redirection [`Policy`],
    /// that adds a [`FollowedPolicy`] extension.
    pub fn with_policy_extension(policy: P) -> Self {
        Self {
            policy,
            handler: UriAndPolicyExtensions::default(),
        }
    }
}

impl<S, P, H> Layer<S> for FollowRedirectLayer<P, H>
where
    S: Clone,
    P: Clone,
    H: Copy,
{
    type Service = FollowRedirect<S, P, H>;

    fn layer(&self, inner: S) -> Self::Service {
        FollowRedirect::with_policy_handler(inner, self.policy.clone(), self.handler)
    }
}

/// Middleware that retries requests with a [`Service`] to follow redirection responses.
///
/// See the [module docs](self) for more details.
#[derive(Clone, Copy, Debug)]
pub struct FollowRedirect<S, P = Standard, H = UriExtension> {
    inner: S,
    policy: P,
    handler: H,
}

impl<S> FollowRedirect<S> {
    /// Create a new [`FollowRedirect`] with a [`Standard`] redirection policy.
    pub fn new(inner: S) -> Self {
        Self::with_policy(inner, Standard::default())
    }

    /// Returns a new [`Layer`] that wraps services with a `FollowRedirect` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer() -> FollowRedirectLayer {
        FollowRedirectLayer::new()
    }
}

impl<S> FollowRedirect<S, Standard, UriAndPolicyExtensions> {
    /// Create a new [`FollowRedirect`] with a [`Standard`] redirection policy,
    /// that inserts the [`FollowedPolicy`] extension.
    pub fn with_extension(inner: S) -> Self {
        Self::with_policy_handler(
            inner,
            Standard::default(),
            UriAndPolicyExtensions::default(),
        )
    }

    /// Returns a new [`Layer`] that wraps services with a `FollowRedirect` middleware
    /// that inserts the [`FollowedPolicy`] extension.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer_with_extension() -> FollowRedirectLayer<Standard, UriAndPolicyExtensions> {
        FollowRedirectLayer::with_policy_extension(Standard::default())
    }
}

impl<S, P> FollowRedirect<S, P>
where
    P: Clone,
{
    /// Create a new [`FollowRedirect`] with the given redirection [`Policy`].
    pub fn with_policy(inner: S, policy: P) -> Self {
        Self::with_policy_handler(inner, policy, UriExtension::default())
    }

    /// Returns a new [`Layer`] that wraps services with a `FollowRedirect` middleware
    /// with the given redirection [`Policy`].
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer_with_policy(policy: P) -> FollowRedirectLayer<P> {
        FollowRedirectLayer::with_policy(policy)
    }
}

impl<S, P> FollowRedirect<S, P, UriAndPolicyExtensions>
where
    P: Clone + Send + Sync + 'static,
{
    /// Create a new [`FollowRedirect`] with the given redirection [`Policy`],
    /// that stores the policy in the [`FollowedPolicy`] extension.
    pub fn with_policy_extension(inner: S, policy: P) -> Self {
        Self::with_policy_handler(inner, policy, UriAndPolicyExtensions::default())
    }

    /// Returns a new [`Layer`] that wraps services with a [`FollowRedirect`] middleware
    /// that uses the given redirection [`Policy`] and store it in the [`FollowedPolicy`] extension.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer_with_policy_extension(
        policy: P,
    ) -> FollowRedirectLayer<P, UriAndPolicyExtensions> {
        FollowRedirectLayer::with_policy_extension(policy)
    }
}

impl<S, P, H> FollowRedirect<S, P, H>
where
    P: Clone,
{
    /// Create a new [`FollowRedirect`] with the given redirection [`Policy`] and [`ResponseHandler`].
    fn with_policy_handler(inner: S, policy: P, handler: H) -> Self {
        FollowRedirect {
            inner,
            policy,
            handler,
        }
    }

    define_inner_service_accessors!();
}

/// Called on each new response, can be used for example to add [`http::Extensions`]
trait ResponseHandler<ReqBody, ResBody, S, P>: Sized
where
    S: Service<Request<ReqBody>>,
{
    fn on_response(res: &mut Response<ResBody>, req: &RedirectingRequest<S, ReqBody, P>);
}

/// Default behavior: adds a [`RequestUri`] extension to the response.
#[derive(Default, Clone, Copy)]
pub struct UriExtension {}

impl<ReqBody, ResBody, S, P> ResponseHandler<ReqBody, ResBody, S, P> for UriExtension
where
    S: Service<Request<ReqBody>>,
{
    #[inline]
    fn on_response(res: &mut Response<ResBody>, req: &RedirectingRequest<S, ReqBody, P>) {
        res.extensions_mut().insert(RequestUri(req.uri.clone()));
    }
}

/// Adds a [`FollowedPolicy`] and [`RequestUri`] extension to the response.
#[derive(Default, Clone, Copy)]
pub struct UriAndPolicyExtensions {}

impl<ReqBody, ResBody, S, P> ResponseHandler<ReqBody, ResBody, S, P> for UriAndPolicyExtensions
where
    S: Service<Request<ReqBody>>,
    P: Clone + Send + Sync + 'static,
{
    #[inline]
    fn on_response(res: &mut Response<ResBody>, req: &RedirectingRequest<S, ReqBody, P>) {
        UriExtension::on_response(res, req);

        res.extensions_mut()
            .insert(FollowedPolicy(req.policy.clone()));
    }
}

impl<ReqBody, ResBody, S, P, H> Service<Request<ReqBody>> for FollowRedirect<S, P, H>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ReqBody: Body + Default,
    P: Policy<ReqBody, S::Error> + Clone,
    H: ResponseHandler<ReqBody, ResBody, S, P> + Copy,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = ResponseFuture<S, ReqBody, P, H>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let service = self.inner.clone();
        let service = mem::replace(&mut self.inner, service);
        let mut policy = self.policy.clone();
        let mut body = BodyRepr::None;
        body.try_clone_from(req.body(), &policy);
        policy.on_request(&mut req);

        let mut request = RedirectingRequest {
            method: req.method().clone(),
            uri: req.uri().clone(),
            version: req.version(),
            headers: req.headers().clone(),
            service,
            body,
            policy,
        };
        ResponseFuture {
            future: Either::Left(request.service.call(req)),
            request,
            handler: self.handler,
        }
    }
}

/// Wraps a [`http::Request`] with a [`policy::Policy`] to apply,
/// and an underlying service in case further requests are required.
#[derive(Debug)]
struct RedirectingRequest<S, B, P> {
    service: S,
    policy: P,
    method: Method,
    uri: Uri,
    version: Version,
    headers: HeaderMap<HeaderValue>,
    body: BodyRepr<B>,
}

pin_project! {
    /// Response future for [`FollowRedirect`].
    #[derive(Debug)]
    pub struct ResponseFuture<S, B, P, H=UriExtension>
    where
        S: Service<Request<B>>,
    {
        #[pin]
        future: Either<S::Future, Oneshot<S, Request<B>>>,
        request: RedirectingRequest<S, B, P>,
        handler: H
    }
}

impl<S, ReqBody, ResBody, P, H> Future for ResponseFuture<S, ReqBody, P, H>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ReqBody: Body + Default,
    P: Policy<ReqBody, S::Error>,
    H: ResponseHandler<ReqBody, ResBody, S, P>,
{
    type Output = Result<Response<ResBody>, S::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let mut res = ready!(this.future.as_mut().poll(cx)?);
        H::on_response(&mut res, this.request);

        let drop_payload_headers = |headers: &mut HeaderMap| {
            for header in &[
                CONTENT_TYPE,
                CONTENT_LENGTH,
                CONTENT_ENCODING,
                TRANSFER_ENCODING,
            ] {
                headers.remove(header);
            }
        };
        match res.status() {
            StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND => {
                // User agents MAY change the request method from POST to GET
                // (RFC 7231 section 6.4.2. and 6.4.3.).
                if this.request.method == Method::POST {
                    this.request.method = Method::GET;
                    this.request.body = BodyRepr::Empty;
                    drop_payload_headers(&mut this.request.headers);
                }
            }
            StatusCode::SEE_OTHER => {
                // A user agent can perform a GET or HEAD request (RFC 7231 section 6.4.4.).
                if this.request.method != Method::HEAD {
                    this.request.method = Method::GET;
                }
                this.request.body = BodyRepr::Empty;
                drop_payload_headers(&mut this.request.headers);
            }
            StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT => {}
            _ => return Poll::Ready(Ok(res)),
        };

        let body = if let Some(body) = this.request.body.take() {
            body
        } else {
            return Poll::Ready(Ok(res));
        };

        let location = res
            .headers()
            .get(&LOCATION)
            .and_then(|loc| resolve_uri(str::from_utf8(loc.as_bytes()).ok()?, &this.request.uri));
        let location = if let Some(loc) = location {
            loc
        } else {
            return Poll::Ready(Ok(res));
        };

        let attempt = Attempt {
            status: res.status(),
            location: &location,
            previous: &this.request.uri,
        };
        match this.request.policy.redirect(&attempt)? {
            Action::Follow => {
                this.request.uri = location;
                this.request
                    .body
                    .try_clone_from(&body, &this.request.policy);

                let mut req = Request::new(body);
                *req.uri_mut() = this.request.uri.clone();
                *req.method_mut() = this.request.method.clone();
                *req.version_mut() = this.request.version;
                *req.headers_mut() = this.request.headers.clone();
                this.request.policy.on_request(&mut req);
                this.future.set(Either::Right(Oneshot::new(
                    this.request.service.clone(),
                    req,
                )));

                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Action::Stop => Poll::Ready(Ok(res)),
        }
    }
}

/// Response [`Extensions`][http::Extensions] value that represents the effective request URI of
/// a response returned by a [`FollowRedirect`] middleware.
///
/// The value differs from the original request's effective URI if the middleware has followed
/// redirections.
#[derive(Clone)]
pub struct RequestUri(pub Uri);

/// Response [`Extensions`][http::Extensions] value that contains the redirect [`Policy`] that
/// was run before the last request of the redirect chain by a [`FollowRedirect<S, P, UriAndPolicyExtensions>`] middleware.
#[derive(Clone)]
pub struct FollowedPolicy<P>(pub P);

#[derive(Debug)]
enum BodyRepr<B> {
    Some(B),
    Empty,
    None,
}

impl<B> BodyRepr<B>
where
    B: Body + Default,
{
    fn take(&mut self) -> Option<B> {
        match mem::replace(self, BodyRepr::None) {
            BodyRepr::Some(body) => Some(body),
            BodyRepr::Empty => {
                *self = BodyRepr::Empty;
                Some(B::default())
            }
            BodyRepr::None => None,
        }
    }

    fn try_clone_from<P, E>(&mut self, body: &B, policy: &P)
    where
        P: Policy<B, E>,
    {
        match self {
            BodyRepr::Some(_) | BodyRepr::Empty => {}
            BodyRepr::None => {
                if let Some(body) = clone_body(policy, body) {
                    *self = BodyRepr::Some(body);
                }
            }
        }
    }
}

fn clone_body<P, B, E>(policy: &P, body: &B) -> Option<B>
where
    P: Policy<B, E>,
    B: Body + Default,
{
    if body.size_hint().exact() == Some(0) {
        Some(B::default())
    } else {
        policy.clone_body(body)
    }
}

/// Try to resolve a URI reference `relative` against a base URI `base`.
fn resolve_uri(relative: &str, base: &Uri) -> Option<Uri> {
    let relative = UriReferenceStr::new(relative).ok()?;
    let base = UriAbsoluteString::try_from(base.to_string()).ok()?;
    let uri = relative.resolve_against(&base).to_string();
    Uri::try_from(uri).ok()
}

#[cfg(test)]
mod tests {
    use super::{policy::*, *};
    use crate::test_helpers::Body;
    use http::header::LOCATION;
    use std::convert::Infallible;
    use tower::{ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn follows() {
        let svc = ServiceBuilder::new()
            .layer(FollowRedirectLayer::with_policy_extension(Action::Follow))
            .buffer(1)
            .service_fn(handle);
        let req = Request::builder()
            .uri("http://example.com/42")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(*res.body(), 0);
        assert_eq!(
            res.extensions().get::<RequestUri>().unwrap().0,
            "http://example.com/0"
        );
        assert!(res
            .extensions()
            .get::<FollowedPolicy<Action>>()
            .unwrap()
            .0
            .is_follow());
    }

    #[tokio::test]
    async fn stops() {
        let svc = ServiceBuilder::new()
            .layer(FollowRedirectLayer::with_policy_extension(Action::Stop))
            .buffer(1)
            .service_fn(handle);
        let req = Request::builder()
            .uri("http://example.com/42")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(*res.body(), 42);
        assert_eq!(
            res.extensions().get::<RequestUri>().unwrap().0,
            "http://example.com/42"
        );
        assert!(res
            .extensions()
            .get::<FollowedPolicy<Action>>()
            .unwrap()
            .0
            .is_stop());
    }

    #[tokio::test]
    async fn limited() {
        let svc = ServiceBuilder::new()
            .layer(FollowRedirectLayer::with_policy_extension(Limited::new(10)))
            .buffer(1)
            .service_fn(handle);
        let req = Request::builder()
            .uri("http://example.com/42")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(*res.body(), 42 - 10);
        assert_eq!(
            res.extensions().get::<RequestUri>().unwrap().0,
            "http://example.com/32"
        );
        assert_eq!(
            res.extensions()
                .get::<FollowedPolicy<Limited>>()
                .unwrap()
                .0
                .remaining,
            0
        );
    }

    /// A server with an endpoint `GET /{n}` which redirects to `/{n-1}` unless `n` equals zero,
    /// returning `n` as the response body.
    async fn handle<B>(req: Request<B>) -> Result<Response<u64>, Infallible> {
        let n: u64 = req.uri().path()[1..].parse().unwrap();
        let mut res = Response::builder();
        if n > 0 {
            res = res
                .status(StatusCode::MOVED_PERMANENTLY)
                .header(LOCATION, format!("/{}", n - 1));
        }
        Ok::<_, Infallible>(res.body(n).unwrap())
    }
}
