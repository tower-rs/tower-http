//! Middleware that decompresses response bodies.

#![cfg_attr(
    not(any(
        feature = "decompression-br",
        feature = "decompression-gzip",
        feature = "decompression-deflate"
    )),
    allow(unused)
)]

use std::error;
use std::fmt::{self, Debug, Display, Formatter};
use std::future::Future;
use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "decompression-br")]
use async_compression::tokio::bufread::BrotliDecoder;
#[cfg(feature = "decompression-gzip")]
use async_compression::tokio::bufread::GzipDecoder;
#[cfg(feature = "decompression-deflate")]
use async_compression::tokio::bufread::ZlibDecoder;
use bytes::{Buf, Bytes, BytesMut};
use futures_core::{ready, Stream, TryFuture};
use http::header::{self, HeaderValue, ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, RANGE};
use http::{HeaderMap, Request, Response};
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
pub struct Decompression<S> {
    inner: S,
    accept: AcceptEncoding,
}

/// Decompresses response bodies of the underlying service.
///
/// This adds the `Accept-Encoding` header to requests and transparently decompresses response
/// bodies based on the `Content-Encoding` header.
#[derive(Debug, Default, Clone)]
pub struct DecompressionLayer {
    accept: AcceptEncoding,
}

/// Response future of [`Decompression`].
#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F> {
    #[pin]
    inner: F,
    accept: AcceptEncoding,
}

#[pin_project]
/// Response body of [`Decompression`].
pub struct DecompressionBody<B: Body> {
    #[pin]
    inner: BodyInner<B, B::Error>,
}

/// Error type of [`DecompressionBody`].
#[derive(Debug)]
pub enum Error<E> {
    /// Error from the underlying body.
    Body(E),
    /// Decompression error.
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
    #[cfg(feature = "decompression-gzip")]
    Gzip {
        #[pin]
        inner: FramedRead<GzipDecoder<BodyReader<B, E>>, BytesCodec>,
    },
    #[cfg(feature = "decompression-deflate")]
    Deflate {
        #[pin]
        inner: FramedRead<ZlibDecoder<BodyReader<B, E>>, BytesCodec>,
    },
    #[cfg(feature = "decompression-br")]
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
    #[cfg(feature = "decompression-gzip")]
    gzip: bool,
    #[cfg(feature = "decompression-deflate")]
    deflate: bool,
    #[cfg(feature = "decompression-br")]
    br: bool,
}

impl<S> Decompression<S> {
    /// Creates a new `Decompression` wrapping the `service`.
    pub fn new(service: S) -> Self {
        Decompression {
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
    #[cfg(feature = "decompression-gzip")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression-gzip")))]
    pub fn gzip(self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to request the Deflate encoding.
    #[cfg(feature = "decompression-deflate")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression-deflate")))]
    pub fn deflate(self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to request the Brotli encoding.
    #[cfg(feature = "decompression-br")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression-br")))]
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

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for Decompression<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
{
    type Response = Response<DecompressionBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
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

impl DecompressionLayer {
    /// Creates a new `DecompressionLayer`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets whether to request the gzip encoding.
    #[cfg(feature = "decompression-gzip")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression-gzip")))]
    pub fn gzip(self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to request the Deflate encoding.
    #[cfg(feature = "decompression-deflate")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression-deflate")))]
    pub fn deflate(self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to request the Brotli encoding.
    #[cfg(feature = "decompression-br")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression-br")))]
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

impl<S> Layer<S> for DecompressionLayer {
    type Service = Decompression<S>;

    fn layer(&self, service: S) -> Self::Service {
        Decompression {
            inner: service,
            accept: self.accept,
        }
    }
}

impl<F, B> Future for ResponseFuture<F>
where
    F: TryFuture<Ok = Response<B>>,
    B: Body,
{
    type Output = Result<Response<DecompressionBody<B>>, F::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.as_mut().project().inner.try_poll(cx)?);
        Poll::Ready(Ok(DecompressionBody::wrap_response(res, &self.accept)))
    }
}

impl<B> DecompressionBody<B>
where
    B: Body,
{
    /// Gets a reference to the underlying body.
    pub fn get_ref(&self) -> &B {
        match &self.inner {
            BodyInner::Identity { inner, .. } => inner,
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip { inner } => &inner.get_ref().get_ref().get_ref().body,
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate { inner } => &inner.get_ref().get_ref().get_ref().body,
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli { inner } => &inner.get_ref().get_ref().get_ref().body,
        }
    }

    /// Gets a mutable reference to the underlying body.
    ///
    /// It is inadvisable to directly read from the underlying body.
    pub fn get_mut(&mut self) -> &mut B {
        match &mut self.inner {
            BodyInner::Identity { inner, .. } => inner,
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip { inner } => &mut inner.get_mut().get_mut().get_mut().body,
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate { inner } => &mut inner.get_mut().get_mut().get_mut().body,
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli { inner } => &mut inner.get_mut().get_mut().get_mut().body,
        }
    }

    /// Gets a pinned mutable reference to the underlying body.
    ///
    /// It is inadvisable to directly read from the underlying body.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut B> {
        match self.project().inner.project() {
            BodyInnerProj::Identity { inner, .. } => inner,
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip { inner } => {
                inner
                    .get_pin_mut()
                    .get_pin_mut()
                    .get_pin_mut()
                    .project()
                    .body
            }
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate { inner } => {
                inner
                    .get_pin_mut()
                    .get_pin_mut()
                    .get_pin_mut()
                    .project()
                    .body
            }
            #[cfg(feature = "decompression-br")]
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
            #[cfg(feature = "decompression-gzip")]
            BodyInner::Gzip { inner } => inner.into_inner().into_inner().into_inner().body,
            #[cfg(feature = "decompression-deflate")]
            BodyInner::Deflate { inner } => inner.into_inner().into_inner().into_inner().body,
            #[cfg(feature = "decompression-br")]
            BodyInner::Brotli { inner } => inner.into_inner().into_inner().into_inner().body,
        }
    }

    #[cfg_attr(
        not(any(
            feature = "decompression-br",
            feature = "decompression-gzip",
            feature = "decompression-deflate"
        )),
        allow(unreachable_code)
    )]
    fn wrap_response(res: Response<B>, accept: &AcceptEncoding) -> Response<Self> {
        let (mut parts, body) = res.into_parts();
        if let header::Entry::Occupied(e) = parts.headers.entry(CONTENT_ENCODING) {
            let body = match e.get().as_bytes() {
                #[cfg(feature = "decompression-gzip")]
                b"gzip" if accept.gzip() => DecompressionBody::gzip(body),
                #[cfg(feature = "decompression-deflate")]
                b"deflate" if accept.deflate() => DecompressionBody::deflate(body),
                #[cfg(feature = "decompression-br")]
                b"br" if accept.br() => DecompressionBody::br(body),
                _ => return Response::from_parts(parts, DecompressionBody::identity(body)),
            };
            e.remove();
            parts.headers.remove(CONTENT_LENGTH);
            Response::from_parts(parts, body)
        } else {
            Response::from_parts(parts, DecompressionBody::identity(body))
        }
    }

    fn identity(body: B) -> Self {
        DecompressionBody {
            inner: BodyInner::Identity {
                inner: body,
                marker: PhantomData,
            },
        }
    }

    #[cfg(feature = "decompression-gzip")]
    fn gzip(body: B) -> Self {
        let read = StreamReader::new(Adapter { body, error: None });
        let inner = FramedRead::new(GzipDecoder::new(read), BytesCodec::new());
        DecompressionBody {
            inner: BodyInner::Gzip { inner },
        }
    }

    #[cfg(feature = "decompression-deflate")]
    fn deflate(body: B) -> Self {
        let read = StreamReader::new(Adapter { body, error: None });
        let inner = FramedRead::new(ZlibDecoder::new(read), BytesCodec::new());
        DecompressionBody {
            inner: BodyInner::Deflate { inner },
        }
    }

    #[cfg(feature = "decompression-br")]
    fn br(body: B) -> Self {
        let read = StreamReader::new(Adapter { body, error: None });
        let inner = FramedRead::new(BrotliDecoder::new(read), BytesCodec::new());
        DecompressionBody {
            inner: BodyInner::Brotli { inner },
        }
    }
}

impl<B> Body for DecompressionBody<B>
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
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip { mut inner } => (
                inner.as_mut().poll_next(cx),
                inner.get_pin_mut().get_pin_mut().get_pin_mut(),
            ),
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate { mut inner } => (
                inner.as_mut().poll_next(cx),
                inner.get_pin_mut().get_pin_mut().get_pin_mut(),
            ),
            #[cfg(feature = "decompression-br")]
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
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        match self.project().inner.project() {
            BodyInnerProj::Identity { inner, .. } => inner.poll_trailers(cx),
            #[cfg(feature = "decompression-gzip")]
            BodyInnerProj::Gzip { inner } => inner
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            #[cfg(feature = "decompression-deflate")]
            BodyInnerProj::Deflate { inner } => inner
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            #[cfg(feature = "decompression-br")]
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
impl<B> Debug for DecompressionBody<B>
where
    B: Body + Debug,
    B::Error: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        struct DecompressionBody<'a, B: Body> {
            inner: &'a BodyInner<B, B::Error>,
        }
        DecompressionBody { inner: &self.inner }.fmt(f)
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
                // Return a placeholder, which should be discarded by the outer `DecompressionBody`.
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
        #[cfg(feature = "decompression-gzip")]
        {
            self.gzip
        }
        #[cfg(not(feature = "decompression-gzip"))]
        {
            false
        }
    }

    fn deflate(&self) -> bool {
        #[cfg(feature = "decompression-deflate")]
        {
            self.deflate
        }
        #[cfg(not(feature = "decompression-deflate"))]
        {
            false
        }
    }

    fn br(&self) -> bool {
        #[cfg(feature = "decompression-br")]
        {
            self.br
        }
        #[cfg(not(feature = "decompression-br"))]
        {
            false
        }
    }

    #[cfg_attr(not(feature = "decompression-gzip"), allow(unused))]
    fn set_gzip(mut self, enable: bool) -> Self {
        #[cfg(feature = "decompression-gzip")]
        {
            self.gzip = enable;
            self
        }
        #[cfg(not(feature = "decompression-gzip"))]
        {
            self
        }
    }

    #[cfg_attr(not(feature = "decompression-deflate"), allow(unused))]
    fn set_deflate(mut self, enable: bool) -> Self {
        #[cfg(feature = "decompression-deflate")]
        {
            self.deflate = enable;
            self
        }
        #[cfg(not(feature = "decompression-deflate"))]
        {
            self
        }
    }

    #[cfg_attr(not(feature = "decompression-br"), allow(unused))]
    fn set_br(mut self, enable: bool) -> Self {
        #[cfg(feature = "decompression-br")]
        {
            self.br = enable;
            self
        }
        #[cfg(not(feature = "decompression-br"))]
        {
            self
        }
    }
}

impl Default for AcceptEncoding {
    fn default() -> Self {
        AcceptEncoding {
            #[cfg(feature = "decompression-gzip")]
            gzip: true,
            #[cfg(feature = "decompression-deflate")]
            deflate: true,
            #[cfg(feature = "decompression-br")]
            br: true,
        }
    }
}
