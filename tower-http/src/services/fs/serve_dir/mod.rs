use super::{
    check_modified_headers, open_file_with_fallback, AsyncReadBody, IfModifiedSince,
    IfUnmodifiedSince, LastModified, PrecompressedVariants,
};
use crate::{
    content_encoding::{encodings, Encoding, QValue},
    services::fs::{file_metadata_with_fallback, DEFAULT_CAPACITY},
    set_status::SetStatus,
    BoxError,
};
use bytes::Bytes;
use futures_util::ready;
use http::response::Builder;
use http::{header, HeaderValue, Method, Request, Response, StatusCode, Uri};
use http_body::{combinators::UnsyncBoxBody, Body, Empty, Full};
use http_range_header::RangeUnsatisfiableError;
use percent_encoding::percent_decode;
use pin_project_lite::pin_project;
use std::{
    convert::Infallible,
    fs::Metadata,
    future::Future,
    io,
    io::SeekFrom,
    ops::RangeInclusive,
    path::{Component, Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{fs::File, io::AsyncSeekExt};
use tower_service::Service;

mod future;

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
#[derive(Clone, Debug)]
pub struct ServeDir<F = DefaultServeDirFallback> {
    base: PathBuf,
    buf_chunk_size: usize,
    precompressed_variants: Option<PrecompressedVariants>,
    // This is used to specialise implementation for
    // single files
    variant: ServeVariant,
    fallback: Option<F>,
}

// Allow the ServeDir service to be used in the ServeFile service
// with almost no overhead
#[derive(Clone, Debug)]
enum ServeVariant {
    Directory {
        append_index_html_on_directories: bool,
    },
    SingleFile {
        mime: HeaderValue,
    },
}

impl ServeVariant {
    fn build_and_validate_path(&self, base_path: &Path, requested_path: &str) -> Option<PathBuf> {
        match self {
            ServeVariant::Directory {
                append_index_html_on_directories: _,
            } => {
                let path = requested_path.trim_start_matches('/');

                let path_decoded = percent_decode(path.as_ref()).decode_utf8().ok()?;
                let path_decoded = Path::new(&*path_decoded);

                let mut full_path = base_path.to_path_buf();
                for component in path_decoded.components() {
                    match component {
                        Component::Normal(comp) => {
                            // protect against paths like `/foo/c:/bar/baz` (#204)
                            if Path::new(&comp)
                                .components()
                                .all(|c| matches!(c, Component::Normal(_)))
                            {
                                full_path.push(comp)
                            } else {
                                return None;
                            }
                        }
                        Component::CurDir => {}
                        Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                            return None;
                        }
                    }
                }
                Some(full_path)
            }
            ServeVariant::SingleFile { mime: _ } => Some(base_path.to_path_buf()),
        }
    }
}

impl ServeDir<DefaultServeDirFallback> {
    /// Create a new [`ServeDir`].
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let mut base = PathBuf::from(".");
        base.push(path.as_ref());

        Self {
            base,
            buf_chunk_size: DEFAULT_CAPACITY,
            precompressed_variants: None,
            variant: ServeVariant::Directory {
                append_index_html_on_directories: true,
            },
            fallback: None,
        }
    }

    pub(crate) fn new_single_file<P: AsRef<Path>>(path: P, mime: HeaderValue) -> Self {
        Self {
            base: path.as_ref().to_owned(),
            buf_chunk_size: DEFAULT_CAPACITY,
            precompressed_variants: None,
            variant: ServeVariant::SingleFile { mime },
            fallback: None,
        }
    }
}

impl<F> ServeDir<F> {
    /// If the requested path is a directory append `index.html`.
    ///
    /// This is useful for static sites.
    ///
    /// Defaults to `true`.
    pub fn append_index_html_on_directories(mut self, append: bool) -> Self {
        match &mut self.variant {
            ServeVariant::Directory {
                append_index_html_on_directories,
            } => {
                *append_index_html_on_directories = append;
                self
            }
            ServeVariant::SingleFile { mime: _ } => self,
        }
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

    /// Set the fallback service.
    ///
    /// This service will be called if there is no file at the path of the request.
    ///
    /// The status code returned by the fallback will not be altered. Use
    /// [`ServeDir::not_found_service`] to set a fallback and always respond with `404 Not Found`.
    ///
    /// # Example
    ///
    /// This can be used to respond with a different file:
    ///
    /// ```rust
    /// use tower_http::services::{ServeDir, ServeFile};
    ///
    /// let service = ServeDir::new("assets")
    ///     // respond with `not_found.html` for missing files
    ///     .fallback(ServeFile::new("assets/not_found.html"));
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
    pub fn fallback<F2>(self, new_fallback: F2) -> ServeDir<F2> {
        ServeDir {
            base: self.base,
            buf_chunk_size: self.buf_chunk_size,
            precompressed_variants: self.precompressed_variants,
            variant: self.variant,
            fallback: Some(new_fallback),
        }
    }

    /// Set the fallback service and override the fallback's status code to `404 Not Found`.
    ///
    /// This service will be called if there is no file at the path of the request.
    ///
    /// # Example
    ///
    /// This can be used to respond with a different file:
    ///
    /// ```rust
    /// use tower_http::services::{ServeDir, ServeFile};
    ///
    /// let service = ServeDir::new("assets")
    ///     // respond with `404 Not Found` and the contents of `not_found.html` for missing files
    ///     .not_found_service(ServeFile::new("assets/not_found.html"));
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
    /// Setups like this are often found in single page applications.
    pub fn not_found_service<F2>(self, new_fallback: F2) -> ServeDir<SetStatus<F2>> {
        self.fallback(SetStatus::new(new_fallback, StatusCode::NOT_FOUND))
    }
}

async fn maybe_redirect_or_append_path(
    full_path: &mut PathBuf,
    uri: &Uri,
    append_index_html_on_directories: bool,
) -> Option<Output> {
    if !uri.path().ends_with('/') {
        if is_dir(full_path).await {
            let location =
                HeaderValue::from_str(&append_slash_on_path(uri.clone()).to_string()).unwrap();
            Some(Output::Redirect { location })
        } else {
            None
        }
    } else if is_dir(full_path).await {
        if append_index_html_on_directories {
            full_path.push("index.html");
            None
        } else {
            Some(Output::StatusCode {
                status_code: StatusCode::NOT_FOUND,
            })
        }
    } else {
        None
    }
}

impl<ReqBody, F, FResBody> Service<Request<ReqBody>> for ServeDir<F>
where
    F: Service<Request<ReqBody>, Response = Response<FResBody>> + Clone,
    F::Error: Into<io::Error>,
    F::Future: Send + 'static,
    FResBody: http_body::Body<Data = Bytes> + Send + 'static,
    FResBody::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = Response<ResponseBody>;
    type Error = io::Error;
    type Future = ResponseFuture<ReqBody, F>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if let Some(fallback) = &mut self.fallback {
            fallback.poll_ready(cx).map_err(Into::into)
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.method() != Method::GET && req.method() != Method::HEAD {
            return ResponseFuture {
                inner: ResponseFutureInner::MethodNotAllowed,
            };
        }

        // `ServeDir` doesn't care about the request body but the fallback might. So move out the
        // body and pass it to the fallback, leaving an empty body in its place
        //
        // this is necessary because we cannot clone bodies
        let (mut parts, body) = req.into_parts();
        // same goes for extensions
        let extensions = std::mem::take(&mut parts.extensions);
        let req = Request::from_parts(parts, Empty::<Bytes>::new());

        let mut full_path = match self
            .variant
            .build_and_validate_path(&self.base, req.uri().path())
        {
            Some(full_path) => full_path,
            None => {
                return ResponseFuture {
                    inner: ResponseFutureInner::InvalidPath,
                };
            }
        };

        let fallback_and_request = self.fallback.as_mut().map(|fallback| {
            let mut req = Request::new(body);
            *req.method_mut() = req.method().clone();
            *req.uri_mut() = req.uri().clone();
            *req.headers_mut() = req.headers().clone();
            *req.extensions_mut() = extensions;

            // get the ready fallback and leave a non-ready clone in its place
            let clone = fallback.clone();
            let fallback = std::mem::replace(fallback, clone);

            (fallback, req)
        });

        let buf_chunk_size = self.buf_chunk_size;
        let uri = req.uri().clone();
        let range_header = req
            .headers()
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok().map(|s| s.to_owned()));

        let negotiated_encodings = encodings(
            req.headers(),
            self.precompressed_variants.unwrap_or_default(),
        );

        let if_unmodified_since = req
            .headers()
            .get(header::IF_UNMODIFIED_SINCE)
            .and_then(IfUnmodifiedSince::from_header_value);

        let if_modified_since = req
            .headers()
            .get(header::IF_MODIFIED_SINCE)
            .and_then(IfModifiedSince::from_header_value);

        let request_method = req.method().clone();
        let variant = self.variant.clone();

        let open_file_future = Box::pin(open_file(
            variant,
            full_path,
            req,
            negotiated_encodings,
            range_header,
            buf_chunk_size,
            if_unmodified_since,
            if_modified_since,
        ));

        ResponseFuture {
            inner: ResponseFutureInner::OpenFileFuture {
                future: open_file_future,
                fallback_and_request,
            },
        }
    }
}

// can we move more things into this function?
#[allow(clippy::too_many_arguments)]
async fn open_file(
    variant: ServeVariant,
    mut full_path: PathBuf,
    req: Request<Empty<Bytes>>,
    negotiated_encodings: Vec<(Encoding, QValue)>,
    range_header: Option<String>,
    buf_chunk_size: usize,
    if_unmodified_since: Option<IfUnmodifiedSince>,
    if_modified_since: Option<IfModifiedSince>,
) -> io::Result<Output> {
    let mime = match variant {
        ServeVariant::Directory {
            append_index_html_on_directories,
        } => {
            // Might already at this point know a redirect or not found result should be
            // returned which corresponds to a Some(output). Otherwise the path might be
            // modified and proceed to the open file/metadata future.
            if let Some(output) = maybe_redirect_or_append_path(
                &mut full_path,
                req.uri(),
                append_index_html_on_directories,
            )
            .await
            {
                return Ok(output);
            }
            let guess = mime_guess::from_path(&full_path);
            guess
                .first_raw()
                .map(HeaderValue::from_static)
                .unwrap_or_else(|| {
                    HeaderValue::from_str(mime::APPLICATION_OCTET_STREAM.as_ref()).unwrap()
                })
        }
        ServeVariant::SingleFile { mime } => mime,
    };

    if req.method() == Method::HEAD {
        let (meta, maybe_encoding) =
            file_metadata_with_fallback(full_path, negotiated_encodings).await?;

        let last_modified = meta.modified().ok().map(LastModified::from);
        if let Some(status_code) = check_modified_headers(
            last_modified.as_ref(),
            if_unmodified_since,
            if_modified_since,
        ) {
            return Ok(Output::StatusCode { status_code });
        }

        let maybe_range = try_parse_range(range_header.as_deref(), meta.len());

        Ok(Output::File {
            file_output: FileOutput {
                extent: FileRequestExtent::Head(meta),
                chunk_size: buf_chunk_size,
                mime_header_value: mime,
                maybe_encoding,
                maybe_range,
                last_modified,
            },
        })
    } else {
        let (mut file, maybe_encoding) =
            open_file_with_fallback(full_path, negotiated_encodings).await?;
        let meta = file.metadata().await?;
        let last_modified = meta.modified().ok().map(LastModified::from);
        if let Some(status_code) = check_modified_headers(
            last_modified.as_ref(),
            if_unmodified_since,
            if_modified_since,
        ) {
            return Ok(Output::StatusCode { status_code });
        }

        let maybe_range = try_parse_range(range_header.as_deref(), meta.len());
        if let Some(Ok(ranges)) = maybe_range.as_ref() {
            // if there is any other amount of ranges than 1 we'll return an
            // unsatisfiable later as there isn't yet support for multipart ranges
            if ranges.len() == 1 {
                file.seek(SeekFrom::Start(*ranges[0].start())).await?;
            }
        }

        Ok(Output::File {
            file_output: FileOutput {
                extent: FileRequestExtent::Full(file, meta),
                chunk_size: buf_chunk_size,
                mime_header_value: mime,
                maybe_encoding,
                maybe_range,
                last_modified,
            },
        })
    }
}

fn try_parse_range(
    maybe_range_ref: Option<&str>,
    file_size: u64,
) -> Option<Result<Vec<RangeInclusive<u64>>, RangeUnsatisfiableError>> {
    maybe_range_ref.map(|header_value| {
        http_range_header::parse_range_header(header_value)
            .and_then(|first_pass| first_pass.validate(file_size))
    })
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

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pin_project! {
    /// Response future of [`ServeDir`].
    pub struct ResponseFuture<ReqBody, F = DefaultServeDirFallback> {
        #[pin]
        inner: ResponseFutureInner<ReqBody, F>,
    }
}

pin_project! {
    #[project = ResponseFutureInnerProj]
    enum ResponseFutureInner<ReqBody, F> {
        OpenFileFuture {
            #[pin]
            future: BoxFuture<io::Result<Output>>,
            fallback_and_request: Option<(F, Request<ReqBody>)>,
        },
        FallbackFuture {
            future: BoxFuture<io::Result<Response<ResponseBody>>>,
        },
        InvalidPath,
        MethodNotAllowed,
    }
}

impl<F, ReqBody, ResBody> Future for ResponseFuture<ReqBody, F>
where
    F: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    F::Error: Into<io::Error>,
    F::Future: Send + 'static,
    ResBody: http_body::Body<Data = Bytes> + Send + 'static,
    ResBody::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Output = io::Result<Response<ResponseBody>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let mut this = self.as_mut().project();

            let new_state = match this.inner.as_mut().project() {
                ResponseFutureInnerProj::OpenFileFuture {
                    future: open_file_future,
                    fallback_and_request,
                } => match ready!(open_file_future.poll(cx)) {
                    Ok(Output::File { file_output }) => {
                        let res = file_output.build_response();
                        return Poll::Ready(Ok(res));
                    }

                    Ok(Output::Redirect { location }) => {
                        let mut res = response_with_status(StatusCode::TEMPORARY_REDIRECT);
                        res.headers_mut().insert(http::header::LOCATION, location);
                        return Poll::Ready(Ok(res));
                    }

                    Ok(Output::StatusCode { status_code }) => {
                        let res = response_with_status(status_code);
                        return Poll::Ready(Ok(res));
                    }

                    Err(err) => match err.kind() {
                        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                            if let Some((mut fallback, request)) = fallback_and_request.take() {
                                call_fallback(&mut fallback, request)
                            } else {
                                return Poll::Ready(Ok(not_found()));
                            }
                        }
                        _ => return Poll::Ready(Err(err)),
                    },
                },

                ResponseFutureInnerProj::FallbackFuture { future } => {
                    return Pin::new(future).poll(cx)
                }

                ResponseFutureInnerProj::InvalidPath => {
                    return Poll::Ready(Ok(not_found()));
                }

                ResponseFutureInnerProj::MethodNotAllowed => {
                    return Poll::Ready(Ok(method_not_allowed()));
                }
            };

            this.inner.set(new_state);
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum Output {
    File { file_output: FileOutput },
    Redirect { location: HeaderValue },
    StatusCode { status_code: StatusCode },
}

struct FileOutput {
    extent: FileRequestExtent,
    chunk_size: usize,
    mime_header_value: HeaderValue,
    maybe_encoding: Option<Encoding>,
    maybe_range: Option<Result<Vec<RangeInclusive<u64>>, RangeUnsatisfiableError>>,
    last_modified: Option<LastModified>,
}

enum FileRequestExtent {
    Full(File, Metadata),
    Head(Metadata),
}

impl FileOutput {
    fn build_response(self) -> Response<ResponseBody> {
        let (maybe_file, size) = match self.extent {
            FileRequestExtent::Full(file, meta) => (Some(file), meta.len()),
            FileRequestExtent::Head(meta) => (None, meta.len()),
        };

        let mut builder = Response::builder()
            .header(header::CONTENT_TYPE, self.mime_header_value)
            .header(header::ACCEPT_RANGES, "bytes");

        if let Some(encoding) = self.maybe_encoding {
            builder = builder.header(header::CONTENT_ENCODING, encoding.into_header_value());
        }

        if let Some(last_modified) = self.last_modified {
            builder = builder.header(header::LAST_MODIFIED, last_modified.0.to_string());
        }

        match self.maybe_range {
            Some(Ok(ranges)) => {
                if let Some(range) = ranges.first() {
                    if ranges.len() > 1 {
                        builder
                            .header(header::CONTENT_RANGE, format!("bytes */{}", size))
                            .status(StatusCode::RANGE_NOT_SATISFIABLE)
                            .body(body_from_bytes(Bytes::from(
                                "Cannot serve multipart range requests",
                            )))
                            .unwrap()
                    } else {
                        let body = if let Some(file) = maybe_file {
                            let range_size = range.end() - range.start() + 1;
                            ResponseBody::new(
                                AsyncReadBody::with_capacity_limited(
                                    file,
                                    self.chunk_size,
                                    range_size,
                                )
                                .boxed_unsync(),
                            )
                        } else {
                            empty_body()
                        };

                        builder
                            .header(
                                header::CONTENT_RANGE,
                                format!("bytes {}-{}/{}", range.start(), range.end(), size),
                            )
                            .header(header::CONTENT_LENGTH, range.end() - range.start() + 1)
                            .status(StatusCode::PARTIAL_CONTENT)
                            .body(body)
                            .unwrap()
                    }
                } else {
                    builder
                        .header(header::CONTENT_RANGE, format!("bytes */{}", size))
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .body(body_from_bytes(Bytes::from(
                            "No range found after parsing range header, please file an issue",
                        )))
                        .unwrap()
                }
            }

            Some(Err(_)) => builder
                .header(header::CONTENT_RANGE, format!("bytes */{}", size))
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .body(empty_body())
                .unwrap(),

            // Not a range request
            None => {
                let body = if let Some(file) = maybe_file {
                    ResponseBody::new(
                        AsyncReadBody::with_capacity(file, self.chunk_size).boxed_unsync(),
                    )
                } else {
                    empty_body()
                };

                builder
                    .header(header::CONTENT_LENGTH, size.to_string())
                    .body(body)
                    .unwrap()
            }
        }
    }
}

fn empty_body() -> ResponseBody {
    let body = Empty::new().map_err(|err| match err {}).boxed_unsync();
    ResponseBody::new(body)
}

fn body_from_bytes(bytes: Bytes) -> ResponseBody {
    let body = Full::from(bytes).map_err(|err| match err {}).boxed_unsync();
    ResponseBody::new(body)
}

opaque_body! {
    /// Response body for [`ServeDir`] and [`ServeFile`].
    pub type ResponseBody = UnsyncBoxBody<Bytes, io::Error>;
}

/// The default fallback service used with [`ServeDir`].
#[derive(Debug, Clone, Copy)]
pub struct DefaultServeDirFallback(Infallible);

impl<ReqBody> Service<Request<ReqBody>> for DefaultServeDirFallback
where
    ReqBody: Send + 'static,
{
    type Response = Response<ResponseBody>;
    type Error = io::Error;
    type Future = ResponseFuture<ReqBody>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.0 {}
    }

    fn call(&mut self, _req: Request<ReqBody>) -> Self::Future {
        match self.0 {}
    }
}

fn response_with_status(status: StatusCode) -> Response<ResponseBody> {
    Response::builder()
        .status(status)
        .body(empty_body())
        .unwrap()
}

fn not_found() -> Response<ResponseBody> {
    response_with_status(StatusCode::NOT_FOUND)
}

fn method_not_allowed() -> Response<ResponseBody> {
    response_with_status(StatusCode::METHOD_NOT_ALLOWED)
}

fn call_fallback<F, B, FResBody>(fallback: &mut F, req: Request<B>) -> ResponseFutureInner<B, F>
where
    F: Service<Request<B>, Response = Response<FResBody>> + Clone,
    F::Error: Into<io::Error>,
    F::Future: Send + 'static,
    FResBody: http_body::Body<Data = Bytes> + Send + 'static,
    FResBody::Error: Into<BoxError>,
{
    let future = fallback.call(req);
    let future = async move {
        let response = future.await.map_err(Into::into)?;
        let response = response
            .map(|body| {
                body.map_err(|err| match err.into().downcast::<io::Error>() {
                    Ok(err) => *err,
                    Err(err) => io::Error::new(io::ErrorKind::Other, err),
                })
                .boxed_unsync()
            })
            .map(ResponseBody::new);
        Ok(response)
    };
    let future = Box::pin(future);
    ResponseFutureInner::FallbackFuture { future }
}
