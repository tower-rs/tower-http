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
use tokio::io::{AsyncSeekExt};
use std::io::SeekFrom;
use crate::services::fs::parse_range::{ParsedRangeHeader, parse_range};
use std::ops::RangeBounds;

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
        let range_header = req.headers().get(header::RANGE)
            .and_then(|value| value.to_str().ok());

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
            let range_request = range_header.map(|range| parse_range(range, file_size));
            if let Some(ParsedRangeHeader::Range(ranges)) = range_request.as_ref() {
                if ranges.len() != 1 {
                    // do something smart
                }
                file.seek(SeekFrom::Start(*ranges[0].start())).await.unwrap();
            }
            Ok(Output::File(FileRequest{
                file,
                total_size: file_size,
                chunk_size: buf_chunk_size,
                mime_header_value: mime,
                range: range_request
            }))
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
    File(FileRequest),
    Redirect(HeaderValue),
    NotFound,
}

struct FileRequest {
    file: File,
    total_size: u64,
    chunk_size: usize,
    mime_header_value: HeaderValue,
    range: Option<ParsedRangeHeader>,
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
                let response = match ready!(Pin::new(open_file_future).poll(cx)) {
                    Ok(Output::File(file_request)) => {
                        let builder = Response::builder()
                            .header(header::CONTENT_TYPE, file_request.mime_header_value)
                            .header(header::ACCEPT_RANGES, "bytes")
                            .header(header::CONTENT_LENGTH, file_request.total_size.to_string());
                        if let Some(range) = file_request.range {
                            match range {
                                ParsedRangeHeader::Range(ranges) => {
                                    let range = &ranges[0];
                                    if *range.end() >= file_request.total_size {
                                        builder.header(header::CONTENT_RANGE, format!("bytes */{}", file_request.total_size))
                                            .status(StatusCode::RANGE_NOT_SATISFIABLE)
                                            .body(empty_body())
                                    } else {
                                        let size = (range.end() - range.start() - 1) as usize;
                                        let body = AsyncReadBody::with_capacity_limited(file_request.file, file_request.chunk_size, size).boxed();
                                        builder.header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", range.start(), range.end(), file_request.total_size))
                                            .status(StatusCode::PARTIAL_CONTENT)
                                            .body(ResponseBody(body))
                                    }

                                }
                                ParsedRangeHeader::Unsatisfiable(reason) => {
                                    builder.header(header::CONTENT_RANGE, format!("bytes */{}", file_request.total_size))
                                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                                        .body(body_from_bytes(Bytes::from(reason)))
                                }
                                ParsedRangeHeader::Malformed(reason) => {
                                    builder.header(header::CONTENT_RANGE, format!("bytes */{}", file_request.total_size))
                                        .status(StatusCode::BAD_REQUEST)
                                        .body(body_from_bytes(Bytes::from(reason)))
                                }
                            }
                        } else {
                            let body = AsyncReadBody::with_capacity(file_request.file, file_request.chunk_size).boxed();
                            let body = ResponseBody(body);
                            builder.body(body)
                        }
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

                    Err(err) => {
                        return Poll::Ready(
                            super::response_from_io_error(err).map(|res| res.map(ResponseBody)),
                        )
                    }
                };
                Poll::Ready(Ok(response.unwrap()))
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

        let req = Request::builder()
            .uri("/README.md")
            .header("Range", format!("bytes={}-{}", bytes_start_incl, bytes_end_incl))
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        let file_contents = std::fs::read("../README.md").unwrap();
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(res.headers()["content-length"], file_contents.len().to_string());
        assert!(res.headers()["content-range"].to_str().unwrap().starts_with(&format!("bytes {}-{}/{}", bytes_start_incl, bytes_end_incl, file_contents.len())));
        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = hyper::body::to_bytes(res.into_body()).await.ok().unwrap();
        let source = Bytes::from(file_contents[bytes_start_incl..=bytes_end_incl].to_vec());
        assert_eq!(body, source);
    }

    #[tokio::test]
    async fn read_partial_rejects_out_of_bounds_range() {
        let svc = ServeDir::new("..");
        let bytes_start_incl = 0;
        let bytes_end_excl = 9999999;
        let requested_len = bytes_end_excl - bytes_start_incl;

        let req = Request::builder()
            .uri("/README.md")
            .header("Range", format!("bytes={}-{}", bytes_start_incl, requested_len - 1))
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    #[tokio::test]
    async fn read_partial_errs_on_garbage_header() {
        let svc = ServeDir::new("..");
        let req = Request::builder()
            .uri("/README.md")
            .header("Range", "bad_format")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }


    #[tokio::test]
    async fn read_partial_errs_on_bad_range() {
        let svc = ServeDir::new("..");
        let req = Request::builder()
            .uri("/README.md")
            .header("Range", "bytes=-1-15")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }
}
