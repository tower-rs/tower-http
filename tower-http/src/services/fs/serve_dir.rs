use super::AsyncReadBody;
use crate::services::fs::DEFAULT_CAPACITY;
use bytes::Bytes;
use futures_util::ready;
use http::{header, HeaderValue, Request, Response, StatusCode, Uri, HeaderMap};
use http_body::{combinators::BoxBody, Body, Empty, Full};
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
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::SeekFrom;

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
                method: req.method().clone(),
                inner: Inner::Invalid,
            };
        };

        let mut full_path = self.base.clone();
        for seg in path_decoded.split('/') {
            if seg.starts_with("..") || seg.contains('\\') {
                return ResponseFuture {
                    method: req.method().clone(),
                    inner: Inner::Invalid,
                };
            }
            full_path.push(seg);
        }

        let append_index_html_on_directories = self.append_index_html_on_directories;
        let buf_chunk_size = self.buf_chunk_size;
        let uri = req.uri().clone();
        let range_request = parse_range(req.headers());

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
            let mut file = File::open(&full_path).await?;
            let file_size = file.metadata().await?.len();
            let guess = mime_guess::from_path(&full_path);
            let mime = guess
                .first_raw()
                .map(|mime| HeaderValue::from_static(mime))
                .unwrap_or_else(|| {
                    HeaderValue::from_str(mime::APPLICATION_OCTET_STREAM.as_ref()).unwrap()
                });

            if let Some(range) = range_request {
                if let Ok(mut content_range) = range {
                    content_range.adjust_end_if_out_of_bounds(file_size);
                    let mut buf = vec![0 as u8; content_range.range_size() as usize];
                    file.seek(SeekFrom::Start(content_range.start_inclusive)).await.unwrap();
                    file.read_exact(&mut buf).await.unwrap();
                    let bytes = Bytes::from(buf);
                    Ok(Output::Range(ContentRange {
                        start_inclusive: content_range.start_inclusive,
                        end_inclusive: content_range.end_inclusive,
                        total_size: file_size,
                        bytes,
                        mime_header_value: mime
                    }))
                } else {
                    Err(range.err().unwrap())
                }
            } else {
                Ok(Output::File(FileRequest {
                    file,
                    chunk_size: buf_chunk_size,
                    total_size: file_size,
                    mime_header_value: mime,
                }))
            }
        });

        ResponseFuture {
            method: req.method().clone(),
            inner: Inner::Valid(open_file_future),
        }
    }
}

fn parse_range(headers: &HeaderMap) -> Option<Result<CntRange, io::Error>> {
    // ex:  `Range: bytes=0-1023`
    let range_header = headers.get(header::RANGE)?;
    if let Ok(as_str) = range_header.to_str() {
        // produces ("bytes", "0-1023")
        return if let Some((_, range)) = as_str.split_once("=") {
            // produces ("0", "1023")
            if let Some((start_incl, end_incl)) = range.split_once("-") {
                if let Ok(start) = start_incl.parse() {
                    if let Ok(end) = end_incl.parse() {
                        return Some(Ok(CntRange {
                            start_inclusive: start,
                            end_inclusive: end
                        }));
                    }
                }
            }
            Some(Err(to_invalid_input(&format!("Range header range not correctly formatted \
            `bytes=<start_inclusive_index>-<end_inclusive_index>`, indices should be positive byte-indices \
            value supplied was {}", range))))
        } else {
            Some(Err(to_invalid_input("Range header value not in format `bytes=<range>`, `=` missing")))
        }
    }
    Some(Err(to_invalid_input( "Range not parseable as utf8")))
}

fn to_invalid_input(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
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
    File(FileRequest),
    Range(ContentRange),
    Redirect(HeaderValue),
    InvalidRange(u64),
    NotFound,
}

struct FileRequest {
    file: File,
    chunk_size: usize,
    total_size: u64,
    mime_header_value: HeaderValue,
}

struct ContentRange {
    start_inclusive: u64,
    end_inclusive: u64,
    total_size: u64,
    bytes: Bytes,
    mime_header_value: HeaderValue,
}

struct CntRange {
    start_inclusive: u64,
    end_inclusive: u64,
}

impl CntRange {
    fn range_size(&self) -> u64 {
        self.end_inclusive - self.start_inclusive + 1
    }

    fn adjust_end_if_out_of_bounds(&mut self, file_size: u64) {
        if self.end_inclusive >= file_size {
            self.end_inclusive = file_size - 1
        }
    }
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'static>>;

enum Inner {
    Valid(BoxFuture<io::Result<Output>>),
    Invalid,
}

/// Response future of [`ServeDir`].
pub struct ResponseFuture {
    method: http::Method,
    inner: Inner,
}

impl Future for ResponseFuture {
    type Output = io::Result<Response<ResponseBody>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.inner {
            Inner::Valid(open_file_future) => {
                let (builder, body, length) = match ready!(Pin::new(open_file_future).poll(cx)) {
                    Ok(Output::File(file_request)) => {
                        let builder = Response::builder()
                            .header(header::CONTENT_TYPE, file_request.mime_header_value);
                        let body = AsyncReadBody::with_capacity(file_request.file, file_request.chunk_size).boxed();
                        let body = ResponseBody(body);
                        (builder, body, file_request.total_size)
                    },

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

                    Ok(Output::InvalidRange) => {
                    }

                    Ok(Output::Range(content)) => {
                        let builder = Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", content.start_inclusive, content.end_inclusive, content.total_size))
                            .header(header::CONTENT_LENGTH, content.bytes.len())
                            .header(header::CONTENT_TYPE, content.mime_header_value);

                        (builder, body_from_bytes(content.bytes), content.total_size)
                    }

                    Err(err) => {
                        return Poll::Ready(
                            super::response_from_io_error(err).map(|res| res.map(ResponseBody)),
                        )
                    }
                };
                let builder = builder.header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_LENGTH, length);
                if self.method != http::Method::HEAD {
                    Poll::Ready(Ok(builder
                        .body(body)
                        .unwrap()))
                } else {
                    Poll::Ready(Ok(builder
                        .body(empty_body())
                        .unwrap()))
                }
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

fn body_from_bytes(bytes: Bytes) -> ResponseBody {
    let body = BoxBody::new(Full::from(bytes))
        .map_err(|err| match err { }).boxed();
    ResponseBody(body)
}

opaque_body! {
    /// Response body for [`ServeDir`].
    pub type ResponseBody = BoxBody<Bytes, io::Error>;
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
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
    async fn basic_tolerates_head_requests() {
        let svc = ServeDir::new("..");

        let req = Request::builder()
            .method(http::Method::HEAD)
            .uri("/README.md")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        let contents = std::fs::read("../README.md").unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/markdown");
        assert_eq!(res.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(res.headers()[header::CONTENT_LENGTH], contents.len().to_string());

        let body = hyper::body::to_bytes(res.into_body()).await.unwrap().to_vec();
        assert!(body.is_empty());
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

    #[tokio::test]
    async fn read_partial_in_bounds() {
        let svc = ServeDir::new("..");
        let bytes_start_incl = 9;
        let bytes_end_incl = 1023;
        let requested_len = bytes_end_incl - bytes_start_incl + 1;

        let req = Request::builder()
            .uri("/README.md")
            .header("Range", format!("bytes={}-{}", bytes_start_incl, bytes_end_incl))
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        let file_contents = std::fs::read("../README.md").unwrap();
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(res.headers()["content-length"], requested_len.to_string());
        assert!(res.headers()["content-range"].to_str().unwrap().starts_with(&format!("bytes {}-{}/{}", bytes_start_incl, bytes_end_incl, file_contents.len())));
        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = hyper::body::to_bytes(res.into_body()).await.ok().unwrap();
        let source = Bytes::from(file_contents[bytes_start_incl..=bytes_end_incl].to_vec());
        assert_eq!(body, source);
    }

    #[tokio::test]
    async fn read_partial_handles_file_out_of_bounds() {
        let svc = ServeDir::new("..");
        let bytes_start_incl = 0;
        let bytes_end_excl = 9999999;
        let requested_len = bytes_end_excl - bytes_start_incl;
        let file_contents = std::fs::read("../README.md").unwrap();

        let req = Request::builder()
            .uri("/README.md")
            .header("Range", format!("bytes={}-{}", bytes_start_incl, requested_len - 1))
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(res.headers()["content-length"], file_contents.len().to_string());
        assert!(res.headers()["content-range"].to_str().unwrap().starts_with(&format!("bytes {}-{}/{}", bytes_start_incl, file_contents.len() - 1, file_contents.len())));
        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = hyper::body::to_bytes(res.into_body()).await.ok().unwrap();
        let source = Bytes::from(file_contents);
        assert_eq!(body, source);
    }

    #[tokio::test]
    async fn read_partial_errs_on_garbage_header() {
        let svc = ServeDir::new("..");
        let req = Request::builder()
            .uri("/README.md")
            .header("Range", "bad_format")
            .body(Body::empty())
            .unwrap();
        assert!(svc.oneshot(req).await.is_err());
    }


    #[tokio::test]
    async fn read_partial_errs_on_bad_range() {
        let svc = ServeDir::new("..");
        let req = Request::builder()
            .uri("/README.md")
            .header("Range", "bytes=-1-15")
            .body(Body::empty())
            .unwrap();
        assert!(svc.oneshot(req).await.is_err());
    }
}
