#![allow(unused_imports)]

use crate::compression_utils::{AsyncReadBody, BodyIntoStream, DecorateAsyncRead, WrapBody};
use crate::BodyOrIoError;
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
use pin_project::pin_project;
use std::task::Context;
use std::{io, pin::Pin, task::Poll};
use tokio_util::io::StreamReader;

/// Response body of [`Decompression`].
///
/// [`Decompression`]: super::Decompression
#[pin_project]
pub struct DecompressionBody<B>(#[pin] pub(crate) BodyInner<B>)
where
    B: Body;

impl<B> DecompressionBody<B>
where
    B: Body,
{
    /// Get a reference to the inner body
    pub fn get_ref(&self) -> &B {
        match &self.0 {
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip(inner) => inner.read.get_ref().get_ref().get_ref().get_ref(),
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate(inner) => inner.read.get_ref().get_ref().get_ref().get_ref(),
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli(inner) => inner.read.get_ref().get_ref().get_ref().get_ref(),
            BodyInner::Identity(inner) => inner,
        }
    }

    /// Get a mutable reference to the inner body
    pub fn get_mut(&mut self) -> &mut B {
        match &mut self.0 {
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip(inner) => inner.read.get_mut().get_mut().get_mut().get_mut(),
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate(inner) => inner.read.get_mut().get_mut().get_mut().get_mut(),
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli(inner) => inner.read.get_mut().get_mut().get_mut().get_mut(),
            BodyInner::Identity(inner) => inner,
        }
    }

    /// Get a pinned mutable reference to the inner body
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        match self.project().0.project() {
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip(inner) => inner
                .project()
                .read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut(),
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate(inner) => inner
                .project()
                .read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut(),
            #[cfg(feature = "decompression-br")]
            BodyInnerProj::Brotli(inner) => inner
                .project()
                .read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut(),
            BodyInnerProj::Identity(inner) => inner,
        }
    }

    /// Consume `self`, returning the inner body
    pub fn into_inner(self) -> B {
        match self.0 {
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip(inner) => inner
                .read
                .into_inner()
                .into_inner()
                .into_inner()
                .into_inner(),
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate(inner) => inner
                .read
                .into_inner()
                .into_inner()
                .into_inner()
                .into_inner(),
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli(inner) => inner
                .read
                .into_inner()
                .into_inner()
                .into_inner()
                .into_inner(),
            BodyInner::Identity(inner) => inner,
        }
    }
}

#[pin_project(project = BodyInnerProj)]
pub(crate) enum BodyInner<B>
where
    B: Body,
{
    #[cfg(feature = "decompression-gzip")]
    Gzip(#[pin] WrapBody<GzipDecoder<B>>),
    #[cfg(feature = "decompression-deflate")]
    Deflate(#[pin] WrapBody<ZlibDecoder<B>>),
    #[cfg(feature = "decompression-br")]
    Brotli(#[pin] WrapBody<BrotliDecoder<B>>),
    Identity(#[pin] B),
}

impl<B> Body for DecompressionBody<B>
where
    B: Body,
{
    type Data = Bytes;
    type Error = BodyOrIoError<B::Error>;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.project().0.project() {
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip(inner) => inner.poll_data(cx),
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate(inner) => inner.poll_data(cx),
            #[cfg(feature = "decompression-br")]
            BodyInnerProj::Brotli(inner) => inner.poll_data(cx),
            BodyInnerProj::Identity(body) => match ready!(body.poll_data(cx)) {
                Some(Ok(mut buf)) => {
                    let bytes = buf.copy_to_bytes(buf.remaining());
                    Poll::Ready(Some(Ok(bytes)))
                }
                Some(Err(err)) => Poll::Ready(Some(Err(BodyOrIoError::Body(err)))),
                None => Poll::Ready(None),
            },
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        match self.project().0.project() {
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip(inner) => inner.poll_trailers(cx),
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate(inner) => inner.poll_trailers(cx),
            #[cfg(feature = "decompression-br")]
            BodyInnerProj::Brotli(inner) => inner.poll_trailers(cx),
            BodyInnerProj::Identity(body) => body.poll_trailers(cx).map_err(BodyOrIoError::Body),
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
