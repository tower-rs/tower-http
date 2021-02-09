use super::{CompressionBody, Encoding};
use futures_util::ready;
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Response future of [`Compression`].
///
/// [`Compression`]: super::Compression
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F> {
    #[pin]
    pub(crate) inner: F,
    pub(crate) encoding: Encoding,
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<http::Response<B>, E>>,
    B: Body,
{
    type Output = Result<http::Response<CompressionBody<B>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let res = ready!(this.inner.poll(cx)?);
        Poll::Ready(Ok(CompressionBody::wrap_response(res, *this.encoding)))
    }
}
