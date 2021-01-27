//! Middleware that decompresses response bodies.

#![cfg_attr(
    not(any(feature = "br", feature = "gzip", feature = "deflate")),
    allow(unused)
)]

use std::error;
use std::fmt::{self, Debug, Display, Formatter};
use std::future::Future;
use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "br")]
use async_compression::tokio::bufread::BrotliDecoder;
#[cfg(feature = "gzip")]
use async_compression::tokio::bufread::GzipDecoder;
#[cfg(feature = "deflate")]
use async_compression::tokio::bufread::ZlibDecoder;
use bytes::{Buf, Bytes, BytesMut};
use futures_core::{ready, Stream, TryFuture};
use http::header::{self, HeaderValue, ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, RANGE};
use http_body::Body;
use pin_project::pin_project;
use tokio_util::codec::{BytesCodec, FramedRead};
use tokio_util::io::StreamReader;
use tower_layer::Layer;
use tower_service::Service;

/// Decompresses response bodies of the underlying service.
///
/// This adds the `Accept-Encoding` header to requests and transparently decompresses response
/// bodies based on the `Content-Encoding` header.
#[derive(Debug, Clone)]
pub struct Decompress<S> {
    inner: S,
    accept: AcceptEncoding,
}

/// Decompresses response bodies of the underlying service.
///
/// This adds the `Accept-Encoding` header to requests and transparently decompresses response
/// bodies based on the `Content-Encoding` header.
#[derive(Debug, Default, Clone)]
pub struct DecompressLayer {
    accept: AcceptEncoding,
}

/// Response future of [`Decompress`].
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F> {
    #[pin]
    inner: F,
    accept: AcceptEncoding,
}

#[pin_project]
/// Response body of [`Decompress`].
pub struct DecompressBody<B: Body> {
    #[pin]
    inner: BodyInner<B, B::Error>,
}

/// Error type of [`DecompressBody`].
#[derive(Debug)]
pub enum Error<E> {
    /// Error from the underlying body.
    Body(E),
    /// Decompress error.
    Decompress(io::Error),
}

type BodyReader<B, E> = StreamReader<Adapter<B, E>, Bytes>;

#[pin_project(project = BodyInnerProj)]
#[derive(Debug)]
enum BodyInner<B, E> {
    Identity {
        #[pin]
        inner: B,
        marker: PhantomData<fn() -> E>,
    },
    #[cfg(feature = "gzip")]
    Gzip {
        #[pin]
        inner: FramedRead<GzipDecoder<BodyReader<B, E>>, BytesCodec>,
    },
    #[cfg(feature = "deflate")]
    Deflate {
        #[pin]
        inner: FramedRead<ZlibDecoder<BodyReader<B, E>>, BytesCodec>,
    },
    #[cfg(feature = "br")]
    Brotli {
        #[pin]
        inner: FramedRead<BrotliDecoder<BodyReader<B, E>>, BytesCodec>,
    },
}

/// A `TryStream<Error>` that captures the errors from the `Body` for later inspection.
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

#[derive(Debug, Clone, Copy)]
struct AcceptEncoding {
    #[cfg(feature = "gzip")]
    gzip: bool,
    #[cfg(feature = "deflate")]
    deflate: bool,
    #[cfg(feature = "br")]
    br: bool,
}

impl<S> Decompress<S> {
    /// Creates a new `Decompress` wrapping the `service`.
    pub fn new(service: S) -> Self {
        Decompress {
            inner: service,
            accept: AcceptEncoding::default(),
        }
    }

    /// Gets a reference to the underlying service.
    pub fn get_ref(&self) -> &S {
        &self.inner
    }

    /// Gets a mutable reference to the underlying service.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Consumes `self`, returning the underlying service.
    pub fn into_inner(self) -> S {
        self.inner
    }

    /// Sets whether to request the gzip encoding.
    #[cfg(feature = "gzip")]
    #[cfg_attr(docsrs, doc(cfg(feature = "gzip")))]
    pub fn gzip(self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to request the Deflate encoding.
    #[cfg(feature = "deflate")]
    #[cfg_attr(docsrs, doc(cfg(feature = "deflate")))]
    pub fn deflate(self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to request the Brotli encoding.
    #[cfg(feature = "br")]
    #[cfg_attr(docsrs, doc(cfg(feature = "br")))]
    pub fn br(self, enable: bool) -> Self {
        self.accept.set_br(enable);
        self
    }

    /// Disables the gzip encoding.
    ///
    /// This method is available even if the `gzip` crate feature is disabled.
    pub fn no_gzip(self) -> Self {
        self.accept.set_gzip(false);
        self
    }

    /// Disables the Deflate encoding.
    ///
    /// This method is available even if the `deflate` crate feature is disabled.
    pub fn no_deflate(self) -> Self {
        self.accept.set_deflate(false);
        self
    }

    /// Disables the Brotli encoding.
    ///
    /// This method is available even if the `br` crate feature is disabled.
    pub fn no_br(self) -> Self {
        self.accept.set_br(false);
        self
    }
}

impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for Decompress<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
    ResBody: Body,
{
    type Response = http::Response<DecompressBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<ReqBody>) -> Self::Future {
        if !req.headers().contains_key(RANGE) {
            if let header::Entry::Vacant(e) = req.headers_mut().entry(ACCEPT_ENCODING) {
                if let Some(accept) = self.accept.to_header_value() {
                    e.insert(accept);
                }
            }
        }
        ResponseFuture {
            inner: self.inner.call(req),
            accept: self.accept,
        }
    }
}

impl DecompressLayer {
    /// Creates a new `DecompressLayer`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets whether to request the gzip encoding.
    #[cfg(feature = "gzip")]
    #[cfg_attr(docsrs, doc(cfg(feature = "gzip")))]
    pub fn gzip(self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to request the Deflate encoding.
    #[cfg(feature = "deflate")]
    #[cfg_attr(docsrs, doc(cfg(feature = "deflate")))]
    pub fn deflate(self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to request the Brotli encoding.
    #[cfg(feature = "br")]
    #[cfg_attr(docsrs, doc(cfg(feature = "br")))]
    pub fn br(self, enable: bool) -> Self {
        self.accept.set_br(enable);
        self
    }

    /// Disables the gzip encoding.
    ///
    /// This method is available even if the `gzip` crate feature is disabled.
    pub fn no_gzip(self) -> Self {
        self.accept.set_gzip(false);
        self
    }

    /// Disables the Deflate encoding.
    ///
    /// This method is available even if the `deflate` crate feature is disabled.
    pub fn no_deflate(self) -> Self {
        self.accept.set_deflate(false);
        self
    }

    /// Disables the Brotli encoding.
    ///
    /// This method is available even if the `br` crate feature is disabled.
    pub fn no_br(self) -> Self {
        self.accept.set_br(false);
        self
    }
}

impl<S> Layer<S> for DecompressLayer {
    type Service = Decompress<S>;

    fn layer(&self, service: S) -> Self::Service {
        Decompress {
            inner: service,
            accept: self.accept,
        }
    }
}

impl<F, B> Future for ResponseFuture<F>
where
    F: TryFuture<Ok = http::Response<B>>,
    B: Body,
{
    type Output = Result<http::Response<DecompressBody<B>>, F::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.as_mut().project().inner.try_poll(cx)?);
        Poll::Ready(Ok(DecompressBody::wrap_response(res, &self.accept)))
    }
}

impl<B> DecompressBody<B>
where
    B: Body,
{
    /// Gets a reference to the underlying body.
    pub fn get_ref(&self) -> &B {
        match &self.inner {
            BodyInner::Identity { inner, .. } => inner,
            #[cfg(feature = "gzip")]
            BodyInner::Gzip { inner } => &inner.get_ref().get_ref().get_ref().body,
            #[cfg(feature = "deflate")]
            BodyInner::Deflate { inner } => &inner.get_ref().get_ref().get_ref().body,
            #[cfg(feature = "br")]
            BodyInner::Brotli { inner } => &inner.get_ref().get_ref().get_ref().body,
        }
    }

    /// Gets a mutable reference to the underlying body.
    ///
    /// It is inadvisable to directly read from the underlying body.
    pub fn get_mut(&mut self) -> &mut B {
        match &mut self.inner {
            BodyInner::Identity { inner, .. } => inner,
            #[cfg(feature = "gzip")]
            BodyInner::Gzip { inner } => &mut inner.get_mut().get_mut().get_mut().body,
            #[cfg(feature = "deflate")]
            BodyInner::Deflate { inner } => &mut inner.get_mut().get_mut().get_mut().body,
            #[cfg(feature = "br")]
            BodyInner::Brotli { inner } => &mut inner.get_mut().get_mut().get_mut().body,
        }
    }

    /// Gets a pinned mutable reference to the underlying body.
    ///
    /// It is inadvisable to directly read from the underlying body.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        match self.project().inner.project() {
            BodyInnerProj::Identity { inner, .. } => inner,
            #[cfg(feature = "gzip")]
            BodyInnerProj::Gzip { inner } => {
                inner
                    .get_pin_mut()
                    .get_pin_mut()
                    .get_pin_mut()
                    .project()
                    .body
            }
            #[cfg(feature = "deflate")]
            BodyInnerProj::Deflate { inner } => {
                inner
                    .get_pin_mut()
                    .get_pin_mut()
                    .get_pin_mut()
                    .project()
                    .body
            }
            #[cfg(feature = "br")]
            BodyInnerProj::Brotli { inner } => {
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
            BodyInner::Identity { inner, .. } => inner,
            #[cfg(feature = "gzip")]
            BodyInner::Gzip { inner } => inner.into_inner().into_inner().into_inner().body,
            #[cfg(feature = "deflate")]
            BodyInner::Deflate { inner } => inner.into_inner().into_inner().into_inner().body,
            #[cfg(feature = "br")]
            BodyInner::Brotli { inner } => inner.into_inner().into_inner().into_inner().body,
        }
    }

    #[cfg_attr(
        not(any(feature = "br", feature = "gzip", feature = "deflate")),
        allow(unreachable_code)
    )]
    fn wrap_response(res: http::Response<B>, accept: &AcceptEncoding) -> http::Response<Self> {
        let (mut parts, body) = res.into_parts();
        if let header::Entry::Occupied(e) = parts.headers.entry(CONTENT_ENCODING) {
            let body = match e.get().as_bytes() {
                #[cfg(feature = "gzip")]
                b"gzip" if accept.gzip() => DecompressBody::gzip(body),
                #[cfg(feature = "deflate")]
                b"deflate" if accept.deflate() => DecompressBody::deflate(body),
                #[cfg(feature = "br")]
                b"br" if accept.br() => DecompressBody::br(body),
                _ => return http::Response::from_parts(parts, DecompressBody::identity(body)),
            };
            e.remove();
            parts.headers.remove(CONTENT_LENGTH);
            http::Response::from_parts(parts, body)
        } else {
            http::Response::from_parts(parts, DecompressBody::identity(body))
        }
    }

    fn identity(body: B) -> Self {
        DecompressBody {
            inner: BodyInner::Identity {
                inner: body,
                marker: PhantomData,
            },
        }
    }

    #[cfg(feature = "gzip")]
    fn gzip(body: B) -> Self {
        let read = StreamReader::new(Adapter { body, error: None });
        let inner = FramedRead::new(GzipDecoder::new(read), BytesCodec::new());
        DecompressBody {
            inner: BodyInner::Gzip { inner },
        }
    }

    #[cfg(feature = "deflate")]
    fn deflate(body: B) -> Self {
        let read = StreamReader::new(Adapter { body, error: None });
        let inner = FramedRead::new(ZlibDecoder::new(read), BytesCodec::new());
        DecompressBody {
            inner: BodyInner::Deflate { inner },
        }
    }

    #[cfg(feature = "br")]
    fn br(body: B) -> Self {
        let read = StreamReader::new(Adapter { body, error: None });
        let inner = FramedRead::new(BrotliDecoder::new(read), BytesCodec::new());
        DecompressBody {
            inner: BodyInner::Brotli { inner },
        }
    }
}

impl<B> Body for DecompressBody<B>
where
    B: Body,
{
    type Data = Bytes;
    type Error = Error<B::Error>;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let (poll, inner) = match self.project().inner.project() {
            BodyInnerProj::Identity { inner, .. } => {
                return match ready!(inner.poll_data(cx)) {
                    Some(Ok(mut data)) => {
                        Poll::Ready(Some(Ok(data.copy_to_bytes(data.remaining()))))
                    }
                    Some(Err(e)) => Poll::Ready(Some(Err(Error::Body(e)))),
                    None => Poll::Ready(None),
                };
            }
            #[cfg(feature = "gzip")]
            BodyInnerProj::Gzip { mut inner } => (
                inner.as_mut().poll_next(cx),
                inner.get_pin_mut().get_pin_mut().get_pin_mut(),
            ),
            #[cfg(feature = "deflate")]
            BodyInnerProj::Deflate { mut inner } => (
                inner.as_mut().poll_next(cx),
                inner.get_pin_mut().get_pin_mut().get_pin_mut(),
            ),
            #[cfg(feature = "br")]
            BodyInnerProj::Brotli { mut inner } => (
                inner.as_mut().poll_next(cx),
                inner.get_pin_mut().get_pin_mut().get_pin_mut(),
            ),
        };
        match ready!(poll) {
            // Use UFCS to help type inference when compiling with no features.
            Some(Ok(data)) => Poll::Ready(Some(Ok(BytesMut::freeze(data)))),
            Some(Err(e)) => Poll::Ready(Some(Err(Adapter::<B, B::Error>::project(inner)
                .error
                .take()
                .map(Error::Body)
                .unwrap_or(Error::Decompress(e))))),
            None => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        match self.project().inner.project() {
            BodyInnerProj::Identity { inner, .. } => inner.poll_trailers(cx),
            #[cfg(feature = "gzip")]
            BodyInnerProj::Gzip { inner } => inner
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            #[cfg(feature = "deflate")]
            BodyInnerProj::Deflate { inner } => inner
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            #[cfg(feature = "br")]
            BodyInnerProj::Brotli { inner } => inner
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

impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self {
            Error::Body(e) => e.fmt(f),
            Error::Decompress(e) => Display::fmt(&e, f),
        }
    }
}

impl<E: error::Error + 'static> error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &self {
            Error::Body(e) => Some(e),
            Error::Decompress(e) => Some(e),
        }
    }
}

// Manually implement `Debug` because the `derive(Debug)` macro cannot figure out that
// `B::Error: Debug` is required.
impl<B> Debug for DecompressBody<B>
where
    B: Body + Debug,
    B::Error: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        struct DecompressBody<'a, B: Body> {
            inner: &'a BodyInner<B, B::Error>,
        }
        DecompressBody { inner: &self.inner }.fmt(f)
    }
}

impl<B: Body> Stream for Adapter<B, B::Error> {
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match ready!(this.body.poll_data(cx)) {
            Some(Ok(mut data)) => Poll::Ready(Some(Ok(data.copy_to_bytes(data.remaining())))),
            Some(Err(e)) => {
                *this.error = Some(e);
                // Return a placeholder, which should be discarded by the outer `DecompressBody`.
                Poll::Ready(Some(Err(io::Error::from_raw_os_error(0))))
            }
            None => Poll::Ready(None),
        }
    }
}

impl AcceptEncoding {
    fn to_header_value(&self) -> Option<HeaderValue> {
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

    fn gzip(&self) -> bool {
        #[cfg(feature = "gzip")]
        {
            self.gzip
        }
        #[cfg(not(feature = "gzip"))]
        {
            false
        }
    }

    fn deflate(&self) -> bool {
        #[cfg(feature = "deflate")]
        {
            self.deflate
        }
        #[cfg(not(feature = "deflate"))]
        {
            false
        }
    }

    fn br(&self) -> bool {
        #[cfg(feature = "br")]
        {
            self.br
        }
        #[cfg(not(feature = "br"))]
        {
            false
        }
    }

    #[cfg_attr(not(feature = "gzip"), allow(unused))]
    fn set_gzip(mut self, enable: bool) -> Self {
        #[cfg(feature = "gzip")]
        {
            self.gzip = enable;
            self
        }
        #[cfg(not(feature = "gzip"))]
        {
            self
        }
    }

    #[cfg_attr(not(feature = "deflate"), allow(unused))]
    fn set_deflate(mut self, enable: bool) -> Self {
        #[cfg(feature = "deflate")]
        {
            self.deflate = enable;
            self
        }
        #[cfg(not(feature = "deflate"))]
        {
            self
        }
    }

    #[cfg_attr(not(feature = "br"), allow(unused))]
    fn set_br(mut self, enable: bool) -> Self {
        #[cfg(feature = "br")]
        {
            self.br = enable;
            self
        }
        #[cfg(not(feature = "br"))]
        {
            self
        }
    }
}

impl Default for AcceptEncoding {
    fn default() -> Self {
        AcceptEncoding {
            #[cfg(feature = "gzip")]
            gzip: true,
            #[cfg(feature = "deflate")]
            deflate: true,
            #[cfg(feature = "br")]
            br: true,
        }
    }
}
