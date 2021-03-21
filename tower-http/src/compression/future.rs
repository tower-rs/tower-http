#![allow(unused_imports)]

use super::{body::BodyInner, CompressionBody, Encoding};
use crate::compression_utils::{BoxError, WrapBody};
use futures_util::ready;
use http::{header, HeaderMap, HeaderValue, Response};
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
{
    type Output = Result<Response<CompressionBody<B>>, E>;

    #[allow(unreachable_code, unused_mut, unused_variables)]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.as_mut().project().inner.poll(cx)?);

        let (mut parts, body) = res.into_parts();

        let body = match (
            dbg!(supports_transparent_compression(&parts.headers)),
            self.encoding,
        ) {
            // if compression is _not_ support or the client doesn't accept it
            (false, _) | (_, Encoding::Identity) => {
                return Poll::Ready(Ok(Response::from_parts(
                    parts,
                    CompressionBody(BodyInner::Identity(body)),
                )))
            }

            #[cfg(feature = "compression-gzip")]
            (_, Encoding::Gzip) => CompressionBody(BodyInner::Gzip(WrapBody::new(body))),
            #[cfg(feature = "compression-deflate")]
            (_, Encoding::Deflate) => CompressionBody(BodyInner::Deflate(WrapBody::new(body))),
            #[cfg(feature = "compression-br")]
            (_, Encoding::Brotli) => CompressionBody(BodyInner::Brotli(WrapBody::new(body))),
        };

        parts.headers.remove(header::CONTENT_LENGTH);

        parts
            .headers
            .insert(header::CONTENT_ENCODING, self.encoding.into_header_value());

        let res = Response::from_parts(parts, body);
        Poll::Ready(Ok(res))
    }
}

#[allow(clippy::clippy::needless_bool)]
fn supports_transparent_compression(response_headers: &HeaderMap) -> bool {
    let content_type = if let Some(content_type) = content_type(response_headers) {
        content_type
    } else {
        return true;
    };

    if content_type == "application/grpc" {
        // grpc doesn't support transparent compression and instead has its compression own
        // algorithm that implementations can use
        // https://grpc.github.io/grpc/core/md_doc_compression.html
        false
    } else {
        // for now just say that all non-grpc requests support compression
        true
    }
}

fn content_type(headers: &HeaderMap) -> Option<&str> {
    let content_type = headers.get(http::header::CONTENT_TYPE)?;
    content_type.to_str().ok()
}
