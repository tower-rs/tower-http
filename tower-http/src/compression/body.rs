#![allow(unused_imports)]

use crate::{
    compression_utils::{AsyncReadBody, BodyIntoStream, DecorateAsyncRead, WrapBody},
    BodyOrIoError,
};
#[cfg(feature = "compression-br")]
use async_compression::tokio::bufread::BrotliEncoder;
#[cfg(feature = "compression-gzip")]
use async_compression::tokio::bufread::GzipEncoder;
#[cfg(feature = "compression-deflate")]
use async_compression::tokio::bufread::ZlibEncoder;
use bytes::{Buf, Bytes};
use futures_util::ready;
use http::HeaderMap;
use http_body::Body;
use pin_project::pin_project;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio_util::io::StreamReader;

/// Response body of [`Compression`].
///
/// [`Compression`]: super::Compression
#[pin_project]
pub struct CompressionBody<B>(#[pin] pub(crate) BodyInner<B>)
where
    B: Body;

impl<B> CompressionBody<B>
where
    B: Body,
{
    /// Get a reference to the inner body
    pub fn get_ref(&self) -> &B {
        match &self.0 {
            #[cfg(feature = "compression-gzip")]
            BodyInner::Gzip(inner) => inner.read.get_ref().get_ref().get_ref().get_ref(),
            #[cfg(feature = "compression-deflate")]
            BodyInner::Deflate(inner) => inner.read.get_ref().get_ref().get_ref().get_ref(),
            #[cfg(feature = "compression-br")]
            BodyInner::Brotli(inner) => inner.read.get_ref().get_ref().get_ref().get_ref(),
            BodyInner::Identity(inner) => inner,
        }
    }

    /// Get a mutable reference to the inner body
    pub fn get_mut(&mut self) -> &mut B {
        match &mut self.0 {
            #[cfg(feature = "compression-gzip")]
            BodyInner::Gzip(inner) => inner.read.get_mut().get_mut().get_mut().get_mut(),
            #[cfg(feature = "compression-deflate")]
            BodyInner::Deflate(inner) => inner.read.get_mut().get_mut().get_mut().get_mut(),
            #[cfg(feature = "compression-br")]
            BodyInner::Brotli(inner) => inner.read.get_mut().get_mut().get_mut().get_mut(),
            BodyInner::Identity(inner) => inner,
        }
    }

    /// Get a pinned mutable reference to the inner body
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        match self.project().0.project() {
            #[cfg(feature = "compression-gzip")]
            BodyInnerProj::Gzip(inner) => inner
                .project()
                .read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut(),
            #[cfg(feature = "compression-deflate")]
            BodyInnerProj::Deflate(inner) => inner
                .project()
                .read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut(),
            #[cfg(feature = "compression-br")]
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
            #[cfg(feature = "compression-gzip")]
            BodyInner::Gzip(inner) => inner
                .read
                .into_inner()
                .into_inner()
                .into_inner()
                .into_inner(),
            #[cfg(feature = "compression-deflate")]
            BodyInner::Deflate(inner) => inner
                .read
                .into_inner()
                .into_inner()
                .into_inner()
                .into_inner(),
            #[cfg(feature = "compression-br")]
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
    #[cfg(feature = "compression-gzip")]
    Gzip(#[pin] WrapBody<GzipEncoder<B>>),
    #[cfg(feature = "compression-deflate")]
    Deflate(#[pin] WrapBody<ZlibEncoder<B>>),
    #[cfg(feature = "compression-br")]
    Brotli(#[pin] WrapBody<BrotliEncoder<B>>),
    Identity(#[pin] B),
}

impl<B> Body for CompressionBody<B>
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
            #[cfg(feature = "compression-gzip")]
            BodyInnerProj::Gzip(inner) => inner.poll_data(cx),
            #[cfg(feature = "compression-deflate")]
            BodyInnerProj::Deflate(inner) => inner.poll_data(cx),
            #[cfg(feature = "compression-br")]
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
            #[cfg(feature = "compression-gzip")]
            BodyInnerProj::Gzip(inner) => inner.poll_trailers(cx),
            #[cfg(feature = "compression-deflate")]
            BodyInnerProj::Deflate(inner) => inner.poll_trailers(cx),
            #[cfg(feature = "compression-br")]
            BodyInnerProj::Brotli(inner) => inner.poll_trailers(cx),
            BodyInnerProj::Identity(body) => body.poll_trailers(cx).map_err(BodyOrIoError::Body),
        }
    }
}

#[cfg(feature = "compression-gzip")]
impl<B> DecorateAsyncRead for GzipEncoder<B>
where
    B: Body,
{
    type Input = AsyncReadBody<B>;
    type Output = GzipEncoder<Self::Input>;

    fn apply(input: Self::Input) -> Self::Output {
        GzipEncoder::new(input)
    }

    fn get_pin_mut(pinned: Pin<&mut Self::Output>) -> Pin<&mut Self::Input> {
        pinned.get_pin_mut()
    }
}

#[cfg(feature = "compression-deflate")]
impl<B> DecorateAsyncRead for ZlibEncoder<B>
where
    B: Body,
{
    type Input = AsyncReadBody<B>;
    type Output = ZlibEncoder<Self::Input>;

    fn apply(input: Self::Input) -> Self::Output {
        ZlibEncoder::new(input)
    }

    fn get_pin_mut(pinned: Pin<&mut Self::Output>) -> Pin<&mut Self::Input> {
        pinned.get_pin_mut()
    }
}

#[cfg(feature = "compression-br")]
impl<B> DecorateAsyncRead for BrotliEncoder<B>
where
    B: Body,
{
    type Input = AsyncReadBody<B>;
    type Output = BrotliEncoder<Self::Input>;

    fn apply(input: Self::Input) -> Self::Output {
        BrotliEncoder::new(input)
    }

    fn get_pin_mut(pinned: Pin<&mut Self::Output>) -> Pin<&mut Self::Input> {
        pinned.get_pin_mut()
    }
}
