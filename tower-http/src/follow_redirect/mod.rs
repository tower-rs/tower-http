//! Middleware for following redirections.

pub mod policy;

use self::policy::{ActionKind, Attempt, Policy};
use futures_core::ready;
use http::{
    header::LOCATION, HeaderMap, HeaderValue, Method, Request, Response, StatusCode, Uri, Version,
};
use http_body::Body;
use iri_string::{
    spec::UriSpec,
    types::{RiAbsoluteString, RiReferenceStr},
};
use pin_project::pin_project;
use std::{
    convert::TryFrom,
    future::Future,
    pin::Pin,
    str,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// [`Layer`] for retrying requests with a [`Service`] to follow redirection responses.
#[derive(Clone, Copy, Debug, Default)]
pub struct FollowRedirectLayer<P> {
    policy: P,
}

impl<P> FollowRedirectLayer<P>
where
    P: Clone,
{
    /// Create a new [`FollowRedirectLayer`] with the given redirection [`Policy`].
    pub fn new(policy: P) -> Self {
        FollowRedirectLayer { policy }
    }
}

impl<S, P> Layer<S> for FollowRedirectLayer<P>
where
    S: Clone,
    P: Clone,
{
    type Service = FollowRedirect<S, P>;

    fn layer(&self, inner: S) -> Self::Service {
        FollowRedirect::new(inner, self.policy.clone())
    }
}

/// Middleware that retries requests with a [`Service`] to follow redirection responses.
#[derive(Clone, Copy, Debug)]
pub struct FollowRedirect<S, P> {
    inner: S,
    policy: P,
}

impl<S, P> FollowRedirect<S, P>
where
    P: Clone,
{
    /// Create a new [`FollowRedirect`] with the given redirection [`Policy`].
    pub fn new(inner: S, policy: P) -> Self {
        FollowRedirect { inner, policy }
    }

    /// Returns a new [`Layer`] that wraps services with a `FollowRedirect` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(policy: P) -> FollowRedirectLayer<P> {
        FollowRedirectLayer::new(policy)
    }
}

impl<ReqBody, ResBody, S, P> Service<Request<ReqBody>> for FollowRedirect<S, P>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ReqBody: Body + Default,
    P: Policy<ReqBody> + Clone,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, S, ReqBody, P>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut service = self.inner.clone();
        let policy = self.policy.clone();
        let body = clone_body(&policy, req.body());
        ResponseFuture {
            method: req.method().clone(),
            uri: req.uri().clone(),
            version: req.version(),
            headers: req.headers().clone(),
            body,
            future: service.call(req),
            service,
            policy,
        }
    }
}

/// Response future for [`FollowRedirect`].
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F, S, B, P> {
    #[pin]
    future: F,
    service: S,
    policy: P,
    method: Method,
    uri: Uri,
    version: Version,
    headers: HeaderMap<HeaderValue>,
    body: Option<B>,
}

impl<F, S, ReqBody, ResBody, P> Future for ResponseFuture<F, S, ReqBody, P>
where
    F: Future<Output = Result<Response<ResBody>, S::Error>>,
    S: Service<Request<ReqBody>, Future = F> + Clone,
    ReqBody: Body + Default,
    P: Policy<ReqBody>,
{
    type Output = Result<Response<ResBody>, S::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let res = ready!(this.future.as_mut().poll(cx)?);

        match res.status() {
            StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND => {
                // User agents MAY change the request method from POST to GET
                // (RFC 7231 section 6.4.2. and 6.4.3.).
                if *this.method == Method::POST {
                    *this.method = Method::GET;
                    *this.body = Some(ReqBody::default());
                }
            }
            StatusCode::SEE_OTHER => {
                // A user agent can perform a GET or HEAD request (RFC 7231 section 6.4.4.).
                if *this.method != Method::HEAD {
                    *this.method = Method::GET;
                }
                *this.body = Some(ReqBody::default());
            }
            StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT => {}
            _ => return Poll::Ready(Ok(res)),
        };

        let body = if let Some(body) = this.body.take() {
            body
        } else {
            return Poll::Ready(Ok(res));
        };

        let location = res
            .headers()
            .get(&LOCATION)
            .and_then(|loc| str::from_utf8(loc.as_bytes()).ok())
            .and_then(|loc| RiReferenceStr::<UriSpec>::new(loc).ok())
            .and_then(|loc| {
                Some(loc.resolve_against(&RiAbsoluteString::try_from(this.uri.to_string()).ok()?))
            })
            .and_then(|loc| Uri::try_from(loc.as_str()).ok());
        let location = if let Some(loc) = location {
            loc
        } else {
            return Poll::Ready(Ok(res));
        };

        let attempt = Attempt {
            status: res.status(),
            location: &location,
            previous: this.uri,
        };
        match this.policy.redirect(&attempt).kind {
            ActionKind::Follow => {
                *this.body = clone_body(this.policy, &body);

                let mut req = Request::new(body);
                *req.uri_mut() = location;
                *req.method_mut() = this.method.clone();
                *req.version_mut() = *this.version;
                *req.headers_mut() = this.headers.clone();
                this.future.set(this.service.call(req));

                cx.waker().wake_by_ref();
                Poll::Pending
            }
            ActionKind::Stop => Poll::Ready(Ok(res)),
        }
    }
}

fn clone_body<P, B>(policy: &P, body: &B) -> Option<B>
where
    P: Policy<B>,
    B: Body + Default,
{
    if body.size_hint().exact() == Some(0) {
        Some(B::default())
    } else {
        policy.clone_body(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{policy::*, *};
    use hyper::{header::LOCATION, Body};
    use std::convert::Infallible;
    use tower::{ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn follows() {
        let svc = ServiceBuilder::new()
            .layer(FollowRedirectLayer::new(Action::follow()))
            .service_fn(handle);
        let req = Request::builder()
            .uri("http://example.com/42")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.into_body(), 0);
    }

    #[tokio::test]
    async fn stops() {
        let svc = ServiceBuilder::new()
            .layer(FollowRedirectLayer::new(Action::stop()))
            .service_fn(handle);
        let req = Request::builder()
            .uri("http://example.com/42")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.into_body(), 42);
    }

    #[tokio::test]
    async fn limited() {
        let svc = ServiceBuilder::new()
            .layer(FollowRedirectLayer::new(Limited::new(10)))
            .service_fn(handle);
        let req = Request::builder()
            .uri("http://example.com/42")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.into_body(), 42 - 10);
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
