#![allow(unused_imports)]

use crate::compression::CompressionLevel;
use crate::BoxError;
#[cfg(feature = "compression-br")]
use async_compression::tokio::write::BrotliEncoder;
#[cfg(feature = "compression-gzip")]
use async_compression::tokio::write::GzipEncoder;
#[cfg(feature = "compression-deflate")]
use async_compression::tokio::write::ZlibEncoder;
#[cfg(feature = "compression-zstd")]
use async_compression::tokio::write::ZstdEncoder;

use bytes::{Buf, Bytes, BytesMut};
use http_body::{Body, Frame};
use pin_project_lite::pin_project;
use std::{
    io,
    pin::Pin,
    task::{ready, Context, Poll},
};
use tokio::io::AsyncWrite;

use super::pin_project_cfg::pin_project_cfg;

pin_project! {
    /// Response body of [`Compression`].
    ///
    /// [`Compression`]: super::Compression
    pub struct CompressionBody<B>
    where
        B: Body,
    {
        #[pin]
        pub(crate) inner: BodyInner<B>,
    }
}

impl<B> Default for CompressionBody<B>
where
    B: Body + Default,
{
    fn default() -> Self {
        Self {
            inner: BodyInner::Identity {
                inner: B::default(),
            },
        }
    }
}

impl<B> CompressionBody<B>
where
    B: Body,
{
    pub(crate) fn new(inner: BodyInner<B>) -> Self {
        Self { inner }
    }

    /// Get a reference to the inner body
    pub fn get_ref(&self) -> &B {
        match &self.inner {
            #[cfg(feature = "compression-gzip")]
            BodyInner::Gzip { inner } => &inner.body,
            #[cfg(feature = "compression-deflate")]
            BodyInner::Deflate { inner } => &inner.body,
            #[cfg(feature = "compression-br")]
            BodyInner::Brotli { inner } => &inner.body,
            #[cfg(feature = "compression-zstd")]
            BodyInner::Zstd { inner } => &inner.body,
            BodyInner::Identity { inner } => inner,
        }
    }

    /// Get a mutable reference to the inner body
    pub fn get_mut(&mut self) -> &mut B {
        match &mut self.inner {
            #[cfg(feature = "compression-gzip")]
            BodyInner::Gzip { inner } => &mut inner.body,
            #[cfg(feature = "compression-deflate")]
            BodyInner::Deflate { inner } => &mut inner.body,
            #[cfg(feature = "compression-br")]
            BodyInner::Brotli { inner } => &mut inner.body,
            #[cfg(feature = "compression-zstd")]
            BodyInner::Zstd { inner } => &mut inner.body,
            BodyInner::Identity { inner } => inner,
        }
    }

    /// Get a pinned mutable reference to the inner body
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        match self.project().inner.project() {
            #[cfg(feature = "compression-gzip")]
            BodyInnerProj::Gzip { inner } => inner.project().body,
            #[cfg(feature = "compression-deflate")]
            BodyInnerProj::Deflate { inner } => inner.project().body,
            #[cfg(feature = "compression-br")]
            BodyInnerProj::Brotli { inner } => inner.project().body,
            #[cfg(feature = "compression-zstd")]
            BodyInnerProj::Zstd { inner } => inner.project().body,
            BodyInnerProj::Identity { inner } => inner,
        }
    }

    /// Consume `self`, returning the inner body
    pub fn into_inner(self) -> B {
        match self.inner {
            #[cfg(feature = "compression-gzip")]
            BodyInner::Gzip { inner } => inner.body,
            #[cfg(feature = "compression-deflate")]
            BodyInner::Deflate { inner } => inner.body,
            #[cfg(feature = "compression-br")]
            BodyInner::Brotli { inner } => inner.body,
            #[cfg(feature = "compression-zstd")]
            BodyInner::Zstd { inner } => inner.body,
            BodyInner::Identity { inner } => inner,
        }
    }
}

#[cfg(feature = "compression-gzip")]
type GzipBody<B> = EncodeBody<B, GzipEncoder<EncodedBuf>>;

#[cfg(feature = "compression-deflate")]
type DeflateBody<B> = EncodeBody<B, ZlibEncoder<EncodedBuf>>;

#[cfg(feature = "compression-br")]
type BrotliBody<B> = EncodeBody<B, BrotliEncoder<EncodedBuf>>;

#[cfg(feature = "compression-zstd")]
type ZstdBody<B> = EncodeBody<B, ZstdEncoder<EncodedBuf>>;

pin_project_cfg! {
    #[project = BodyInnerProj]
    pub(crate) enum BodyInner<B>
    where
        B: Body,
    {
        #[cfg(feature = "compression-gzip")]
        Gzip {
            #[pin]
            inner: GzipBody<B>,
        },
        #[cfg(feature = "compression-deflate")]
        Deflate {
            #[pin]
            inner: DeflateBody<B>,
        },
        #[cfg(feature = "compression-br")]
        Brotli {
            #[pin]
            inner: BrotliBody<B>,
        },
        #[cfg(feature = "compression-zstd")]
        Zstd {
            #[pin]
            inner: ZstdBody<B>,
        },
        Identity {
            #[pin]
            inner: B,
        },
    }
}

impl<B: Body> BodyInner<B> {
    #[cfg(feature = "compression-gzip")]
    pub(crate) fn gzip(inner: GzipBody<B>) -> Self {
        Self::Gzip { inner }
    }

    #[cfg(feature = "compression-deflate")]
    pub(crate) fn deflate(inner: DeflateBody<B>) -> Self {
        Self::Deflate { inner }
    }

    #[cfg(feature = "compression-br")]
    pub(crate) fn brotli(inner: BrotliBody<B>) -> Self {
        Self::Brotli { inner }
    }

    #[cfg(feature = "compression-zstd")]
    pub(crate) fn zstd(inner: ZstdBody<B>) -> Self {
        Self::Zstd { inner }
    }

    pub(crate) fn identity(inner: B) -> Self {
        Self::Identity { inner }
    }
}

impl<B> Body for CompressionBody<B>
where
    B: Body,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        match self.project().inner.project() {
            #[cfg(feature = "compression-gzip")]
            BodyInnerProj::Gzip { inner } => inner.poll_frame(cx),
            #[cfg(feature = "compression-deflate")]
            BodyInnerProj::Deflate { inner } => inner.poll_frame(cx),
            #[cfg(feature = "compression-br")]
            BodyInnerProj::Brotli { inner } => inner.poll_frame(cx),
            #[cfg(feature = "compression-zstd")]
            BodyInnerProj::Zstd { inner } => inner.poll_frame(cx),
            BodyInnerProj::Identity { inner } => match ready!(inner.poll_frame(cx)) {
                Some(Ok(frame)) => {
                    let frame = frame.map_data(|mut buf| buf.copy_to_bytes(buf.remaining()));
                    Poll::Ready(Some(Ok(frame)))
                }
                Some(Err(err)) => Poll::Ready(Some(Err(err.into()))),
                None => Poll::Ready(None),
            },
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        if let BodyInner::Identity { inner } = &self.inner {
            inner.size_hint()
        } else {
            http_body::SizeHint::new()
        }
    }

    fn is_end_stream(&self) -> bool {
        if let BodyInner::Identity { inner } = &self.inner {
            inner.is_end_stream()
        } else {
            false
        }
    }
}

/// In-memory sink that the encoders write their compressed output into.
///
/// It is always ready, so driving an encoder that writes into it never yields `Poll::Pending`:
/// once `poll_write`, `poll_flush` or `poll_shutdown` returns, everything the encoder produced is
/// available through [`EncodedBuf::take`].
#[derive(Debug, Default)]
pub(crate) struct EncodedBuf {
    buf: BytesMut,
}

impl EncodedBuf {
    /// Take the compressed bytes written so far, if any.
    fn take(&mut self) -> Option<Bytes> {
        if self.buf.is_empty() {
            None
        } else {
            Some(self.buf.split().freeze())
        }
    }
}

impl AsyncWrite for EncodedBuf {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.buf.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// A compression encoder that writes its output into an [`EncodedBuf`].
///
/// Using the `AsyncWrite` flavor of the `async_compression` encoders (rather than the
/// `AsyncRead` one) gives [`EncodeBody`] access to `poll_flush`, which is what lets it push
/// compressed data out before the inner body has produced more input.
pub(crate) trait Encoder: AsyncWrite {
    fn new(quality: CompressionLevel) -> Self;

    /// The buffer holding the compressed bytes produced so far.
    fn output(self: Pin<&mut Self>) -> &mut EncodedBuf;
}

pin_project! {
    /// `Body` whose data frames are compressed by an [`Encoder`].
    ///
    /// Compressed bytes are handed out as soon as they are available. In particular, whenever
    /// the inner body has no data ready (`Poll::Pending`), whatever has been written to the
    /// encoder since the last flush is flushed out of it and yielded before this body returns
    /// `Poll::Pending` itself. This keeps streaming responses, such as server-sent events,
    /// flowing instead of having their chunks held back inside the encoder until enough further
    /// input arrives to fill its internal buffers.
    pub(crate) struct EncodeBody<B, E>
    where
        B: Body,
    {
        #[pin]
        body: B,
        #[pin]
        encoder: E,
        // A data frame from the inner body that has not been fully written to the encoder yet.
        data: Option<B::Data>,
        // A non-data frame (trailers) that arrived while the encoder was still open. It is
        // yielded once the encoder has been finished and its remaining output emitted.
        non_data_frame: Option<Frame<B::Data>>,
        // Whether bytes have been written to the encoder since it was last flushed.
        unflushed: bool,
        // Whether the inner body has returned `None`. It must not be polled again after that.
        body_done: bool,
        state: State,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Data frames from the inner body are written to the encoder.
    Encoding,
    /// The inner body has no more data frames; the encoder is being finished.
    Finishing,
    /// The compressed data has been emitted in full. Remaining frames (trailers) are passed
    /// through from the inner body untouched.
    PassThrough,
}

impl<B, E> EncodeBody<B, E>
where
    B: Body,
    E: Encoder,
{
    #[allow(dead_code)]
    pub(crate) fn new(body: B, quality: CompressionLevel) -> Self {
        Self {
            body,
            encoder: E::new(quality),
            data: None,
            non_data_frame: None,
            unflushed: false,
            body_done: false,
            state: State::Encoding,
        }
    }
}

impl<B, E> Body for EncodeBody<B, E>
where
    B: Body,
    B::Error: Into<BoxError>,
    E: Encoder,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();

        loop {
            match *this.state {
                State::Encoding => {
                    // Write the data frame that is currently being processed, if any.
                    if let Some(data) = this.data {
                        while data.has_remaining() {
                            let written =
                                ready!(this.encoder.as_mut().poll_write(cx, data.chunk()))?;
                            if written == 0 {
                                return Poll::Ready(Some(Err(io::Error::new(
                                    io::ErrorKind::WriteZero,
                                    "compression encoder did not accept any data",
                                )
                                .into())));
                            }
                            data.advance(written);
                            *this.unflushed = true;
                        }
                        *this.data = None;

                        // Yield whatever the encoder decided to emit on its own.
                        if let Some(chunk) = this.encoder.as_mut().output().take() {
                            return Poll::Ready(Some(Ok(Frame::data(chunk))));
                        }
                    }

                    match this.body.as_mut().poll_frame(cx) {
                        Poll::Pending => {
                            // The inner body is waiting for more data. Don't hold back what
                            // has already been compressed: flush it out and yield it.
                            if *this.unflushed {
                                ready!(this.encoder.as_mut().poll_flush(cx))?;
                                *this.unflushed = false;
                                if let Some(chunk) = this.encoder.as_mut().output().take() {
                                    return Poll::Ready(Some(Ok(Frame::data(chunk))));
                                }
                            }
                            return Poll::Pending;
                        }
                        Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                            Ok(data) => *this.data = Some(data),
                            Err(frame) => {
                                *this.non_data_frame = Some(frame);
                                *this.state = State::Finishing;
                            }
                        },
                        Poll::Ready(Some(Err(err))) => {
                            return Poll::Ready(Some(Err(err.into())));
                        }
                        Poll::Ready(None) => {
                            *this.body_done = true;
                            *this.state = State::Finishing;
                        }
                    }
                }

                State::Finishing => {
                    ready!(this.encoder.as_mut().poll_shutdown(cx))?;
                    *this.state = State::PassThrough;
                    if let Some(chunk) = this.encoder.as_mut().output().take() {
                        return Poll::Ready(Some(Ok(Frame::data(chunk))));
                    }
                }

                State::PassThrough => {
                    if let Some(frame) = this.non_data_frame.take() {
                        return Poll::Ready(Some(Ok(
                            frame.map_data(|mut data| data.copy_to_bytes(data.remaining()))
                        )));
                    }

                    if *this.body_done {
                        return Poll::Ready(None);
                    }

                    // Yield any remaining frames in the body. There shouldn't be any after the
                    // trailers but you never know.
                    return match ready!(this.body.as_mut().poll_frame(cx)) {
                        Some(Ok(frame)) => Poll::Ready(Some(Ok(
                            frame.map_data(|mut data| data.copy_to_bytes(data.remaining()))
                        ))),
                        Some(Err(err)) => Poll::Ready(Some(Err(err.into()))),
                        None => {
                            *this.body_done = true;
                            Poll::Ready(None)
                        }
                    };
                }
            }
        }
    }
}

#[cfg(feature = "compression-gzip")]
impl Encoder for GzipEncoder<EncodedBuf> {
    fn new(quality: CompressionLevel) -> Self {
        GzipEncoder::with_quality(EncodedBuf::default(), quality.into_async_compression())
    }

    fn output(self: Pin<&mut Self>) -> &mut EncodedBuf {
        self.get_pin_mut().get_mut()
    }
}

#[cfg(feature = "compression-deflate")]
impl Encoder for ZlibEncoder<EncodedBuf> {
    fn new(quality: CompressionLevel) -> Self {
        ZlibEncoder::with_quality(EncodedBuf::default(), quality.into_async_compression())
    }

    fn output(self: Pin<&mut Self>) -> &mut EncodedBuf {
        self.get_pin_mut().get_mut()
    }
}

#[cfg(feature = "compression-br")]
impl Encoder for BrotliEncoder<EncodedBuf> {
    fn new(quality: CompressionLevel) -> Self {
        // The brotli crate used under the hood here has a default compression level of 11,
        // which is the max for brotli. This causes extremely slow compression times, so we
        // manually set a default of 4 here.
        //
        // This is the same default used by NGINX for on-the-fly brotli compression.
        let level = match quality {
            CompressionLevel::Default => async_compression::Level::Precise(4),
            other => other.into_async_compression(),
        };
        BrotliEncoder::with_quality(EncodedBuf::default(), level)
    }

    fn output(self: Pin<&mut Self>) -> &mut EncodedBuf {
        self.get_pin_mut().get_mut()
    }
}

#[cfg(feature = "compression-zstd")]
impl Encoder for ZstdEncoder<EncodedBuf> {
    fn new(quality: CompressionLevel) -> Self {
        // See https://issues.chromium.org/issues/41493659:
        //  "For memory usage reasons, Chromium limits the window size to 8MB"
        // See https://datatracker.ietf.org/doc/html/rfc8878#name-window-descriptor
        //  "For improved interoperability, it's recommended for decoders to support values
        //  of Window_Size up to 8 MB and for encoders not to generate frames requiring a
        //  Window_Size larger than 8 MB."
        // Level 17 in zstd (as of v1.5.6) is the first level with a window size of 8 MB (2^23):
        // https://github.com/facebook/zstd/blob/v1.5.6/lib/compress/clevels.h#L25-L51
        // Set the parameter for all levels >= 17. This will either have no effect (but reduce
        // the risk of future changes in zstd) or limit the window log to 8MB.
        let needs_window_limit = match quality {
            CompressionLevel::Best => true, // level 20
            CompressionLevel::Precise(level) => level >= 17,
            _ => false,
        };
        // The parameter is not set for levels below 17 as it will increase the window size
        // for those levels.
        if needs_window_limit {
            let params = [async_compression::zstd::CParameter::window_log(23)];
            ZstdEncoder::with_quality_and_params(
                EncodedBuf::default(),
                quality.into_async_compression(),
                &params,
            )
        } else {
            ZstdEncoder::with_quality(EncodedBuf::default(), quality.into_async_compression())
        }
    }

    fn output(self: Pin<&mut Self>) -> &mut EncodedBuf {
        self.get_pin_mut().get_mut()
    }
}
