//! Remove a header from the request.
//!
//! See the root [`crate::set_header::request`] module for full documentation and usage examples.

use http::{header::HeaderName, Request, Response};
use std::task::{Context, Poll};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`RemoveRequestHeader`] which removes a request header.
///
/// See [`RemoveRequestHeader`] for more details.
#[derive(Debug, Clone)]
pub struct RemoveRequestHeaderLayer {
    header_name: HeaderName,
}

impl RemoveRequestHeaderLayer {
    /// Create a new [`RemoveRequestHeaderLayer`].
    pub fn new(header_name: HeaderName) -> Self {
        Self { header_name }
    }
}

impl<S> Layer<S> for RemoveRequestHeaderLayer {
    type Service = RemoveRequestHeader<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RemoveRequestHeader {
            inner,
            header_name: self.header_name.clone(),
        }
    }
}

/// Middleware that removes a header from the request.
///
/// All values of the header are removed. Removing a header that is not present
/// is a no-op.
///
/// See the [module docs](crate::set_header::request) for an example.
#[derive(Debug, Clone)]
pub struct RemoveRequestHeader<S> {
    inner: S,
    header_name: HeaderName,
}

impl<S> RemoveRequestHeader<S> {
    /// Create a new [`RemoveRequestHeader`].
    pub fn new(inner: S, header_name: HeaderName) -> Self {
        Self { inner, header_name }
    }

    define_inner_service_accessors!();
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for RemoveRequestHeader<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        req.headers_mut().remove(&self.header_name);
        self.inner.call(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::{self, HeaderName, HeaderValue};
    use std::convert::Infallible;
    use tower::{service_fn, Service, ServiceExt};

    #[tokio::test]
    async fn removes_all_values_of_the_header() {
        let x_forwarded_for = HeaderName::from_static("x-forwarded-for");

        let mut svc = RemoveRequestHeaderLayer::new(x_forwarded_for.clone()).layer(service_fn(
            |req: Request<()>| async move {
                assert!(req.headers().get("x-forwarded-for").is_none());
                Ok::<_, Infallible>(Response::new(()))
            },
        ));

        let mut request = Request::new(());
        request.headers_mut().append(
            x_forwarded_for.clone(),
            HeaderValue::from_static("203.0.113.7"),
        );
        request
            .headers_mut()
            .append(x_forwarded_for, HeaderValue::from_static("198.51.100.7"));

        svc.ready().await.unwrap().call(request).await.unwrap();
    }

    #[tokio::test]
    async fn leaves_other_headers_and_absent_header_alone() {
        let mut svc = RemoveRequestHeaderLayer::new(HeaderName::from_static("x-forwarded-for"))
            .layer(service_fn(|req: Request<()>| async move {
                assert_eq!(req.headers().get(header::USER_AGENT).unwrap(), "test-agent");
                Ok::<_, Infallible>(Response::new(()))
            }));

        // the header being removed is not present, so removal is a no-op
        let mut request = Request::new(());
        request
            .headers_mut()
            .insert(header::USER_AGENT, HeaderValue::from_static("test-agent"));

        svc.ready().await.unwrap().call(request).await.unwrap();
    }
}
