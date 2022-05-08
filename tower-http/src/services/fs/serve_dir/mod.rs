use self::future::ResponseFuture;
use crate::{
    content_encoding::{encodings, SupportedEncodings},
    set_status::SetStatus,
};
use bytes::Bytes;
use http::{header, HeaderValue, Method, Request, Response, StatusCode};
use http_body::{combinators::UnsyncBoxBody, Empty};
use percent_encoding::percent_decode;
use std::{
    convert::Infallible,
    io,
    path::{Component, Path, PathBuf},
    task::{Context, Poll},
};
use tower_service::Service;

pub(crate) mod future;
mod headers;
mod open_file;

#[cfg(test)]
mod tests;

// default capacity 64KiB
const DEFAULT_CAPACITY: usize = 65536;

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
    call_fallback_on_method_not_allowed: bool,
}

impl ServeDir<DefaultServeDirFallback> {
    /// Create a new [`ServeDir`].
    pub fn new<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
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
            call_fallback_on_method_not_allowed: false,
        }
    }

    pub(crate) fn new_single_file<P>(path: P, mime: HeaderValue) -> Self
    where
        P: AsRef<Path>,
    {
        Self {
            base: path.as_ref().to_owned(),
            buf_chunk_size: DEFAULT_CAPACITY,
            precompressed_variants: None,
            variant: ServeVariant::SingleFile { mime },
            fallback: None,
            call_fallback_on_method_not_allowed: false,
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
            call_fallback_on_method_not_allowed: self.call_fallback_on_method_not_allowed,
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

    /// Customize whether or not to call the fallback for requests that aren't `GET` or `HEAD`.
    ///
    /// Defaults to not calling the fallback and instead returning `405 Method Not Allowed`.
    pub fn call_fallback_on_method_not_allowed(mut self, call_fallback: bool) -> Self {
        self.call_fallback_on_method_not_allowed = call_fallback;
        self
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
            if self.call_fallback_on_method_not_allowed {
                if let Some(fallback) = &mut self.fallback {
                    return ResponseFuture {
                        inner: future::call_fallback(fallback, req),
                    };
                }
            } else {
                return ResponseFuture::method_not_allowed();
            }
        }

        // `ServeDir` doesn't care about the request body but the fallback might. So move out the
        // body and pass it to the fallback, leaving an empty body in its place
        //
        // this is necessary because we cannot clone bodies
        let (mut parts, body) = req.into_parts();
        // same goes for extensions
        let extensions = std::mem::take(&mut parts.extensions);
        let req = Request::from_parts(parts, Empty::<Bytes>::new());

        let path_to_file = match self
            .variant
            .build_and_validate_path(&self.base, req.uri().path())
        {
            Some(path_to_file) => path_to_file,
            None => {
                return ResponseFuture::invalid_path();
            }
        };

        let fallback_and_request = self.fallback.as_mut().map(|fallback| {
            let mut fallback_req = Request::new(body);
            *fallback_req.method_mut() = req.method().clone();
            *fallback_req.uri_mut() = req.uri().clone();
            *fallback_req.headers_mut() = req.headers().clone();
            *fallback_req.extensions_mut() = extensions;

            // get the ready fallback and leave a non-ready clone in its place
            let clone = fallback.clone();
            let fallback = std::mem::replace(fallback, clone);

            (fallback, fallback_req)
        });

        let buf_chunk_size = self.buf_chunk_size;
        let range_header = req
            .headers()
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok())
            .map(|s| s.to_owned());

        let negotiated_encodings = encodings(
            req.headers(),
            self.precompressed_variants.unwrap_or_default(),
        );

        let variant = self.variant.clone();

        let open_file_future = Box::pin(open_file::open_file(
            variant,
            path_to_file,
            req,
            negotiated_encodings,
            range_header,
            buf_chunk_size,
        ));

        ResponseFuture::open_file_future(open_file_future, fallback_and_request)
    }
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

                let mut path_to_file = base_path.to_path_buf();
                for component in path_decoded.components() {
                    match component {
                        Component::Normal(comp) => {
                            // protect against paths like `/foo/c:/bar/baz` (#204)
                            if Path::new(&comp)
                                .components()
                                .all(|c| matches!(c, Component::Normal(_)))
                            {
                                path_to_file.push(comp)
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
                Some(path_to_file)
            }
            ServeVariant::SingleFile { mime: _ } => Some(base_path.to_path_buf()),
        }
    }
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

#[derive(Clone, Copy, Debug, Default)]
struct PrecompressedVariants {
    gzip: bool,
    deflate: bool,
    br: bool,
}

impl SupportedEncodings for PrecompressedVariants {
    fn gzip(&self) -> bool {
        self.gzip
    }

    fn deflate(&self) -> bool {
        self.deflate
    }

    fn br(&self) -> bool {
        self.br
    }
}
