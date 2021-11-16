#![allow(unused_imports)]

use crate::{
    compression_utils::{AsyncReadBody, BodyIntoStream, DecorateAsyncRead, WrapBody},
    BoxError,
};
#[cfg(feature = "decompression-br")]
use async_compression::tokio::bufread::BrotliDecoder;
#[cfg(feature = "decompression-gzip")]
use async_compression::tokio::bufread::GzipDecoder;
#[cfg(feature = "decompression-deflate")]
use async_compression::tokio::bufread::ZlibDecoder;
use bytes::{Buf, Bytes};
use futures_util::ready;
use http::HeaderMap;
use http_body::Body;
use pin_project_lite::pin_project;
use std::task::Context;
use std::{io, marker::PhantomData, pin::Pin, task::Poll};
use tokio_util::io::StreamReader;

pin_project! {
    /// Response body of [`Decompression`].
    ///
    /// [`Decompression`]: super::Decompression
    pub struct DecompressionBody<B>
    where
        B: Body
    {
        #[pin]
        pub(crate) inner: BodyInner<B>,
    }
}

impl<B> DecompressionBody<B>
where
    B: Body,
{
    pub(crate) fn new(inner: BodyInner<B>) -> Self {
        Self { inner }
    }

    /// Get a reference to the inner body
    pub fn get_ref(&self) -> &B {
        match &self.inner {
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip { inner } => inner.read.get_ref().get_ref().get_ref().get_ref(),
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate { inner } => inner.read.get_ref().get_ref().get_ref().get_ref(),
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli { inner } => inner.read.get_ref().get_ref().get_ref().get_ref(),
            BodyInner::Identity { inner } => inner,

            // FIXME: Remove once possible; see https://github.com/rust-lang/rust/issues/51085
            #[cfg(not(feature = "decompression-gzip"))]
            BodyInner::Gzip { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-deflate"))]
            BodyInner::Deflate { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-br"))]
            BodyInner::Brotli { inner } => match inner.0 {},
        }
    }

    /// Get a mutable reference to the inner body
    pub fn get_mut(&mut self) -> &mut B {
        match &mut self.inner {
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip { inner } => inner.read.get_mut().get_mut().get_mut().get_mut(),
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate { inner } => inner.read.get_mut().get_mut().get_mut().get_mut(),
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli { inner } => inner.read.get_mut().get_mut().get_mut().get_mut(),
            BodyInner::Identity { inner } => inner,

            #[cfg(not(feature = "decompression-gzip"))]
            BodyInner::Gzip { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-deflate"))]
            BodyInner::Deflate { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-br"))]
            BodyInner::Brotli { inner } => match inner.0 {},
        }
    }

    /// Get a pinned mutable reference to the inner body
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        match self.project().inner.project() {
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip { inner } => inner
                .project()
                .read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut(),
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate { inner } => inner
                .project()
                .read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut(),
            #[cfg(feature = "decompression-br")]
            BodyInnerProj::Brotli { inner } => inner
                .project()
                .read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut(),
            BodyInnerProj::Identity { inner } => inner,

            #[cfg(not(feature = "decompression-gzip"))]
            BodyInnerProj::Gzip { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-deflate"))]
            BodyInnerProj::Deflate { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-br"))]
            BodyInnerProj::Brotli { inner } => match inner.0 {},
        }
    }

    /// Consume `self`, returning the inner body
    pub fn into_inner(self) -> B {
        match self.inner {
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip { inner } => inner
                .read
                .into_inner()
                .into_inner()
                .into_inner()
                .into_inner(),
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate { inner } => inner
                .read
                .into_inner()
                .into_inner()
                .into_inner()
                .into_inner(),
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli { inner } => inner
                .read
                .into_inner()
                .into_inner()
                .into_inner()
                .into_inner(),
            BodyInner::Identity { inner } => inner,

            #[cfg(not(feature = "decompression-gzip"))]
            BodyInner::Gzip { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-deflate"))]
            BodyInner::Deflate { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-br"))]
            BodyInner::Brotli { inner } => match inner.0 {},
        }
    }
}

#[cfg(any(
    not(feature = "decompression-gzip"),
    not(feature = "decompression-deflate"),
    not(feature = "decompression-br")
))]
pub(crate) enum Never {}

#[cfg(feature = "decompression-gzip")]
type GzipBody<B> = WrapBody<GzipDecoder<B>>;
#[cfg(not(feature = "decompression-gzip"))]
type GzipBody<B> = (Never, PhantomData<B>);

#[cfg(feature = "decompression-deflate")]
type DeflateBody<B> = WrapBody<ZlibDecoder<B>>;
#[cfg(not(feature = "decompression-deflate"))]
type DeflateBody<B> = (Never, PhantomData<B>);

#[cfg(feature = "decompression-br")]
type BrotliBody<B> = WrapBody<BrotliDecoder<B>>;
#[cfg(not(feature = "decompression-br"))]
type BrotliBody<B> = (Never, PhantomData<B>);

pin_project! {
    #[project = BodyInnerProj]
    pub(crate) enum BodyInner<B>
    where
        B: Body,
    {
        Gzip {
            #[pin]
            inner: GzipBody<B>,
        },
        Deflate {
            #[pin]
            inner: DeflateBody<B>,
        },
        Brotli {
            #[pin]
            inner: BrotliBody<B>,
        },
        Identity {
            #[pin]
            inner: B,
        },
    }
}

impl<B: Body> BodyInner<B> {
    #[cfg(feature = "decompression-gzip")]
    pub(crate) fn gzip(inner: WrapBody<GzipDecoder<B>>) -> Self {
        Self::Gzip { inner }
    }

    #[cfg(feature = "decompression-deflate")]
    pub(crate) fn deflate(inner: WrapBody<ZlibDecoder<B>>) -> Self {
        Self::Deflate { inner }
    }

    #[cfg(feature = "decompression-br")]
    pub(crate) fn brotli(inner: WrapBody<BrotliDecoder<B>>) -> Self {
        Self::Brotli { inner }
    }

    pub(crate) fn identity(inner: B) -> Self {
        Self::Identity { inner }
    }
}

impl<B> Body for DecompressionBody<B>
where
    B: Body,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.project().inner.project() {
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip { inner } => inner.poll_data(cx),
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate { inner } => inner.poll_data(cx),
            #[cfg(feature = "decompression-br")]
            BodyInnerProj::Brotli { inner } => inner.poll_data(cx),
            BodyInnerProj::Identity { inner } => match ready!(inner.poll_data(cx)) {
                Some(Ok(mut buf)) => {
                    let bytes = buf.copy_to_bytes(buf.remaining());
                    Poll::Ready(Some(Ok(bytes)))
                }
                Some(Err(err)) => Poll::Ready(Some(Err(err.into()))),
                None => Poll::Ready(None),
            },

            #[cfg(not(feature = "decompression-gzip"))]
            BodyInnerProj::Gzip { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-deflate"))]
            BodyInnerProj::Deflate { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-br"))]
            BodyInnerProj::Brotli { inner } => match inner.0 {},
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        match self.project().inner.project() {
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip { inner } => inner.poll_trailers(cx),
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate { inner } => inner.poll_trailers(cx),
            #[cfg(feature = "decompression-br")]
            BodyInnerProj::Brotli { inner } => inner.poll_trailers(cx),
            BodyInnerProj::Identity { inner } => inner.poll_trailers(cx).map_err(Into::into),

            #[cfg(not(feature = "decompression-gzip"))]
            BodyInnerProj::Gzip { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-deflate"))]
            BodyInnerProj::Deflate { inner } => match inner.0 {},
            #[cfg(not(feature = "decompression-br"))]
            BodyInnerProj::Brotli { inner } => match inner.0 {},
        }
    }
}

#[cfg(feature = "decompression-gzip")]
impl<B> DecorateAsyncRead for GzipDecoder<B>
where
    B: Body,
{
    type Input = AsyncReadBody<B>;
    type Output = GzipDecoder<Self::Input>;

    fn apply(input: Self::Input) -> Self::Output {
        GzipDecoder::new(input)
    }

    fn get_pin_mut(pinned: Pin<&mut Self::Output>) -> Pin<&mut Self::Input> {
        pinned.get_pin_mut()
    }
}

#[cfg(feature = "decompression-deflate")]
impl<B> DecorateAsyncRead for ZlibDecoder<B>
where
    B: Body,
{
    type Input = AsyncReadBody<B>;
    type Output = ZlibDecoder<Self::Input>;

    fn apply(input: Self::Input) -> Self::Output {
        ZlibDecoder::new(input)
    }

    fn get_pin_mut(pinned: Pin<&mut Self::Output>) -> Pin<&mut Self::Input> {
        pinned.get_pin_mut()
    }
}

#[cfg(feature = "decompression-br")]
impl<B> DecorateAsyncRead for BrotliDecoder<B>
where
    B: Body,
{
    type Input = AsyncReadBody<B>;
    type Output = BrotliDecoder<Self::Input>;

    fn apply(input: Self::Input) -> Self::Output {
        BrotliDecoder::new(input)
    }

    fn get_pin_mut(pinned: Pin<&mut Self::Output>) -> Pin<&mut Self::Input> {
        pinned.get_pin_mut()
    }
}
