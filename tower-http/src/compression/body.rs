#![allow(unused_imports)]

use super::Encoding;
#[cfg(feature = "compression-br")]
use async_compression::tokio::bufread::BrotliEncoder;
#[cfg(feature = "compression-gzip")]
use async_compression::tokio::bufread::GzipEncoder;
#[cfg(feature = "compression-deflate")]
use async_compression::tokio::bufread::ZlibEncoder;
use bytes::{Buf, Bytes, BytesMut};
use futures_core::Stream;
use futures_util::ready;
use http::{header, HeaderValue, Response};
use http_body::Body;
use pin_project::pin_project;
use std::{
    fmt, io,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use tokio_util::{
    codec::{BytesCodec, FramedRead},
    io::StreamReader,
};

/// Response body of [`Compression`].
///
/// [`Compression`]: super::Compression
#[pin_project]
pub struct CompressionBody<B: Body> {
    #[pin]
    inner: BodyInner<B, B::Error>,
}

impl<B> CompressionBody<B>
where
    B: Body,
{
    /// Gets a reference to the underlying body.
    pub fn get_ref(&self) -> &B {
        match &self.inner {
            BodyInner::Identity(inner, _) => inner,
            #[cfg(feature = "compression-gzip")]
            BodyInner::Gzip(inner) => &inner.get_ref().get_ref().get_ref().body,
            #[cfg(feature = "compression-deflate")]
            BodyInner::Deflate(inner) => &inner.get_ref().get_ref().get_ref().body,
            #[cfg(feature = "compression-br")]
            BodyInner::Brotli(inner) => &inner.get_ref().get_ref().get_ref().body,
        }
    }

    /// Gets a mutable reference to the underlying body.
    ///
    /// It is inadvisable to directly read from the underlying body.
    pub fn get_mut(&mut self) -> &mut B {
        match &mut self.inner {
            BodyInner::Identity(inner, _) => inner,
            #[cfg(feature = "compression-gzip")]
            BodyInner::Gzip(inner) => &mut inner.get_mut().get_mut().get_mut().body,
            #[cfg(feature = "compression-deflate")]
            BodyInner::Deflate(inner) => &mut inner.get_mut().get_mut().get_mut().body,
            #[cfg(feature = "compression-br")]
            BodyInner::Brotli(inner) => &mut inner.get_mut().get_mut().get_mut().body,
        }
    }

    /// Gets a pinned mutable reference to the underlying body.
    ///
    /// It is inadvisable to directly read from the underlying body.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        match self.project().inner.project() {
            BodyInnerProj::Identity(inner, _) => inner,
            #[cfg(feature = "compression-gzip")]
            BodyInnerProj::Gzip(inner) => {
                inner
                    .get_pin_mut()
                    .get_pin_mut()
                    .get_pin_mut()
                    .project()
                    .body
            }
            #[cfg(feature = "compression-deflate")]
            BodyInnerProj::Deflate(inner) => {
                inner
                    .get_pin_mut()
                    .get_pin_mut()
                    .get_pin_mut()
                    .project()
                    .body
            }
            #[cfg(feature = "compression-br")]
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
            #[cfg(feature = "compression-gzip")]
            BodyInner::Gzip(inner) => inner.into_inner().into_inner().into_inner().body,
            #[cfg(feature = "compression-deflate")]
            BodyInner::Deflate(inner) => inner.into_inner().into_inner().into_inner().body,
            #[cfg(feature = "compression-br")]
            BodyInner::Brotli(inner) => inner.into_inner().into_inner().into_inner().body,
            BodyInner::Identity(inner, _) => inner,
        }
    }

    #[allow(unused_variables, unused_mut, unreachable_code)]
    pub(crate) fn wrap_response(res: Response<B>, encoding: Encoding) -> Response<Self> {
        let (mut parts, body) = res.into_parts();

        let body = match encoding {
            #[cfg(feature = "compression-gzip")]
            Encoding::Gzip => {
                let read = StreamReader::new(Adapter { body, error: None });
                let framed_read = FramedRead::new(GzipEncoder::new(read), BytesCodec::new());
                CompressionBody {
                    inner: BodyInner::Gzip(framed_read),
                }
            }
            #[cfg(feature = "compression-deflate")]
            Encoding::Deflate => {
                let read = StreamReader::new(Adapter { body, error: None });
                let framed_read = FramedRead::new(ZlibEncoder::new(read), BytesCodec::new());
                CompressionBody {
                    inner: BodyInner::Deflate(framed_read),
                }
            }
            #[cfg(feature = "compression-br")]
            Encoding::Brotli => {
                let read = StreamReader::new(Adapter { body, error: None });
                let framed_read = FramedRead::new(BrotliEncoder::new(read), BytesCodec::new());
                CompressionBody {
                    inner: BodyInner::Brotli(framed_read),
                }
            }
            Encoding::Identity => {
                return Response::from_parts(
                    parts,
                    CompressionBody {
                        inner: BodyInner::Identity(body, PhantomData),
                    },
                )
            }
        };

        parts.headers.remove(header::CONTENT_LENGTH);

        parts.headers.insert(
            header::CONTENT_ENCODING,
            HeaderValue::from_str(encoding.to_str()).unwrap(),
        );

        http::Response::from_parts(parts, body)
    }
}

impl<B> Body for CompressionBody<B>
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
                #[cfg(feature = "compression-gzip")]
                BodyInnerProj::Gzip(mut framed_read) => (
                    ready!(framed_read.as_mut().poll_next(cx)),
                    framed_read.get_pin_mut().get_pin_mut().get_pin_mut(),
                ),
                #[cfg(feature = "compression-deflate")]
                BodyInnerProj::Deflate(mut framed_read) => (
                    ready!(framed_read.as_mut().poll_next(cx)),
                    framed_read.get_pin_mut().get_pin_mut().get_pin_mut(),
                ),
                #[cfg(feature = "compression-br")]
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
                    .unwrap_or(Error::Compress(err));
                Poll::Ready(Some(Err(err)))
            }
            None => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        match self.project().inner.project() {
            #[cfg(feature = "compression-gzip")]
            BodyInnerProj::Gzip(framed_read) => framed_read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            #[cfg(feature = "compression-deflate")]
            BodyInnerProj::Deflate(framed_read) => framed_read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            #[cfg(feature = "compression-br")]
            BodyInnerProj::Brotli(framed_read) => framed_read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            BodyInnerProj::Identity(inner, _) => inner.poll_trailers(cx),
        }
        .map_err(Error::Body)
    }
}

#[pin_project(project = BodyInnerProj)]
#[derive(Debug)]
enum BodyInner<B, E> {
    #[cfg(feature = "compression-gzip")]
    Gzip(#[pin] FramedRead<GzipEncoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
    #[cfg(feature = "compression-deflate")]
    Deflate(#[pin] FramedRead<ZlibEncoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
    #[cfg(feature = "compression-br")]
    Brotli(#[pin] FramedRead<BrotliEncoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
    Identity(#[pin] B, PhantomData<fn() -> E>),
}

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

                // Return a placeholder, which should be discarded by the outer `CompressBody`.
                Poll::Ready(Some(Err(io::Error::from_raw_os_error(0))))
            }
            None => Poll::Ready(None),
        }
    }
}

/// Error type of [`CompressionBody`].
#[derive(Debug)]
pub enum Error<E> {
    /// Error from the underlying body.
    Body(E),
    /// Compression error.
    Compress(io::Error),
}

impl<E> fmt::Display for Error<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Body(err) => err.fmt(f),
            Error::Compress(err) => err.fmt(f),
        }
    }
}

// TODO(david): impl `fn source`
impl<E> std::error::Error for Error<E> where E: std::error::Error {}
