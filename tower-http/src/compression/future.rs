#![allow(unused_imports)]

use super::{body::BodyInner, CompressionBody};
use crate::compression_utils::WrapBody;
use crate::content_encoding::Encoding;
use futures_util::ready;
use http::{header, HeaderMap, HeaderValue, Response};
use http_body::Body;
use pin_project_lite::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use crate::compression::compression_filter::CompressionFilter;

pin_project! {
    /// Response future of [`Compression`].
    ///
    /// [`Compression`]: super::Compression
    #[derive(Debug)]
    pub struct ResponseFuture<F, P> {
        #[pin]
        pub(crate) inner: F,
        pub(crate) encoding: Encoding,
        pub(crate) compression_filter: P
    }
}

impl<F, B, E, P> Future for ResponseFuture<F, P>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: Body,
    P: CompressionFilter,
{
    type Output = Result<Response<CompressionBody<B>>, E>;

    #[allow(unreachable_code, unused_mut, unused_variables)]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.as_mut().project().inner.poll(cx)?);

        let (mut parts, body) = res.into_parts();

        let body = match (
            self.compression_filter.should_compress(&parts),
            self.encoding,
        ) {
            // if compression is _not_ support or the client doesn't accept it
            (false, _) | (_, Encoding::Identity) => {
                return Poll::Ready(Ok(Response::from_parts(
                    parts,
                    CompressionBody::new(BodyInner::identity(body)),
                )))
            }

            #[cfg(feature = "compression-gzip")]
            (_, Encoding::Gzip) => CompressionBody::new(BodyInner::gzip(WrapBody::new(body))),
            #[cfg(feature = "compression-deflate")]
            (_, Encoding::Deflate) => CompressionBody::new(BodyInner::deflate(WrapBody::new(body))),
            #[cfg(feature = "compression-br")]
            (_, Encoding::Brotli) => CompressionBody::new(BodyInner::brotli(WrapBody::new(body))),
        };

        parts.headers.remove(header::CONTENT_LENGTH);

        parts
            .headers
            .insert(header::CONTENT_ENCODING, self.encoding.into_header_value());

        let res = Response::from_parts(parts, body);
        Poll::Ready(Ok(res))
    }
}
