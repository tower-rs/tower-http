//! Propagate a header from the request to the response.

use futures_util::ready;
use http::{header::HeaderName, HeaderValue, Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`PropagateHeader`] which propagates headers from requests to responses.
///
/// See [`PropagateHeader`] for more details.
#[derive(Clone, Debug)]
pub struct PropagateHeaderLayer {
    header: HeaderName,
}

impl PropagateHeaderLayer {
    /// Create a new [`PropagateHeaderLayer`].
    pub fn new(header: HeaderName) -> Self {
        Self { header }
    }
}

impl<S> Layer<S> for PropagateHeaderLayer {
    type Service = PropagateHeader<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PropagateHeader {
            inner,
            header: self.header.clone(),
        }
    }
}

/// Middleware that propagates headers from requests to responses.
///
/// So if the header is present on the request it'll be applied to the response as well. This could
/// for example be used to propagate headers such as `X-Request-Id`.
///
/// # Example
///
/// ```rust
/// use http::{Request, Response, header::HeaderName};
/// use std::convert::Infallible;
/// use tower::{Service, ServiceExt};
/// use tower_http::propagate_header::PropagateHeader;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let echo_service = tower::service_fn(|request: Request<String>| async move {
///     Ok::<_, Infallible>(Response::new(request.into_body()))
/// });
///
/// // This will copy `x-request-id` headers on requests onto responses.
/// let mut svc = PropagateHeader::new(echo_service, HeaderName::from_static("x-request-id"));
///
/// let request = Request::builder()
///     .header("x-request-id", "1337")
///     .body(String::new())?;
///
/// let response = svc.ready_and().await?.call(request).await?;
///
/// assert_eq!(response.headers()["x-request-id"], "1337");
/// #
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct PropagateHeader<S> {
    inner: S,
    header: HeaderName,
}

impl<S> PropagateHeader<S> {
    /// Create a new [`PropagateHeader`] that propagates the given header.
    pub fn new(inner: S, header: HeaderName) -> Self {
        Self { inner, header }
    }

    define_inner_service_accessors!();
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for PropagateHeader<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let value = req.headers().get(&self.header).cloned();

        ResponseFuture {
            future: self.inner.call(req),
            header_and_value: Some(self.header.clone()).zip(value),
        }
    }
}

/// Response future for [`PropagateHeader`].
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F> {
    #[pin]
    future: F,
    header_and_value: Option<(HeaderName, HeaderValue)>,
}

impl<F, ResBody, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        if let Some((header, value)) = this.header_and_value.take() {
            res.headers_mut().insert(header, value);
        }

        Poll::Ready(Ok(res))
    }
}
