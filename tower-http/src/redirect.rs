//! Service that redirects all requests.

use http::{header, HeaderValue, Request, Response, StatusCode, Uri};
use std::{
    convert::Infallible,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use tower_service::Service;

/// Service that redirects all requests.
#[derive(Clone, Debug)]
pub struct Redirect<ResBody> {
    status_code: StatusCode,
    uri: Uri,
    _marker: PhantomData<fn() -> ResBody>,
}

impl<ResBody> Redirect<ResBody> {
    /// Create a new [`Redirect`] that uses a `307 Temporary Redirect` status code.
    pub fn temporary(uri: Uri) -> Self {
        Self::with_status_code(StatusCode::TEMPORARY_REDIRECT, uri)
    }

    /// Create a new [`Redirect`] that uses a `308 Permanent Redirect` status code.
    pub fn permanent(uri: Uri) -> Self {
        Self::with_status_code(StatusCode::PERMANENT_REDIRECT, uri)
    }

    /// Create a new [`Redirect`] that uses the given status code.
    ///
    /// # Panics
    ///
    /// Panics if `status_code` isn't a redirection status code (3xx).
    pub fn with_status_code(status_code: StatusCode, uri: Uri) -> Self {
        assert!(
            status_code.is_redirection(),
            "not a redirection status code"
        );

        Self {
            status_code,
            uri,
            _marker: PhantomData,
        }
    }
}

impl<ReqBody, ResBody> Service<Request<ReqBody>> for Redirect<ResBody>
where
    ResBody: Default,
{
    type Response = Response<ResBody>;
    type Error = Infallible;
    type Future = ResponseFuture<ResBody>;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Request<ReqBody>) -> Self::Future {
        ResponseFuture {
            status_code: self.status_code,
            uri: Some(self.uri.clone()),
            _marker: PhantomData,
        }
    }
}

/// Response future of [`Redirect`].
#[derive(Debug)]
pub struct ResponseFuture<ResBody> {
    uri: Option<Uri>,
    status_code: StatusCode,
    _marker: PhantomData<fn() -> ResBody>,
}

impl<ResBody> Future for ResponseFuture<ResBody>
where
    ResBody: Default,
{
    type Output = Result<Response<ResBody>, Infallible>;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut res = Response::default();

        *res.status_mut() = self.status_code;

        res.headers_mut().insert(
            header::LOCATION,
            HeaderValue::from_str(&self.uri.take().unwrap().to_string())
                .expect("URI isn't a valid header value"),
        );

        Poll::Ready(Ok(res))
    }
}
