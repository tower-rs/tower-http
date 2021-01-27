use async_compression::tokio::bufread::{BrotliEncoder, DeflateEncoder, GzipEncoder, ZstdEncoder};
use bytes::{Buf, Bytes};
use futures_core::Stream;
use futures_util::ready;
use http::{header, HeaderMap, HeaderValue, Request, Response};
use http_body::Body;
use pin_project::pin_project;
use std::{
    fmt,
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio_util::{
    codec::{BytesCodec, FramedRead},
    io::StreamReader,
};
use tower_layer::Layer;
use tower_service::Service;

#[derive(Clone, Copy, Debug, Default)]
pub struct CompressionLayer {
    _priv: (),
}

impl CompressionLayer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<S> Layer<S> for CompressionLayer {
    type Service = Compression<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Compression { inner }
    }
}

#[derive(Clone, Copy)]
pub struct Compression<S> {
    inner: S,
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for Compression<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
{
    type Response = Response<CompressionBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let encoding = Encoding::from_headers(req.headers());
        ResponseFuture {
            inner: self.inner.call(req),
            encoding,
        }
    }
}

#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F> {
    #[pin]
    inner: F,
    encoding: Encoding,
}

impl<F, B, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<http::Response<B>, E>>,
    B: Body,
{
    type Output = Result<http::Response<CompressionBody<B>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let res = ready!(this.inner.poll(cx)?);
        Poll::Ready(Ok(CompressionBody::wrap_response(res, *this.encoding)))
    }
}

#[pin_project]
pub struct CompressionBody<B: Body> {
    #[pin]
    inner: BodyInner<B, B::Error>,
}

impl<B> CompressionBody<B>
where
    B: Body,
{
    fn wrap_response(res: Response<B>, encoding: Encoding) -> Response<Self> {
        let (mut parts, body) = res.into_parts();

        let body = match encoding {
            Encoding::Gzip => {
                let read = StreamReader::new(Adapter { body, error: None });
                let framed_read = FramedRead::new(GzipEncoder::new(read), BytesCodec::new());
                CompressionBody {
                    inner: BodyInner::Gzip(framed_read),
                }
            }
            Encoding::Deflate => {
                let read = StreamReader::new(Adapter { body, error: None });
                let framed_read = FramedRead::new(DeflateEncoder::new(read), BytesCodec::new());
                CompressionBody {
                    inner: BodyInner::Deflate(framed_read),
                }
            }
            Encoding::Brotli => {
                let read = StreamReader::new(Adapter { body, error: None });
                let framed_read = FramedRead::new(BrotliEncoder::new(read), BytesCodec::new());
                CompressionBody {
                    inner: BodyInner::Brotli(framed_read),
                }
            }
            Encoding::Zstd => {
                let read = StreamReader::new(Adapter { body, error: None });
                let framed_read = FramedRead::new(ZstdEncoder::new(read), BytesCodec::new());
                CompressionBody {
                    inner: BodyInner::Zstd(framed_read),
                }
            }
            Encoding::Identity => {
                return Response::from_parts(
                    parts,
                    CompressionBody {
                        inner: BodyInner::Identity(body),
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

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let (bytes, inner) = match self.project().inner.project() {
            BodyInnerProj::Gzip(mut framed_read) => (
                ready!(framed_read.as_mut().poll_next(cx)),
                framed_read.get_pin_mut().get_pin_mut().get_pin_mut(),
            ),
            BodyInnerProj::Deflate(mut framed_read) => (
                ready!(framed_read.as_mut().poll_next(cx)),
                framed_read.get_pin_mut().get_pin_mut().get_pin_mut(),
            ),
            BodyInnerProj::Brotli(mut framed_read) => (
                ready!(framed_read.as_mut().poll_next(cx)),
                framed_read.get_pin_mut().get_pin_mut().get_pin_mut(),
            ),
            BodyInnerProj::Zstd(mut framed_read) => (
                ready!(framed_read.as_mut().poll_next(cx)),
                framed_read.get_pin_mut().get_pin_mut().get_pin_mut(),
            ),
            BodyInnerProj::Identity(inner) => {
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
            Some(Ok(data)) => Poll::Ready(Some(Ok(data.freeze()))),
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
            BodyInnerProj::Gzip(framed_read) => framed_read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            BodyInnerProj::Deflate(framed_read) => framed_read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            BodyInnerProj::Brotli(framed_read) => framed_read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            BodyInnerProj::Zstd(framed_read) => framed_read
                .get_pin_mut()
                .get_pin_mut()
                .get_pin_mut()
                .project()
                .body
                .poll_trailers(cx),
            BodyInnerProj::Identity(inner) => inner.poll_trailers(cx),
        }
        .map_err(Error::Body)
    }
}

#[pin_project(project = BodyInnerProj)]
#[derive(Debug)]
enum BodyInner<B, E> {
    Identity(#[pin] B),
    Gzip(#[pin] FramedRead<GzipEncoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
    Deflate(#[pin] FramedRead<DeflateEncoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
    Brotli(#[pin] FramedRead<BrotliEncoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
    Zstd(#[pin] FramedRead<ZstdEncoder<StreamReader<Adapter<B, E>, Bytes>>, BytesCodec>),
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

                // Return a placeholder, which should be discarded by the outer `DecompressBody`.
                Poll::Ready(Some(Err(io::Error::from_raw_os_error(0))))
            }
            None => Poll::Ready(None),
        }
    }
}

#[derive(Debug)]
pub enum Error<E> {
    Body(E),
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

impl<E> std::error::Error for Error<E> where E: std::error::Error {}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Encoding {
    Gzip,
    Deflate,
    Brotli,
    Zstd,
    Identity,
}

impl Encoding {
    fn to_str(self) -> &'static str {
        match self {
            Encoding::Gzip => "gzip",
            Encoding::Deflate => "deflate",
            Encoding::Identity => "identity",
            Encoding::Brotli => "br",
            Encoding::Zstd => "zstd",
        }
    }

    fn parse(s: &str) -> Option<Encoding> {
        match s {
            "gzip" => Some(Encoding::Gzip),
            "deflate" => Some(Encoding::Deflate),
            "br" => Some(Encoding::Brotli),
            "zstd" => Some(Encoding::Zstd),
            "identity" => Some(Encoding::Identity),
            _ => None,
        }
    }

    // based on https://github.com/http-rs/accept-encoding
    fn from_headers(headers: &HeaderMap) -> Self {
        let mut preferred_encoding = None;
        let mut max_qval = 0.0;

        for (encoding, qval) in encodings(headers) {
            if (qval - 1.0f32).abs() < 0.01 {
                preferred_encoding = Some(encoding);
                break;
            } else if qval > max_qval {
                preferred_encoding = Some(encoding);
                max_qval = qval;
            }
        }

        preferred_encoding.unwrap_or(Encoding::Identity)
    }
}

// based on https://github.com/http-rs/accept-encoding
fn encodings(headers: &HeaderMap) -> Vec<(Encoding, f32)> {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|hval| hval.to_str().ok())
        .flat_map(|s| s.split(',').map(str::trim))
        .filter_map(|v| {
            let mut v = v.splitn(2, ";q=");

            let encoding = match Encoding::parse(v.next().unwrap()) {
                Some(encoding) => encoding,
                None => return None, // ignore unknown encodings
            };

            let qval = if let Some(qval) = v.next() {
                let qval = match qval.parse::<f32>() {
                    Ok(f) => f,
                    Err(_) => return None,
                };
                if qval > 1.0 {
                    return None; // q-values over 1 are unacceptable
                }
                qval
            } else {
                1.0f32
            };

            Some((encoding, qval))
        })
        .collect::<Vec<(Encoding, f32)>>()
}
