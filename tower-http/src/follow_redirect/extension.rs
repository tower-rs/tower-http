//! The [`FollowRedirectExtension`] middleware works just like [`super::FollowRedirect`]
//! and also stores a copy of the [`Policy`] in a [`FollowedPolicy`] extension.
//! see [`FollowRedirect`](super) for usage.

use super::policy::{Policy, Standard};
use super::RedirectingRequest;
use futures_util::future::Either;
use http::{Request, Response};
use http_body::Body;
use pin_project_lite::pin_project;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use tower::util::Oneshot;
use tower::{Layer, Service};

/// [`Layer`] for retrying requests with a [`Service`] to follow redirection responses.
///
/// See the [module docs](self) for more details.
#[derive(Clone, Copy, Debug, Default)]
pub struct FollowRedirectExtensionLayer<P = Standard> {
    policy: P,
}

impl FollowRedirectExtensionLayer {
    /// Create a new [`FollowRedirectExtension`] with a [`Standard`] redirection policy.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<P> FollowRedirectExtensionLayer<P> {
    /// Create a new [`FollowRedirectExtension`] with the given redirection [`Policy`].
    pub fn with_policy(policy: P) -> Self {
        Self { policy }
    }
}

impl<S, P> Layer<S> for FollowRedirectExtensionLayer<P>
where
    S: Clone,
    P: Clone + Send + Sync + 'static,
{
    type Service = FollowRedirectExtension<S, P>;

    fn layer(&self, inner: S) -> Self::Service {
        FollowRedirectExtension::with_policy(inner, self.policy.clone())
    }
}

/// Middleware that retries requests with a [`Service`] to follow redirection responses.
/// Stores the redirect [`Policy`] that was run before the last request of the redirect chain
/// in the [`FollowedPolicy`] [extension](http::Extensions)
///
/// See the [module docs](super) for more details.
#[derive(Clone, Copy, Debug)]
pub struct FollowRedirectExtension<S, P = Standard> {
    inner: S,
    policy: P,
}

impl<S> FollowRedirectExtension<S> {
    /// Create a new [`FollowRedirectExtension`] with a [`Standard`] redirection policy.
    pub fn new(inner: S) -> Self {
        Self::with_policy(inner, Standard::default())
    }

    /// Returns a new [`Layer`] that wraps services with a [`FollowRedirectExtension`] middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer() -> FollowRedirectExtensionLayer {
        FollowRedirectExtensionLayer::new()
    }
}

impl<S, P> FollowRedirectExtension<S, P>
where
    P: Clone + Send + Sync + 'static,
{
    /// Create a new [`FollowRedirectExtension`] with the given redirection [`Policy`].
    pub fn with_policy(inner: S, policy: P) -> Self {
        FollowRedirectExtension { inner, policy }
    }

    /// Returns a new [`Layer`] that wraps services with a [`FollowRedirectExtension`] middleware
    /// with the given redirection [`Policy`].
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer_with_policy(policy: P) -> FollowRedirectExtensionLayer<P> {
        FollowRedirectExtensionLayer::with_policy(policy)
    }

    define_inner_service_accessors!();
}

impl<ReqBody, ResBody, S, P> Service<Request<ReqBody>> for FollowRedirectExtension<S, P>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ReqBody: Body + Default,
    P: Policy<ReqBody, S::Error> + Clone + Send + Sync + 'static,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = ResponseFuture<S, ReqBody, P>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let service = self.inner.clone();
        let mut request = RedirectingRequest::new(
            mem::replace(&mut self.inner, service),
            self.policy.clone(),
            &mut req,
        );
        ResponseFuture {
            future: Either::Left(request.service.call(req)),
            request,
        }
    }
}

/// Response [`Extensions`][http::Extensions] value that contains the redirect [`Policy`] that
/// was run before the last request of the redirect chain by a [`FollowRedirectExtension`] middleware.
#[derive(Clone)]
pub struct FollowedPolicy<P>(pub P);

pin_project! {
    /// Response future for [`FollowRedirectExtension`].
    #[derive(Debug)]
    pub struct ResponseFuture<S, B, P>
    where
        S: Service<Request<B>>,
    {
        #[pin]
        future: Either<S::Future, Oneshot<S, Request<B>>>,
        request: RedirectingRequest<S, B, P>
    }
}

impl<S, ReqBody, ResBody, P> Future for ResponseFuture<S, ReqBody, P>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ReqBody: Body + Default,
    P: Policy<ReqBody, S::Error> + Clone + Send + Sync + 'static,
{
    type Output = Result<Response<ResBody>, S::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let mut res = ready!(this.future.as_mut().poll(cx)?);

        res.extensions_mut()
            .insert(FollowedPolicy(this.request.policy.clone()));

        match this.request.handle_response(&mut res) {
            Ok(Some(pending)) => {
                this.future.set(Either::Right(pending));
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Ok(None) => Poll::Ready(Ok(res)),
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{policy::*, tests::handle, *};
    use super::*;
    use crate::test_helpers::Body;
    use tower::{ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn follows() {
        let svc = ServiceBuilder::new()
            .layer(FollowRedirectExtensionLayer::with_policy(Action::Follow))
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
            .layer(FollowRedirectExtensionLayer::with_policy(Action::Stop))
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
            .layer(FollowRedirectExtensionLayer::with_policy(Limited::new(10)))
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
}
