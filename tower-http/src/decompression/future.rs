use super::DecompressionBody;
use crate::accept_encoding::AcceptEncoding;
use futures_util::ready;
use http::Response;
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Response future of [`Decompression`].
///
/// [`Decompression`]: super::Decompression
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F> {
    #[pin]
    pub(crate) inner: F,
    pub(crate) accept: AcceptEncoding,
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: Body,
{
    type Output = Result<Response<DecompressionBody<B>>, E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.as_mut().project().inner.poll(cx)?);
        Poll::Ready(Ok(DecompressionBody::wrap_response(res, &self.accept)))
    }
}
