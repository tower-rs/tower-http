//! Service that serves a file.

use super::{check_precompressed_file, AsyncReadBody, PrecompressedVariants};
use crate::content_encoding::Encoding;
use crate::services::fs::DEFAULT_CAPACITY;
use bytes::Bytes;
use futures_util::ready;
use http::{header, HeaderValue, Request, Response};
use http_body::{combinators::BoxBody, Body};
use mime::Mime;
use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::fs::File;
use tower_service::Service;

/// Service that serves a file.
#[derive(Clone, Debug)]
pub struct ServeFile {
    path: PathBuf,
    mime: HeaderValue,
    buf_chunk_size: usize,
    precompressed_variants: Option<PrecompressedVariants>,
}

impl ServeFile {
    /// Create a new [`ServeFile`].
    ///
    /// The `Content-Type` will be guessed from the file extension.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let guess = mime_guess::from_path(&path);
        let mime = guess
            .first_raw()
            .map(|mime| HeaderValue::from_static(mime))
            .unwrap_or_else(|| {
                HeaderValue::from_str(mime::APPLICATION_OCTET_STREAM.as_ref()).unwrap()
            });

        let path = path.as_ref().to_owned();

        Self {
            path,
            mime,
            buf_chunk_size: DEFAULT_CAPACITY,
            precompressed_variants: None,
        }
    }

    /// Create a new [`ServeFile`] with a specific mime type.
    ///
    /// # Panics
    ///
    /// Will panic if the mime type isn't a valid [header value].
    ///
    /// [header value]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html
    pub fn new_with_mime<P: AsRef<Path>>(path: P, mime: &Mime) -> Self {
        let mime = HeaderValue::from_str(mime.as_ref()).expect("mime isn't a valid header value");
        let path = path.as_ref().to_owned();

        Self {
            path,
            mime,
            buf_chunk_size: DEFAULT_CAPACITY,
            precompressed_variants: None,
        }
    }

    /// Informs the service that it should also look for a precompressed gzip
    /// version of the file.
    ///
    /// If the client has an `Accept-Encoding` header that allows the gzip encoding,
    /// the file `foo.txt.gz` will be served instead of `foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the same directory. Different precompressed
    /// variants can be combined.
    pub fn precompressed_gzip(mut self) -> Self {
        self.precompressed_variants
            .get_or_insert(Default::default())
            .gzip = true;
        self
    }

    /// Informs the service that it should also look for a precompressed brotli
    /// version of the file.
    ///
    /// If the client has an `Accept-Encoding` header that allows the brotli encoding,
    /// the file `foo.txt.br` will be served instead of `foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the same directory. Different precompressed
    /// variants can be combined.
    pub fn precompressed_br(mut self) -> Self {
        self.precompressed_variants
            .get_or_insert(Default::default())
            .br = true;
        self
    }

    /// Informs the service that it should also look for a precompressed deflate
    /// version of the file.
    ///
    /// If the client has an `Accept-Encoding` header that allows the deflate encoding,
    /// the file `foo.txt.zz` will be served instead of `foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the same directory. Different precompressed
    /// variants can be combined.
    pub fn precompressed_deflate(mut self) -> Self {
        self.precompressed_variants
            .get_or_insert(Default::default())
            .deflate = true;
        self
    }

    /// Set a specific read buffer chunk size.
    ///
    /// The default capacity is 64kb.
    pub fn with_buf_chunk_size(mut self, chunk_size: usize) -> Self {
        self.buf_chunk_size = chunk_size;
        self
    }
}

impl<ReqBody> Service<Request<ReqBody>> for ServeFile {
    type Response = Response<ResponseBody>;
    type Error = io::Error;
    type Future = ResponseFuture;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut path = self.path.clone();

        let negotiated_encoding = self
            .precompressed_variants
            .map(|precompressed| {
                Encoding::from_headers(
                    req.headers(),
                    check_precompressed_file(precompressed, &path),
                )
            })
            .filter(|encoding| *encoding != Encoding::Identity);

        if let Some(file_extension) =
            negotiated_encoding.and_then(|encoding| encoding.to_file_extension())
        {
            let new_extension = path
                .extension()
                .map(|extension| {
                    let mut os_string = extension.to_os_string();
                    os_string.push(file_extension);
                    os_string
                })
                .unwrap_or_else(|| file_extension.to_os_string());
            path.set_extension(new_extension);
        }

        let open_file_future = Box::pin(File::open(path));

        ResponseFuture {
            open_file_future,
            mime: Some(self.mime.clone()),
            buf_chunk_size: self.buf_chunk_size,
            encoding: negotiated_encoding,
        }
    }
}

/// Response future of [`ServeFile`].
pub struct ResponseFuture {
    open_file_future: Pin<Box<dyn Future<Output = io::Result<File>> + Send + Sync + 'static>>,
    mime: Option<HeaderValue>,
    encoding: Option<Encoding>,
    buf_chunk_size: usize,
}

impl Future for ResponseFuture {
    type Output = io::Result<Response<ResponseBody>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = ready!(Pin::new(&mut self.open_file_future).poll(cx));

        let file = match result {
            Ok(file) => file,
            Err(err) => {
                return Poll::Ready(
                    super::response_from_io_error(err).map(|res| res.map(ResponseBody::new)),
                )
            }
        };

        let chunk_size = self.buf_chunk_size;
        let body = AsyncReadBody::with_capacity(file, chunk_size).boxed();
        let body = ResponseBody::new(body);

        let mut res = Response::new(body);
        res.headers_mut()
            .insert(header::CONTENT_TYPE, self.mime.take().unwrap());

        if let Some(encoding) = self.encoding {
            res.headers_mut()
                .insert(header::CONTENT_ENCODING, encoding.into_header_value());
        }
        Poll::Ready(Ok(res))
    }
}

opaque_body! {
    /// Response body for [`ServeFile`].
    pub type ResponseBody = BoxBody<Bytes, io::Error>;
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    #[allow(unused_imports)]
    use super::*;
    use brotli::BrotliDecompress;
    use flate2::bufread::DeflateDecoder;
    use flate2::bufread::GzDecoder;
    use http::{Request, StatusCode};
    use http_body::Body as _;
    use hyper::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn basic() {
        let svc = ServeFile::new("../README.md");

        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = res.into_body().data().await.unwrap().unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert!(body.starts_with("# Tower HTTP"));
    }

    #[tokio::test]
    async fn precompressed_gzip() {
        let svc = ServeFile::new("../test-files/precompressed.txt").precompressed_gzip();

        let request = Request::builder()
            .header("Accept-Encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "gzip");

        let body = res.into_body().data().await.unwrap().unwrap();
        let mut decoder = GzDecoder::new(&body[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert!(decompressed.starts_with("\"This is a test file!\""));
    }

    #[tokio::test]
    async fn unsupported_precompression_alogrithm_fallbacks_to_uncompressed() {
        let svc = ServeFile::new("../test-files/precompressed.txt").precompressed_gzip();

        let request = Request::builder()
            .header("Accept-Encoding", "br")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert!(res.headers().get("content-encoding").is_none());

        let body = res.into_body().data().await.unwrap().unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.starts_with("\"This is a test file!\""));
    }

    #[tokio::test]
    async fn only_precompressed_variant_existing() {
        let svc = ServeFile::new("../test-files/only_gzipped.txt").precompressed_gzip();

        let request = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        // Should reply with gzipped file if client supports it
        let request = Request::builder()
            .header("Accept-Encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "gzip");

        let body = res.into_body().data().await.unwrap().unwrap();
        let mut decoder = GzDecoder::new(&body[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert!(decompressed.starts_with("\"This is a test file\""));
    }

    #[tokio::test]
    async fn only_uncompressed_variant_existing() {
        let svc = ServeFile::new("../test-files/only_uncompressed.txt").precompressed_gzip();

        let request = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);

        // Should reply with gzipped file if client supports it
        let request = Request::builder()
            .header("Accept-Encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert!(res.headers().get("content-encoding").is_none());

        let body = res.into_body().data().await.unwrap().unwrap();
        let body=String::from_utf8(body.to_vec()).unwrap();
        assert!(body.starts_with("\"This is a test file!\""));
    }


    #[tokio::test]
    async fn precompressed_br() {
        let svc = ServeFile::new("../test-files/precompressed.txt").precompressed_br();

        let request = Request::builder()
            .header("Accept-Encoding", "gzip,br")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "br");

        let body = res.into_body().data().await.unwrap().unwrap();
        let mut decompressed = Vec::new();
        BrotliDecompress(&mut &body[..], &mut decompressed).unwrap();
        let decompressed = String::from_utf8(decompressed.to_vec()).unwrap();
        assert!(decompressed.starts_with("\"This is a test file!\""));
    }

    #[tokio::test]
    async fn precompressed_deflate() {
        let svc = ServeFile::new("../test-files/precompressed.txt").precompressed_deflate();
        let request = Request::builder()
            .header("Accept-Encoding", "deflate,br")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "deflate");

        let body = res.into_body().data().await.unwrap().unwrap();
        let mut decoder = DeflateDecoder::new(&body[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert!(decompressed.starts_with("\"This is a test file!\""));
    }

    #[tokio::test]
    async fn multi_precompressed() {
        let svc = ServeFile::new("../test-files/precompressed.txt")
            .precompressed_gzip()
            .precompressed_br();

        let request = Request::builder()
            .header("Accept-Encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "gzip");

        let body = res.into_body().data().await.unwrap().unwrap();
        let mut decoder = GzDecoder::new(&body[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert!(decompressed.starts_with("\"This is a test file!\""));

        let request = Request::builder()
            .header("Accept-Encoding", "br")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "br");

        let body = res.into_body().data().await.unwrap().unwrap();
        let mut decompressed = Vec::new();
        BrotliDecompress(&mut &body[..], &mut decompressed).unwrap();
        let decompressed = String::from_utf8(decompressed.to_vec()).unwrap();
        assert!(decompressed.starts_with("\"This is a test file!\""));
    }

    #[tokio::test]
    async fn with_custom_chunk_size() {
        let svc = ServeFile::new("../README.md").with_buf_chunk_size(1024 * 32);

        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = res.into_body().data().await.unwrap().unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert!(body.starts_with("# Tower HTTP"));
    }

    #[tokio::test]
    async fn returns_404_if_file_doesnt_exist() {
        let svc = ServeFile::new("../this-doesnt-exist.md");

        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert!(res.headers().get(header::CONTENT_TYPE).is_none());
    }
}
