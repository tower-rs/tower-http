//! Types used by compression and decompression middlewares.

use bytes::{Bytes, BytesMut};
use futures_core::Stream;
use futures_util::ready;
use http::HeaderValue;
use http_body::Body;
use pin_project::pin_project;
use std::{
    io,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::AsyncRead;
use tokio_util::io::{poll_read_buf, StreamReader};

#[derive(Debug, Clone, Copy)]
pub(crate) struct AcceptEncoding {
    pub(crate) gzip: bool,
    pub(crate) deflate: bool,
    pub(crate) br: bool,
}

impl AcceptEncoding {
    #[allow(dead_code)]
    pub(crate) fn to_header_value(&self) -> Option<HeaderValue> {
        let accept = match (self.gzip(), self.deflate(), self.br()) {
            (true, true, true) => "gzip,deflate,br",
            (true, true, false) => "gzip,deflate",
            (true, false, true) => "gzip,br",
            (true, false, false) => "gzip",
            (false, true, true) => "deflate,br",
            (false, true, false) => "deflate",
            (false, false, true) => "br",
            (false, false, false) => return None,
        };
        Some(HeaderValue::from_static(accept))
    }

    #[allow(dead_code)]
    pub(crate) fn gzip(&self) -> bool {
        #[cfg(any(feature = "decompression-gzip", feature = "compression-gzip"))]
        {
            self.gzip
        }
        #[cfg(not(any(feature = "decompression-gzip", feature = "compression-gzip")))]
        {
            false
        }
    }

    #[allow(dead_code)]
    pub(crate) fn deflate(&self) -> bool {
        #[cfg(any(feature = "decompression-deflate", feature = "compression-deflate"))]
        {
            self.deflate
        }
        #[cfg(not(any(feature = "decompression-deflate", feature = "compression-deflate")))]
        {
            false
        }
    }

    #[allow(dead_code)]
    pub(crate) fn br(&self) -> bool {
        #[cfg(any(feature = "decompression-br", feature = "compression-br"))]
        {
            self.br
        }
        #[cfg(not(any(feature = "decompression-br", feature = "compression-br")))]
        {
            false
        }
    }

    #[allow(dead_code)]
    pub(crate) fn set_gzip(mut self, enable: bool) -> Self {
        self.gzip = enable;
        self
    }

    #[allow(dead_code)]
    pub(crate) fn set_deflate(mut self, enable: bool) -> Self {
        self.deflate = enable;
        self
    }

    #[allow(dead_code)]
    pub(crate) fn set_br(mut self, enable: bool) -> Self {
        self.br = enable;
        self
    }
}

impl Default for AcceptEncoding {
    fn default() -> Self {
        AcceptEncoding {
            gzip: true,
            deflate: true,
            br: true,
        }
    }
}

/// Trait for applying some decorator to an `AsyncRead`
pub(crate) trait DecorateAsyncRead {
    type Input: AsyncRead;
    type Output: AsyncRead;

    /// Apply the decorator
    fn apply(input: Self::Input) -> Self::Output;

    /// Get a pinned mutable reference to the original input.
    ///
    /// This is necessary to implement `Body::poll_trailers`.
    fn get_pin_mut(pinned: Pin<&mut Self::Output>) -> Pin<&mut Self::Input>;
}

/// `Body` that has been decorated by an `AsyncRead`
#[pin_project]
pub(crate) struct WrapBody<B, M: DecorateAsyncRead> {
    #[pin]
    pub(crate) read: M::Output,
    _marker: PhantomData<B>,
}

impl<B, R, M> WrapBody<B, M>
where
    B: Body,
    B::Error: Into<io::Error>,
    M: DecorateAsyncRead<Input = StreamReader<BodyIntoStream<B>, B::Data>, Output = R>,
{
    #[allow(dead_code)]
    pub(crate) fn new(body: B) -> Self {
        // convert `Body` into a `Stream`
        let stream = BodyIntoStream::new(body);
        // convert `Stream` into an `AsyncRead`
        let read = StreamReader::new(stream);
        // apply decorator to `AsyncRead` yieling another `AsyncRead`
        let read = M::apply(read);

        Self {
            read,
            _marker: PhantomData,
        }
    }
}

impl<B, M> Body for WrapBody<B, M>
where
    B: Body,
    B::Error: Into<io::Error>,
    M: DecorateAsyncRead<Input = StreamReader<BodyIntoStream<B>, B::Data>>,
{
    type Data = Bytes;
    type Error = io::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let mut buf = BytesMut::new();
        let read = ready!(poll_read_buf(self.project().read, cx, &mut buf)?);
        if read == 0 {
            Poll::Ready(None)
        } else {
            Poll::Ready(Some(Ok(buf.freeze())))
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let this = self.project();
        let body = M::get_pin_mut(this.read).get_pin_mut().get_pin_mut();
        body.poll_trailers(cx).map_err(Into::into)
    }
}

// When https://github.com/hyperium/http-body/pull/36 is merged we can remove this
#[pin_project]
pub(crate) struct BodyIntoStream<B> {
    #[pin]
    body: B,
}

#[allow(dead_code)]
impl<B> BodyIntoStream<B> {
    pub(crate) fn new(body: B) -> Self {
        Self { body }
    }

    /// Get a reference to the inner body
    pub(crate) fn get_ref(&self) -> &B {
        &self.body
    }

    /// Get a mutable reference to the inner body
    pub(crate) fn get_mut(&mut self) -> &mut B {
        &mut self.body
    }

    /// Get a pinned mutable reference to the inner body
    pub(crate) fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        self.project().body
    }

    /// Consume `self`, returning the inner body
    pub(crate) fn into_inner(self) -> B {
        self.body
    }
}

impl<B> Stream for BodyIntoStream<B>
where
    B: Body,
{
    type Item = Result<B::Data, B::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().body.poll_data(cx)
    }
}

// When https://github.com/hyperium/http-body/pull/36 is merged we can remove this
#[pin_project]
pub(crate) struct BodyMapErr<B, F> {
    #[pin]
    inner: B,
    f: F,
}

impl<B, F> BodyMapErr<B, F> {
    #[inline]
    pub(crate) fn new(body: B, f: F) -> Self {
        Self { inner: body, f }
    }

    /// Get a reference to the inner body
    pub(crate) fn get_ref(&self) -> &B {
        &self.inner
    }

    /// Get a mutable reference to the inner body
    pub(crate) fn get_mut(&mut self) -> &mut B {
        &mut self.inner
    }

    /// Get a pinned mutable reference to the inner body
    pub(crate) fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        self.project().inner
    }

    /// Consume `self`, returning the inner body
    pub(crate) fn into_inner(self) -> B {
        self.inner
    }
}

impl<B, F, E> Body for BodyMapErr<B, F>
where
    B: Body,
    F: FnMut(B::Error) -> E,
{
    type Data = B::Data;
    type Error = E;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        match this.inner.poll_data(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Ok(data))) => Poll::Ready(Some(Ok(data))),
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err((this.f)(err)))),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let this = self.project();
        match this.inner.poll_trailers(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(None)) => Poll::Ready(Ok(None)),
            Poll::Ready(Ok(Some(headers))) => Poll::Ready(Ok(Some(headers))),
            Poll::Ready(Err(err)) => Poll::Ready(Err((this.f)(err))),
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

pub(crate) fn into_io_error<E>(err: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::Other, err)
}
