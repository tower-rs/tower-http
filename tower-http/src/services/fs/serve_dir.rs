use super::{
    check_modified_headers, open_file_with_fallback, AsyncReadBody, IfModifiedSince,
    IfUnmodifiedSince, LastModified, PrecompressedVariants,
};
use crate::{
    content_encoding::{encodings, Encoding},
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
                            return None
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
    ///     // respond with `index.html` for missing files
    ///     .fallback(ServeFile::new("assets/index.html"));
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
    ///     // respond with `404 Not Found` and the contents of `index.html` for missing files
    ///     .not_found_service(ServeFile::new("assets/index.html"));
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
    uri: Uri,
    append_index_html_on_directories: bool,
) -> Option<Output> {
    if !uri.path().ends_with('/') {
        if is_dir(full_path).await {
            let location = HeaderValue::from_str(&append_slash_on_path(uri).to_string()).unwrap();
            return Some(Output::Redirect(location));
        } else {
            return None;
        }
    } else if is_dir(full_path).await {
        if append_index_html_on_directories {
            full_path.push("index.html");
            return None;
        } else {
            return Some(Output::StatusCode(StatusCode::NOT_FOUND));
        }
    }
    None
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

        // The negotiated encodings based on the Accept-Encoding header and
        // precompressed variants
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

        let open_file_future = Box::pin(async move {
            let mime = match variant {
                ServeVariant::Directory {
                    append_index_html_on_directories,
                } => {
                    // Might already at this point know a redirect or not found result should be
                    // returned which corresponds to a Some(output). Otherwise the path might be
                    // modified and proceed to the open file/metadata future.
                    if let Some(output) = maybe_redirect_or_append_path(
                        &mut full_path,
                        uri,
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

            match request_method {
                Method::HEAD => {
                    let (meta, maybe_encoding) =
                        file_metadata_with_fallback(full_path, negotiated_encodings).await?;

                    let last_modified = meta.modified().ok().map(LastModified::from);
                    if let Some(status_code) = check_modified_headers(
                        last_modified.as_ref(),
                        if_unmodified_since,
                        if_modified_since,
                    ) {
                        return Ok(Output::StatusCode(status_code));
                    }

                    let maybe_range = try_parse_range(range_header.as_ref(), meta.len());
                    Ok(Output::File(FileRequest {
                        extent: FileRequestExtent::Head(meta),
                        chunk_size: buf_chunk_size,
                        mime_header_value: mime,
                        maybe_encoding,
                        maybe_range,
                        last_modified,
                    }))
                }
                _ => {
                    let (mut file, maybe_encoding) =
                        open_file_with_fallback(full_path, negotiated_encodings).await?;
                    let meta = file.metadata().await?;
                    let last_modified = meta.modified().ok().map(LastModified::from);
                    if let Some(status_code) = check_modified_headers(
                        last_modified.as_ref(),
                        if_unmodified_since,
                        if_modified_since,
                    ) {
                        return Ok(Output::StatusCode(status_code));
                    }

                    let maybe_range = try_parse_range(range_header.as_ref(), meta.len());
                    if let Some(Ok(ranges)) = maybe_range.as_ref() {
                        // If there is any other amount of ranges than 1 we'll return an unsatisfiable later as there isn't yet support for multipart ranges
                        if ranges.len() == 1 {
                            file.seek(SeekFrom::Start(*ranges[0].start())).await?;
                        }
                    }
                    Ok(Output::File(FileRequest {
                        extent: FileRequestExtent::Full(file, meta),
                        chunk_size: buf_chunk_size,
                        mime_header_value: mime,
                        maybe_encoding,
                        maybe_range,
                        last_modified,
                    }))
                }
            }
        });

        ResponseFuture {
            inner: ResponseFutureInner::OpenFileFuture {
                future: open_file_future,
                fallback_and_request,
            },
        }
    }
}

fn try_parse_range(
    maybe_range_ref: Option<&String>,
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

#[allow(clippy::large_enum_variant)]
enum Output {
    File(FileRequest),
    Redirect(HeaderValue),
    StatusCode(StatusCode),
}

struct FileRequest {
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
                    Ok(Output::File(file_request)) => {
                        let (maybe_file, size) = match file_request.extent {
                            FileRequestExtent::Full(file, meta) => (Some(file), meta.len()),
                            FileRequestExtent::Head(meta) => (None, meta.len()),
                        };
                        let mut builder = Response::builder()
                            .header(header::CONTENT_TYPE, file_request.mime_header_value)
                            .header(header::ACCEPT_RANGES, "bytes");
                        if let Some(encoding) = file_request.maybe_encoding {
                            builder = builder
                                .header(header::CONTENT_ENCODING, encoding.into_header_value());
                        }
                        if let Some(last_modified) = file_request.last_modified {
                            builder =
                                builder.header(header::LAST_MODIFIED, last_modified.0.to_string());
                        }

                        let res = handle_file_request(
                            builder,
                            maybe_file,
                            file_request.maybe_range,
                            file_request.chunk_size,
                            size,
                        );
                        return Poll::Ready(Ok(res.unwrap()));
                    }

                    Ok(Output::Redirect(location)) => {
                        let res = Response::builder()
                            .header(http::header::LOCATION, location)
                            .status(StatusCode::TEMPORARY_REDIRECT)
                            .body(empty_body())
                            .unwrap();
                        return Poll::Ready(Ok(res));
                    }

                    Ok(Output::StatusCode(code)) => {
                        let res = Response::builder().status(code).body(empty_body()).unwrap();

                        return Poll::Ready(Ok(res));
                    }

                    Err(err) => match err.kind() {
                        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                            if let Some((mut fallback, request)) = fallback_and_request.take() {
                                call_fallback(&mut fallback, request)
                            } else {
                                return Poll::Ready(not_found());
                            }
                        }
                        _ => return Poll::Ready(Err(err)),
                    },
                },

                ResponseFutureInnerProj::FallbackFuture { future } => {
                    return Pin::new(future).poll(cx)
                }

                ResponseFutureInnerProj::InvalidPath => {
                    return Poll::Ready(not_found());
                }
            };

            this.inner.set(new_state);
        }
    }
}

fn handle_file_request(
    builder: Builder,
    maybe_file: Option<File>,
    maybe_range: Option<Result<Vec<RangeInclusive<u64>>, RangeUnsatisfiableError>>,
    chunk_size: usize,
    size: u64,
) -> Result<Response<ResponseBody>, http::Error> {
    match maybe_range {
        Some(Ok(ranges)) => {
            if let Some(range) = ranges.first() {
                if ranges.len() > 1 {
                    builder
                        .header(header::CONTENT_RANGE, format!("bytes */{}", size))
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .body(body_from_bytes(Bytes::from(
                            "Cannot serve multipart range requests",
                        )))
                } else {
                    let range_size = range.end() - range.start() + 1;
                    let body = if let Some(file) = maybe_file {
                        let body =
                            AsyncReadBody::with_capacity_limited(file, chunk_size, range_size)
                                .boxed_unsync();
                        ResponseBody::new(body)
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
                }
            } else {
                builder
                    .header(header::CONTENT_RANGE, format!("bytes */{}", size))
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .body(body_from_bytes(Bytes::from(
                        "No range found after parsing range header, please file an issue",
                    )))
            }
        }
        Some(Err(_)) => builder
            .header(header::CONTENT_RANGE, format!("bytes */{}", size))
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .body(empty_body()),
        // Not a range request
        None => {
            let body = if let Some(file) = maybe_file {
                let box_body = AsyncReadBody::with_capacity(file, chunk_size).boxed_unsync();
                ResponseBody::new(box_body)
            } else {
                empty_body()
            };
            builder
                .header(header::CONTENT_LENGTH, size.to_string())
                .body(body)
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
        // contains `Infallible` so can never be called
        unreachable!()
    }

    fn call(&mut self, _req: Request<ReqBody>) -> Self::Future {
        // contains `Infallible` so can never be called
        unreachable!()
    }
}

fn not_found() -> io::Result<Response<ResponseBody>> {
    let res = Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(empty_body())
        .unwrap();
    Ok(res)
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

#[cfg(test)]
mod tests {
    use crate::services::ServeFile;

    use super::*;
    use brotli::BrotliDecompress;
    use flate2::bufread::{DeflateDecoder, GzDecoder};
    use http::{Request, StatusCode};
    use http_body::Body as HttpBody;
    use hyper::Body;
    use std::io::Read;
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
    async fn basic_with_index() {
        let svc = ServeDir::new("../test-files");

        let req = Request::new(Body::empty());
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "text/html");

        let body = body_into_text(res.into_body()).await;
        assert_eq!(body, "<b>HTML!</b>\n");
    }

    #[tokio::test]
    async fn head_request() {
        let svc = ServeDir::new("../test-files");

        let req = Request::builder()
            .uri("/precompressed.txt")
            .method(Method::HEAD)
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-length"], "23");

        let body = res.into_body().data().await;
        assert!(body.is_none());
    }

    #[tokio::test]
    async fn precompresed_head_request() {
        let svc = ServeDir::new("../test-files").precompressed_gzip();

        let req = Request::builder()
            .uri("/precompressed.txt")
            .header("Accept-Encoding", "gzip")
            .method(Method::HEAD)
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "gzip");
        assert_eq!(res.headers()["content-length"], "59");

        let body = res.into_body().data().await;
        assert!(body.is_none());
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
    async fn missing_precompressed_variant_fallbacks_to_uncompressed() {
        let svc = ServeDir::new("../test-files").precompressed_gzip();

        let request = Request::builder()
            .uri("/missing_precompressed.txt")
            .header("Accept-Encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        // Uncompressed file is served because compressed version is missing
        assert!(res.headers().get("content-encoding").is_none());

        let body = res.into_body().data().await.unwrap().unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.starts_with("Test file!"));
    }

    #[tokio::test]
    async fn missing_precompressed_variant_fallbacks_to_uncompressed_for_head_request() {
        let svc = ServeDir::new("../test-files").precompressed_gzip();

        let request = Request::builder()
            .uri("/missing_precompressed.txt")
            .header("Accept-Encoding", "gzip")
            .method(Method::HEAD)
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(request).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-length"], "11");
        // Uncompressed file is served because compressed version is missing
        assert!(res.headers().get("content-encoding").is_none());

        assert!(res.into_body().data().await.is_none());
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
    async fn not_found_precompressed() {
        let svc = ServeDir::new("../test-files").precompressed_gzip();

        let req = Request::builder()
            .uri("/not-found")
            .header("Accept-Encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert!(res.headers().get(header::CONTENT_TYPE).is_none());

        let body = body_into_text(res.into_body()).await;
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn fallbacks_to_different_precompressed_variant_if_not_found_for_head_request() {
        let svc = ServeDir::new("../test-files")
            .precompressed_gzip()
            .precompressed_br();

        let req = Request::builder()
            .uri("/precompressed_br.txt")
            .header("Accept-Encoding", "gzip,br,deflate")
            .method(Method::HEAD)
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "br");
        assert_eq!(res.headers()["content-length"], "15");

        assert!(res.into_body().data().await.is_none());
    }

    #[tokio::test]
    async fn fallbacks_to_different_precompressed_variant_if_not_found() {
        let svc = ServeDir::new("../test-files")
            .precompressed_gzip()
            .precompressed_br();

        let req = Request::builder()
            .uri("/precompressed_br.txt")
            .header("Accept-Encoding", "gzip,br,deflate")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/plain");
        assert_eq!(res.headers()["content-encoding"], "br");

        let body = res.into_body().data().await.unwrap().unwrap();
        let mut decompressed = Vec::new();
        BrotliDecompress(&mut &body[..], &mut decompressed).unwrap();
        let decompressed = String::from_utf8(decompressed.to_vec()).unwrap();
        assert!(decompressed.starts_with("Test file"));
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
        // percent encoding present of 你好世界.txt
        let cjk_filename_encoded = "%E4%BD%A0%E5%A5%BD%E4%B8%96%E7%95%8C.txt";

        let svc = ServeDir::new("../test-files");

        let req = Request::builder()
            .uri(format!("/{}", cjk_filename_encoded))
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/plain");
    }

    #[tokio::test]
    async fn access_space_percent_encoded_uri_path() {
        let encoded_filename = "filename%20with%20space.txt";

        let svc = ServeDir::new("../test-files");

        let req = Request::builder()
            .uri(format!("/{}", encoded_filename))
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/plain");
    }

    #[tokio::test]
    async fn read_partial_in_bounds() {
        let svc = ServeDir::new("..");
        let bytes_start_incl = 9;
        let bytes_end_incl = 1023;

        let req = Request::builder()
            .uri("/README.md")
            .header(
                "Range",
                format!("bytes={}-{}", bytes_start_incl, bytes_end_incl),
            )
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        let file_contents = std::fs::read("../README.md").unwrap();
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            res.headers()["content-length"],
            (bytes_end_incl - bytes_start_incl + 1).to_string()
        );
        assert!(res.headers()["content-range"]
            .to_str()
            .unwrap()
            .starts_with(&format!(
                "bytes {}-{}/{}",
                bytes_start_incl,
                bytes_end_incl,
                file_contents.len()
            )));
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
            .header(
                "Range",
                format!("bytes={}-{}", bytes_start_incl, requested_len - 1),
            )
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        let file_contents = std::fs::read("../README.md").unwrap();
        assert_eq!(
            res.headers()["content-range"],
            &format!("bytes */{}", file_contents.len())
        )
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
        assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        let file_contents = std::fs::read("../README.md").unwrap();
        assert_eq!(
            res.headers()["content-range"],
            &format!("bytes */{}", file_contents.len())
        )
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
        let file_contents = std::fs::read("../README.md").unwrap();
        assert_eq!(
            res.headers()["content-range"],
            &format!("bytes */{}", file_contents.len())
        )
    }
    #[tokio::test]
    async fn last_modified() {
        let svc = ServeDir::new("..");
        let req = Request::builder()
            .uri("/README.md")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let last_modified = res
            .headers()
            .get(header::LAST_MODIFIED)
            .expect("Missing last modified header!");

        // -- If-Modified-Since

        let svc = ServeDir::new("..");
        let req = Request::builder()
            .uri("/README.md")
            .header(header::IF_MODIFIED_SINCE, last_modified)
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
        let body = res.into_body().data().await;
        assert!(body.is_none());

        let svc = ServeDir::new("..");
        let req = Request::builder()
            .uri("/README.md")
            .header(header::IF_MODIFIED_SINCE, "Fri, 09 Aug 1996 14:21:40 GMT")
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let readme_bytes = include_bytes!("../../../../README.md");
        let body = res.into_body().data().await.unwrap().unwrap();
        assert_eq!(body.as_ref(), readme_bytes);

        // -- If-Unmodified-Since

        let svc = ServeDir::new("..");
        let req = Request::builder()
            .uri("/README.md")
            .header(header::IF_UNMODIFIED_SINCE, last_modified)
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().data().await.unwrap().unwrap();
        assert_eq!(body.as_ref(), readme_bytes);

        let svc = ServeDir::new("..");
        let req = Request::builder()
            .uri("/README.md")
            .header(header::IF_UNMODIFIED_SINCE, "Fri, 09 Aug 1996 14:21:40 GMT")
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::PRECONDITION_FAILED);
        let body = res.into_body().data().await;
        assert!(body.is_none());
    }

    #[tokio::test]
    async fn with_fallback_svc() {
        async fn fallback<B>(_: Request<B>) -> io::Result<Response<Body>> {
            Ok(Response::new(Body::from("from fallback")))
        }

        let svc = ServeDir::new("..").fallback(tower::service_fn(fallback));

        let req = Request::builder()
            .uri("/doesnt-exist")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);

        let body = body_into_text(res.into_body()).await;
        assert_eq!(body, "from fallback");
    }

    #[tokio::test]
    async fn with_fallback_serve_file() {
        let svc = ServeDir::new("..").fallback(ServeFile::new("../README.md"));

        let req = Request::builder()
            .uri("/doesnt-exist")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = body_into_text(res.into_body()).await;

        let contents = std::fs::read_to_string("../README.md").unwrap();
        assert_eq!(body, contents);
    }
}
