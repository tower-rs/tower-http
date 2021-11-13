use super::{AsyncReadBody, PrecompressedVariants};
use crate::{content_encoding::Encoding, services::fs::DEFAULT_CAPACITY};
use bytes::Bytes;
use futures_util::ready;
use http::{header, HeaderValue, Request, Response, StatusCode, Uri};
use http_body::{combinators::BoxBody, Body, Empty};
use percent_encoding::percent_decode;
use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::fs::File;
use tower_service::Service;

/// Service that serves files from a given directory and all its sub directories.
///
/// The `Content-Type` will be guessed from the file extension.
///
/// An empty response with status `404 Not Found` will be returned if:
///
/// - The file doesn't exist
/// - Any segment of the path contains `..`
/// - Any segment of the path contains a backslash
/// - We don't have necessary permissions to read the file
///
/// # Example
///
/// ```
/// use tower_http::services::ServeDir;
///
/// // This will serve files in the "assets" directory and
/// // its subdirectories
/// let service = ServeDir::new("assets");
///
/// # async {
/// // Run our service using `hyper`
/// let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 3000));
/// hyper::Server::bind(&addr)
///     .serve(tower::make::Shared::new(service))
///     .await
///     .expect("server error");
/// # };
/// ```
///
/// # Handling files not found
///
/// By default `ServeDir` will return an empty `404 Not Found` response if there
/// is no file at the requested path. That can be customized by using
/// [`and_then`](tower::ServiceBuilder::and_then) to change the response:
///
/// ```
/// use tower_http::services::fs::{ServeDir, ServeDirResponseBody};
/// use tower::ServiceBuilder;
/// use http::{StatusCode, Response};
/// use http_body::{Body as _, Full};
/// use std::io;
///
/// let service = ServiceBuilder::new()
///     .and_then(|response: Response<ServeDirResponseBody>| async move {
///         let response = if response.status() == StatusCode::NOT_FOUND {
///             let body = Full::from("Not Found")
///                 .map_err(|err| match err {})
///                 .boxed();
///             Response::builder()
///                 .status(StatusCode::NOT_FOUND)
///                 .body(body)
///                 .unwrap()
///         } else {
///             response.map(|body| body.boxed())
///         };
///
///         Ok::<_, io::Error>(response)
///     })
///     .service(ServeDir::new("assets"));
/// # async {
/// # let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 3000));
/// # hyper::Server::bind(&addr)
/// #     .serve(tower::make::Shared::new(service))
/// #     .await
/// #     .expect("server error");
/// # };
/// ```
#[derive(Clone, Debug)]
pub struct ServeDir {
    base: PathBuf,
    append_index_html_on_directories: bool,
    buf_chunk_size: usize,
    precompressed_variants: Option<PrecompressedVariants>,
}

impl ServeDir {
    /// Create a new [`ServeDir`].
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let mut base = PathBuf::from(".");
        base.push(path.as_ref());

        Self {
            base,
            append_index_html_on_directories: true,
            buf_chunk_size: DEFAULT_CAPACITY,
            precompressed_variants: None,
        }
    }

    /// If the requested path is a directory append `index.html`.
    ///
    /// This is useful for static sites.
    ///
    /// Defaults to `true`.
    pub fn append_index_html_on_directories(mut self, append: bool) -> Self {
        self.append_index_html_on_directories = append;
        self
    }

    /// Set a specific read buffer chunk size.
    ///
    /// The default capacity is 64kb.
    pub fn with_buf_chunk_size(mut self, chunk_size: usize) -> Self {
        self.buf_chunk_size = chunk_size;
        self
    }

    /// Informs the service that it should also look for a precompressed gzip
    /// version of _any_ file in the directory.
    ///
    /// Assuming the `dir` directory is being served and `dir/foo.txt` is requested,
    /// a client with an `Accept-Encoding` header that allows the gzip encoding
    /// will receive the file `dir/foo.txt.gz` instead of `dir/foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the directory. Different precompressed variants can be combined.
    pub fn precompressed_gzip(mut self) -> Self {
        self.precompressed_variants
            .get_or_insert(Default::default())
            .gzip = true;
        self
    }

    /// Informs the service that it should also look for a precompressed brotli
    /// version of _any_ file in the directory.
    ///
    /// Assuming the `dir` directory is being served and `dir/foo.txt` is requested,
    /// a client with an `Accept-Encoding` header that allows the brotli encoding
    /// will receive the file `dir/foo.txt.br` instead of `dir/foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the directory. Different precompressed variants can be combined.
    pub fn precompressed_br(mut self) -> Self {
        self.precompressed_variants
            .get_or_insert(Default::default())
            .br = true;
        self
    }

    /// Informs the service that it should also look for a precompressed deflate
    /// version of _any_ file in the directory.
    ///
    /// Assuming the `dir` directory is being served and `dir/foo.txt` is requested,
    /// a client with an `Accept-Encoding` header that allows the deflate encoding
    /// will receive the file `dir/foo.txt.zz` instead of `dir/foo.txt`.
    /// If the precompressed file is not available, or the client doesn't support it,
    /// the uncompressed version will be served instead.
    /// Both the precompressed version and the uncompressed version are expected
    /// to be present in the directory. Different precompressed variants can be combined.
    pub fn precompressed_deflate(mut self) -> Self {
        self.precompressed_variants
            .get_or_insert(Default::default())
            .deflate = true;
        self
    }
}

impl<ReqBody> Service<Request<ReqBody>> for ServeDir {
    type Response = Response<ResponseBody>;
    type Error = io::Error;
    type Future = ResponseFuture;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        // build and validate the path
        let path = req.uri().path();
        let path = path.trim_start_matches('/');

        let path_decoded = if let Ok(decoded_utf8) = percent_decode(path.as_ref()).decode_utf8() {
            decoded_utf8
        } else {
            return ResponseFuture {
                inner: Inner::Invalid,
            };
        };

        let mut full_path = self.base.clone();
        for seg in path_decoded.split('/') {
            if seg.starts_with("..") || seg.contains('\\') {
                return ResponseFuture {
                    inner: Inner::Invalid,
                };
            }
            full_path.push(seg);
        }

        let append_index_html_on_directories = self.append_index_html_on_directories;
        let buf_chunk_size = self.buf_chunk_size;
        let uri = req.uri().clone();

        let negotiated_encoding = self
            .precompressed_variants
            .map(|precompressed| Encoding::from_headers(req.headers(), precompressed))
            .filter(|encoding| *encoding != Encoding::Identity);

        let open_file_future = Box::pin(async move {
            if !uri.path().ends_with('/') {
                if is_dir(&full_path).await {
                    let location =
                        HeaderValue::from_str(&append_slash_on_path(uri).to_string()).unwrap();
                    return Ok(Output::Redirect(location));
                }
            } else if is_dir(&full_path).await {
                if append_index_html_on_directories {
                    full_path.push("index.html");
                } else {
                    return Ok(Output::NotFound);
                }
            }

            let guess = mime_guess::from_path(&full_path);
            let mime = guess
                .first_raw()
                .map(|mime| HeaderValue::from_static(mime))
                .unwrap_or_else(|| {
                    HeaderValue::from_str(mime::APPLICATION_OCTET_STREAM.as_ref()).unwrap()
                });

            if let Some(file_extension) =
                negotiated_encoding.and_then(|encoding| encoding.to_file_extension())
            {
                let new_extension = full_path
                    .extension()
                    .map(|extension| {
                        let mut os_string = extension.to_os_string();
                        os_string.push(file_extension);
                        os_string
                    })
                    .unwrap_or_else(|| file_extension.to_os_string());
                full_path.set_extension(new_extension);
            }

            let file = File::open(full_path).await?;
            Ok(Output::File(
                file,
                mime,
                buf_chunk_size,
                negotiated_encoding,
            ))
        });

        ResponseFuture {
            inner: Inner::Valid(open_file_future),
        }
    }
}

async fn is_dir(full_path: &Path) -> bool {
    tokio::fs::metadata(full_path)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false)
}

fn append_slash_on_path(uri: Uri) -> Uri {
    let http::uri::Parts {
        scheme,
        authority,
        path_and_query,
        ..
    } = uri.into_parts();

    let mut builder = Uri::builder();
    if let Some(scheme) = scheme {
        builder = builder.scheme(scheme);
    }
    if let Some(authority) = authority {
        builder = builder.authority(authority);
    }
    if let Some(path_and_query) = path_and_query {
        if let Some(query) = path_and_query.query() {
            builder = builder.path_and_query(format!("{}/?{}", path_and_query.path(), query));
        } else {
            builder = builder.path_and_query(format!("{}/", path_and_query.path()));
        }
    } else {
        builder = builder.path_and_query("/");
    }

    builder.build().unwrap()
}

enum Output {
    File(File, HeaderValue, usize, Option<Encoding>),
    Redirect(HeaderValue),
    NotFound,
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'static>>;

enum Inner {
    Valid(BoxFuture<io::Result<Output>>),
    Invalid,
}

/// Response future of [`ServeDir`].
pub struct ResponseFuture {
    inner: Inner,
}

impl Future for ResponseFuture {
    type Output = io::Result<Response<ResponseBody>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.inner {
            Inner::Valid(open_file_future) => {
                let (file, mime, chunk_size, maybe_encoding) =
                    match ready!(Pin::new(open_file_future).poll(cx)) {
                        Ok(Output::File(file, mime, chunk_size, maybe_encoding)) => {
                            (file, mime, chunk_size, maybe_encoding)
                        }

                        Ok(Output::Redirect(location)) => {
                            let res = Response::builder()
                                .header(http::header::LOCATION, location)
                                .status(StatusCode::TEMPORARY_REDIRECT)
                                .body(empty_body())
                                .unwrap();
                            return Poll::Ready(Ok(res));
                        }

                        Ok(Output::NotFound) => {
                            let res = Response::builder()
                                .status(StatusCode::NOT_FOUND)
                                .body(empty_body())
                                .unwrap();

                            return Poll::Ready(Ok(res));
                        }

                        Err(err) => {
                            return Poll::Ready(
                                super::response_from_io_error(err).map(|res| res.map(ResponseBody)),
                            )
                        }
                    };
                let body = AsyncReadBody::with_capacity(file, chunk_size).boxed();
                let body = ResponseBody(body);

                let mut res = Response::new(body);
                res.headers_mut().insert(header::CONTENT_TYPE, mime);

                if let Some(encoding) = maybe_encoding {
                    res.headers_mut()
                        .insert(header::CONTENT_ENCODING, encoding.into_header_value());
                }

                Poll::Ready(Ok(res))
            }
            Inner::Invalid => {
                let res = Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(empty_body())
                    .unwrap();

                Poll::Ready(Ok(res))
            }
        }
    }
}

fn empty_body() -> ResponseBody {
    let body = Empty::new().map_err(|err| match err {}).boxed();
    ResponseBody(body)
}

opaque_body! {
    /// Response body for [`ServeDir`].
    pub type ResponseBody = BoxBody<Bytes, io::Error>;
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    #[allow(unused_imports)]
    use super::*;
    use brotli::BrotliDecompress;
    use flate2::bufread::{DeflateDecoder, GzDecoder};
    use http::{Request, StatusCode};
    use http_body::Body as HttpBody;
    use hyper::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn basic() {
        let svc = ServeDir::new("..");

        let req = Request::builder()
            .uri("/README.md")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = body_into_text(res.into_body()).await;

        let contents = std::fs::read_to_string("../README.md").unwrap();
        assert_eq!(body, contents);
    }

    #[tokio::test]
    async fn with_custom_chunk_size() {
        let svc = ServeDir::new("..").with_buf_chunk_size(1024 * 32);

        let req = Request::builder()
            .uri("/README.md")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = body_into_text(res.into_body()).await;

        let contents = std::fs::read_to_string("../README.md").unwrap();
        assert_eq!(body, contents);
    }

    #[tokio::test]
    async fn precompressed_gzip() {
        let svc = ServeDir::new("../test-files").precompressed_gzip();

        let req = Request::builder()
            .uri("/precompressed.txt")
            .header("Accept-Encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "gzip");

        let body = res.into_body().data().await.unwrap().unwrap();
        let mut decoder = GzDecoder::new(&body[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert!(decompressed.starts_with("\"This is a test file!\""));
    }

    #[tokio::test]
    async fn precompressed_br() {
        let svc = ServeDir::new("../test-files").precompressed_br();

        let req = Request::builder()
            .uri("/precompressed.txt")
            .header("Accept-Encoding", "br")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

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
        let svc = ServeDir::new("../test-files").precompressed_deflate();
        let request = Request::builder()
            .uri("/precompressed.txt")
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
    async fn unsupported_precompression_alogrithm_fallbacks_to_uncompressed() {
        let svc = ServeDir::new("../test-files").precompressed_gzip();

        let request = Request::builder()
            .uri("/precompressed.txt")
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
        let svc = ServeDir::new("../test-files").precompressed_gzip();

        let request = Request::builder()
            .uri("/only_gzipped.txt")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(request).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        // Should reply with gzipped file if client supports it
        let request = Request::builder()
            .uri("/only_gzipped.txt")
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
    async fn access_to_sub_dirs() {
        let svc = ServeDir::new("..");

        let req = Request::builder()
            .uri("/tower-http/Cargo.toml")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/x-toml");

        let body = body_into_text(res.into_body()).await;

        let contents = std::fs::read_to_string("Cargo.toml").unwrap();
        assert_eq!(body, contents);
    }

    #[tokio::test]
    async fn not_found() {
        let svc = ServeDir::new("..");

        let req = Request::builder()
            .uri("/not-found")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert!(res.headers().get(header::CONTENT_TYPE).is_none());

        let body = body_into_text(res.into_body()).await;
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn redirect_to_trailing_slash_on_dir() {
        let svc = ServeDir::new(".");

        let req = Request::builder().uri("/src").body(Body::empty()).unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);

        let location = &res.headers()[http::header::LOCATION];
        assert_eq!(location, "/src/");
    }

    #[tokio::test]
    async fn empty_directory_without_index() {
        let svc = ServeDir::new(".").append_index_html_on_directories(false);

        let req = Request::new(Body::empty());
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert!(res.headers().get(header::CONTENT_TYPE).is_none());

        let body = body_into_text(res.into_body()).await;
        assert!(body.is_empty());
    }

    async fn body_into_text<B>(body: B) -> String
    where
        B: HttpBody<Data = bytes::Bytes> + Unpin,
        B::Error: std::fmt::Debug,
    {
        let bytes = hyper::body::to_bytes(body).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn access_cjk_percent_encoded_uri_path() {
        let cjk_filename = "你好世界.txt";
        // percent encoding present of 你好世界.txt
        let cjk_filename_encoded = "%E4%BD%A0%E5%A5%BD%E4%B8%96%E7%95%8C.txt";

        let tmp_dir = std::env::temp_dir();
        let tmp_filename = std::path::Path::new(tmp_dir.as_path()).join(cjk_filename);
        let _ = tokio::fs::File::create(&tmp_filename).await.unwrap();

        let svc = ServeDir::new(&tmp_dir);

        let req = Request::builder()
            .uri(format!("/{}", cjk_filename_encoded))
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/plain");
        let _ = tokio::fs::remove_file(&tmp_filename).await.unwrap();
    }

    #[tokio::test]
    async fn access_space_percent_encoded_uri_path() {
        let raw_filename = "filename with space.txt";
        // percent encoding present of "filename with space.txt"
        let encoded_filename = "filename%20with%20space.txt";

        let tmp_dir = std::env::temp_dir();
        let tmp_filename = std::path::Path::new(tmp_dir.as_path()).join(raw_filename);
        let _ = tokio::fs::File::create(&tmp_filename).await.unwrap();

        let svc = ServeDir::new(&tmp_dir);

        let req = Request::builder()
            .uri(format!("/{}", encoded_filename))
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/plain");
        let _ = tokio::fs::remove_file(&tmp_filename).await.unwrap();
    }
}
