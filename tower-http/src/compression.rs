use crate::common::*;
use async_compression::tokio::bufread::BrotliEncoder;
use async_compression::tokio::bufread::DeflateEncoder;
use async_compression::tokio::bufread::GzipEncoder;
use async_compression::tokio::bufread::ZstdEncoder;
use futures_util::TryStreamExt;
use http::HeaderMap;
use hyper::Body;
use std::io;
use tokio_util::{
    codec::{BytesCodec, FramedRead},
    io::StreamReader,
};

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

#[derive(Clone, Copy, Debug)]
pub struct Compression<S> {
    inner: S,
}

impl<ResBody, S> Service<Request<ResBody>> for Compression<S>
where
    S: Service<Request<ResBody>, Response = Response<Body>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ResBody>) -> Self::Future {
        let encoding = parse(req.headers()).unwrap_or(Encoding::Identity);

        ResponseFuture {
            future: self.inner.call(req),
            encoding,
        }
    }
}

#[pin_project]
#[derive(Debug)]
pub struct ResponseFuture<F> {
    #[pin]
    future: F,
    encoding: Encoding,
}

impl<F, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<Body>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let res = ready!(this.future.poll(cx)?);

        // do the encoding
        let encoding = *this.encoding;
        let mut res = res.map(move |body| {
            let stream = body.map_err(|err| io::Error::new(io::ErrorKind::Other, err));

            match encoding {
                Encoding::Gzip => {
                    let stream = FramedRead::new(
                        GzipEncoder::new(StreamReader::new(stream)),
                        BytesCodec::new(),
                    );
                    Body::wrap_stream(stream)
                }
                Encoding::Deflate => {
                    let stream = FramedRead::new(
                        DeflateEncoder::new(StreamReader::new(stream)),
                        BytesCodec::new(),
                    );
                    Body::wrap_stream(stream)
                }
                Encoding::Brotli => {
                    let stream = FramedRead::new(
                        BrotliEncoder::new(StreamReader::new(stream)),
                        BytesCodec::new(),
                    );
                    Body::wrap_stream(stream)
                }
                Encoding::Zstd => {
                    let stream = FramedRead::new(
                        ZstdEncoder::new(StreamReader::new(stream)),
                        BytesCodec::new(),
                    );
                    Body::wrap_stream(stream)
                }
                Encoding::Identity => Body::wrap_stream(stream),
            }
        });

        // update headers
        if let Encoding::Identity = encoding {
            // no need to mess with headers
        } else {
            let headers = res.headers_mut();
            headers.append(
                header::CONTENT_ENCODING,
                HeaderValue::from_static(encoding.to_str()),
            );
            headers.remove(header::CONTENT_LENGTH);
        }

        Poll::Ready(Ok(res))
    }
}

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
}

// based on https://github.com/http-rs/accept-encoding
fn parse(headers: &HeaderMap) -> Option<Encoding> {
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

    preferred_encoding
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
