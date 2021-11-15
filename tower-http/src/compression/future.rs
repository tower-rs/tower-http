#![allow(unused_imports)]

use super::{body::BodyInner, CompressionBody};
use crate::compression_utils::WrapBody;
use crate::content_encoding::Encoding;
use futures_util::ready;
use http::{header, HeaderMap, HeaderValue, Response};
use http_body::Body;
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use crate::compression::compression_predicate::CompressionPredicate;

/// Response future of [`Compression`].
///
/// [`Compression`]: super::Compression
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F, P> {
    #[pin]
    pub(crate) inner: F,
    pub(crate) encoding: Encoding,
    pub(crate) compression_predicate: P
}

impl<F, B, E, P> Future for ResponseFuture<F, P>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: Body,
    P: CompressionPredicate<B>,
{
    type Output = Result<Response<CompressionBody<B>>, E>;

    #[allow(unreachable_code, unused_mut, unused_variables)]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.as_mut().project().inner.poll(cx)?);

        let should_compress = self.compression_predicate.should_compress(&res);
        let (mut parts, body) = res.into_parts();


        let body = match (
            should_compress,
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
