#![allow(unused_imports)]

use super::{body::BodyInner, DecompressionBody};
use crate::{
    compression_utils::{AcceptEncoding, WrapBody},
    BoxError,
};
use futures_util::ready;
use http::{header, Response};
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
    B::Error: Into<BoxError>,
{
    type Output = Result<Response<DecompressionBody<B>>, E>;

    #[allow(unreachable_code, unused_mut, unused_variables)]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.as_mut().project().inner.poll(cx)?);
        let (mut parts, body) = res.into_parts();

        let res =
            if let header::Entry::Occupied(entry) = parts.headers.entry(header::CONTENT_ENCODING) {
                let body = match entry.get().as_bytes() {
                    #[cfg(feature = "decompression-gzip")]
                    b"gzip" if self.accept.gzip() => {
                        DecompressionBody(BodyInner::Gzip(WrapBody::new(body)))
                    }

                    #[cfg(feature = "decompression-deflate")]
                    b"deflate" if self.accept.deflate() => {
                        DecompressionBody(BodyInner::Deflate(WrapBody::new(body)))
                    }

                    #[cfg(feature = "decompression-br")]
                    b"br" if self.accept.br() => {
                        DecompressionBody(BodyInner::Brotli(WrapBody::new(body)))
                    }

                    _ => {
                        return Poll::Ready(Ok(Response::from_parts(
                            parts,
                            DecompressionBody(BodyInner::Identity(body)),
                        )))
                    }
                };

                entry.remove();
                parts.headers.remove(header::CONTENT_LENGTH);

                Response::from_parts(parts, body)
            } else {
                Response::from_parts(parts, DecompressionBody(BodyInner::Identity(body)))
            };

        Poll::Ready(Ok(res))
    }
}
