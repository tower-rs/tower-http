#![allow(unused_imports)]

use crate::accept_encoding::AcceptEncoding;
#[cfg(feature = "decompression-br")]
use async_compression::tokio::bufread::BrotliDecoder;
#[cfg(feature = "decompression-gzip")]
use async_compression::tokio::bufread::GzipDecoder;
#[cfg(feature = "decompression-deflate")]
use async_compression::tokio::bufread::ZlibDecoder;
use bytes::{Buf, Bytes, BytesMut};
use futures_core::Stream;
use futures_util::ready;
use http::{
    header::{self, CONTENT_ENCODING, CONTENT_LENGTH},
    HeaderMap, Response,
};
use http_body::Body;
use pin_project::pin_project;
use std::{fmt, marker::PhantomData, task::Context};
use std::{io, pin::Pin, task::Poll};
use tokio_util::{
    codec::{BytesCodec, FramedRead},
    io::StreamReader,
};

/// Response body of [`Decompression`].
///
/// [`Decompression`]: super::Decompression
#[pin_project]
pub struct DecompressionBody<B: Body> {
    #[pin]
    inner: BodyInner<B, B::Error>,
}

impl<B> DecompressionBody<B>
where
    B: Body,
{
    /// Gets a reference to the underlying body.
    pub fn get_ref(&self) -> &B {
        match &self.inner {
            BodyInner::Identity(inner, _) => inner,
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip(inner) => &inner.get_ref().get_ref().get_ref().body,
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate(inner) => &inner.get_ref().get_ref().get_ref().body,
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli(inner) => &inner.get_ref().get_ref().get_ref().body,
        }
    }

    /// Gets a mutable reference to the underlying body.
    ///
    /// It is inadvisable to directly read from the underlying body.
    pub fn get_mut(&mut self) -> &mut B {
        match &mut self.inner {
            BodyInner::Identity(inner, _) => inner,
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip(inner) => &mut inner.get_mut().get_mut().get_mut().body,
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate(inner) => &mut inner.get_mut().get_mut().get_mut().body,
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli(inner) => &mut inner.get_mut().get_mut().get_mut().body,
        }
    }

    /// Gets a pinned mutable reference to the underlying body.
    ///
    /// It is inadvisable to directly read from the underlying body.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        match self.project().inner.project() {
            BodyInnerProj::Identity(inner, _) => inner,
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip(inner) => {
                inner
                    .get_pin_mut()
                    .get_pin_mut()
                    .get_pin_mut()
                    .project()
                    .body
            }
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate(inner) => {
                inner
                    .get_pin_mut()
                    .get_pin_mut()
                    .get_pin_mut()
                    .project()
                    .body
            }
            #[cfg(feature = "decompression-br")]
            BodyInnerProj::Brotli(inner) => {
                inner
                    .get_pin_mut()
                    .get_pin_mut()
                    .get_pin_mut()
                    .project()
                    .body
            }
        }
    }

    /// Comsumes `self`, returning the underlying body.
    ///
    /// Note that any leftover data in the internal buffer is lost.
    pub fn into_inner(self) -> B {
        match self.inner {
            BodyInner::Identity(inner, _) => inner,
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip(inner) => inner.into_inner().into_inner().into_inner().body,
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate(inner) => inner.into_inner().into_inner().into_inner().body,
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli(inner) => inner.into_inner().into_inner().into_inner().body,
        }
    }

    #[allow(unused_variables, unreachable_code)]
    pub(crate) fn wrap_response(res: Response<B>, accept: &AcceptEncoding) -> Response<Self> {
        let (mut parts, body) = res.into_parts();

        if let header::Entry::Occupied(entry) = parts.headers.entry(CONTENT_ENCODING) {
            let body = match entry.get().as_bytes() {
                #[cfg(feature = "decompression-gzip")]
                b"gzip" if accept.gzip() => DecompressionBody::gzip(body),
                #[cfg(feature = "decompression-deflate")]
                b"deflate" if accept.deflate() => DecompressionBody::deflate(body),
                #[cfg(feature = "decompression-br")]
                b"br" if accept.br() => DecompressionBody::br(body),

                _ => return Response::from_parts(parts, DecompressionBody::identity(body)),
            };

            entry.remove();
            parts.headers.remove(CONTENT_LENGTH);

            Response::from_parts(parts, body)
        } else {
            Response::from_parts(parts, DecompressionBody::identity(body))
        }
    }

    fn identity(body: B) -> Self {
        DecompressionBody {
            inner: BodyInner::Identity(body, PhantomData),
        }
    }

    #[cfg(feature = "decompression-gzip")]
    fn gzip(body: B) -> Self {
        let read = StreamReader::new(Adapter { body, error: None });
        let inner = FramedRead::new(GzipDecoder::new(read), BytesCodec::new());
        DecompressionBody {
            inner: BodyInner::Gzip(inner),
        }
    }

    #[cfg(feature = "decompression-deflate")]
    fn deflate(body: B) -> Self {
        let read = StreamReader::new(Adapter { body, error: None });
        let inner = FramedRead::new(ZlibDecoder::new(read), BytesCodec::new());
        DecompressionBody {
            inner: BodyInner::Deflate(inner),
        }
    }

    #[cfg(feature = "decompression-br")]
    fn br(body: B) -> Self {
        let read = StreamReader::new(Adapter { body, error: None });
        let inner = FramedRead::new(BrotliDecoder::new(read), BytesCodec::new());
        DecompressionBody {
            inner: BodyInner::Brotli(inner),
        }
    }
}

impl<B> Body for DecompressionBody<B>
where
    B: Body,
{
    type Data = Bytes;
    type Error = Error<B::Error>;

    #[allow(unused_variables, unreachable_code)]
    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let (bytes, inner): (_, Pin<&mut Adapter<B, <B as Body>::Error>>) =
            match self.project().inner.project() {
                #[cfg(feature = "decompression-gzip")]
                BodyInnerProj::Gzip(mut framed_read) => (
                    ready!(framed_read.as_mut().poll_next(cx)),
                    framed_read.get_pin_mut().get_pin_mut().get_pin_mut(),
                ),
                #[cfg(feature = "decompression-deflate")]
                BodyInnerProj::Deflate(mut framed_read) => (
                    ready!(framed_read.as_mut().poll_next(cx)),
                    framed_read.get_pin_mut().get_pin_mut().get_pin_mut(),
                ),
                #[cfg(feature = "decompression-br")]
                BodyInnerProj::Brotli(mut framed_read) => (
                    ready!(framed_read.as_mut().poll_next(cx)),
                    framed_read.get_pin_mut().get_pin_mut().get_pin_mut(),
                ),

                BodyInnerProj::Identity(inner, _) => {
                    return match ready!(inner.poll_data(cx)) {
                        Some(Ok(mut data)) => {
                            Poll::Ready(Some(Ok(data.copy_to_bytes(data.remaining()))))
                        }
                        Some(Err(e)) => Poll::Ready(Some(Err(Error::Body(e)))),
                        None => Poll::Ready(None),
                    };
                }
            };

        match bytes {
            Some(Ok(data)) => Poll::Ready(Some(Ok(BytesMut::freeze(data)))),
            Some(Err(err)) => {
                let err = inner
                    .project()
                    .error
                    .take()
                    .map(Error::Body)
                    .unwrap_or(Error::Decompress(err));
                Poll::Ready(Some(Err(err)))
            }
            None => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        match self.project().inner.project() {
            BodyInnerProj::Identity(inner, _) => inner.poll_trailers(cx),
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip(inner) => inner
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate(inner) => inner
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            #[cfg(feature = "decompression-br")]
            BodyInnerProj::Brotli(inner) => inner
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
        }
        .map_err(Error::Body)
    }
}

// Manually implement `Debug` because the `derive(Debug)` macro cannot figure out that
// `B::Error: Debug` is required.
impl<B> fmt::Debug for DecompressionBody<B>
where
    B: Body + fmt::Debug,
    B::Error: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        struct DecompressionBody<'a, B: Body> {
            inner: &'a BodyInner<B, B::Error>,
        }
        DecompressionBody { inner: &self.inner }.fmt(f)
    }
}

#[pin_project(project = BodyInnerProj)]
#[derive(Debug)]
enum BodyInner<B, E> {
    Identity(#[pin] B, PhantomData<fn() -> E>),
    #[cfg(feature = "decompression-gzip")]
    Gzip(#[pin] FramedRead<GzipDecoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
    #[cfg(feature = "decompression-deflate")]
    Deflate(#[pin] FramedRead<ZlibDecoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
    #[cfg(feature = "decompression-br")]
    Brotli(#[pin] FramedRead<BrotliDecoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
}

/// A stream adapter that captures the errors from the `Body` for later inspection.
///
/// This is needed since the `io::Read` wrappers do not provide direct access to
/// the inner `Body::Error` values.
#[pin_project]
#[derive(Debug)]
struct Adapter<B, E> {
    #[pin]
    body: B,
    error: Option<E>,
}

impl<B: Body> Stream for Adapter<B, B::Error> {
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match ready!(this.body.poll_data(cx)) {
            Some(Ok(mut data)) => Poll::Ready(Some(Ok(data.copy_to_bytes(data.remaining())))),
            Some(Err(e)) => {
                *this.error = Some(e);
                // Return a placeholder, which should be discarded by the outer `DecompressionBody`.
                Poll::Ready(Some(Err(io::Error::from_raw_os_error(0))))
            }
            None => Poll::Ready(None),
        }
    }
}

/// Error type of [`DecompressionBody`].
#[derive(Debug)]
pub enum Error<E> {
    /// Error from the underlying body.
    Body(E),
    /// Decompression error.
    Decompress(io::Error),
}

impl<E: fmt::Display> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self {
            Error::Body(e) => e.fmt(f),
            Error::Decompress(e) => fmt::Display::fmt(&e, f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self {
            Error::Body(e) => Some(e),
            Error::Decompress(e) => Some(e),
        }
    }
}
