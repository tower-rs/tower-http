#![allow(unused_imports)]

use super::{body::BodyInner, CompressionBody, Encoding};
use crate::compression_utils::{into_io_error, BodyMapErr, WrapBody};
use futures_util::ready;
use http::{header, HeaderValue, Response};
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
    F: Future<Output = Result<Response<B>, E>>,
    B: Body,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Output = Result<Response<CompressionBody<B>>, E>;

    #[allow(unreachable_code, unused_mut, unused_variables)]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.as_mut().project().inner.poll(cx)?);

        let (mut parts, body) = res.into_parts();

        let body = BodyMapErr::new(body, into_io_error as _);

        let body = match self.encoding {
            #[cfg(feature = "compression-gzip")]
            Encoding::Gzip => CompressionBody(BodyInner::Gzip(WrapBody::new(body))),
            #[cfg(feature = "compression-deflate")]
            Encoding::Deflate => CompressionBody(BodyInner::Deflate(WrapBody::new(body))),
            #[cfg(feature = "compression-br")]
            Encoding::Brotli => CompressionBody(BodyInner::Brotli(WrapBody::new(body))),
            Encoding::Identity => {
                return Poll::Ready(Ok(Response::from_parts(
                    parts,
                    CompressionBody(BodyInner::Identity(body)),
                )))
            }
        };

        parts.headers.remove(header::CONTENT_LENGTH);

        parts.headers.insert(
            header::CONTENT_ENCODING,
            HeaderValue::from_str(self.encoding.to_str()).unwrap(),
        );

        let res = Response::from_parts(parts, body);
        Poll::Ready(Ok(res))
    }
}
