//! File system related services.

use bytes::Bytes;
use http::{HeaderMap, Response, StatusCode};
use http_body::{combinators::BoxBody, Body, Empty};
use pin_project_lite::pin_project;
use std::{ffi::OsStr, future::Future, path::PathBuf};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::fs::File;
use tokio::io::AsyncRead;

use futures_util::Stream;
use tokio_util::io::ReaderStream;

mod serve_dir;
mod serve_file;

// default capacity 64KiB
const DEFAULT_CAPACITY: usize = 65536;

use crate::content_encoding::{Encoding, SupportedEncodings};

pub use self::{
    serve_dir::{
        ResponseBody as ServeDirResponseBody, ResponseFuture as ServeDirResponseFuture, ServeDir,
    },
    serve_file::{
        ResponseBody as ServeFileResponseBody, ResponseFuture as ServeFileResponseFuture, ServeFile,
    },
};

#[derive(Clone, Copy, Debug)]
struct PrecompressedVariants {
    gzip: bool,
    deflate: bool,
    br: bool,
}

impl Default for PrecompressedVariants {
    fn default() -> Self {
        Self {
            gzip: false,
            deflate: false,
            br: false,
        }
    }
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

type FileFuture =
    Pin<Box<dyn Future<Output = io::Result<(File, Option<Encoding>)>> + Send + Sync + 'static>>;

// Attempts to open the file with corresponding encoding but
// fallbacks to the uncompressed variant if it can't be found
async fn open_file_with_fallback(
    mut path: PathBuf,
    mut precompressed_encoding: Option<Encoding>,
) -> io::Result<(File, Option<Encoding>)> {
    let file = loop {
        match File::open(&path).await {
            Ok(file) => break file,
            Err(err)
                if err.kind() == io::ErrorKind::NotFound && precompressed_encoding.is_some() =>
            {
                // Remove the extension corresponding to a precompressed file (.gz, .br, .zz)
                // to fallback to the uncompressed version
                path.set_extension(OsStr::new(""));
                // Remove the encoding to make sure the correct content encoding header is set
                precompressed_encoding.take();
                continue;
            }
            Err(err) => return Err(err),
        };
    };
    Ok((file, precompressed_encoding))
}

pin_project! {
    // NOTE: This could potentially be upstreamed to `http-body`.
    /// Adapter that turns an `impl AsyncRead` to an `impl Body`.
    #[derive(Debug)]
    pub struct AsyncReadBody<T> {
        #[pin]
        reader: ReaderStream<T>,
    }
}

impl<T> AsyncReadBody<T>
where
    T: AsyncRead,
{
    /// Create a new [`AsyncReadBody`] wrapping the given reader,
    /// with a specific read buffer capacity
    fn with_capacity(read: T, capacity: usize) -> Self {
        Self {
            reader: ReaderStream::with_capacity(read, capacity),
        }
    }
}

impl<T> Body for AsyncReadBody<T>
where
    T: AsyncRead,
{
    type Data = Bytes;
    type Error = io::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        self.project().reader.poll_next(cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        Poll::Ready(Ok(None))
    }
}

fn response_from_io_error(
    err: io::Error,
) -> Result<Response<BoxBody<Bytes, io::Error>>, io::Error> {
    match err.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
            let res = Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Empty::new().map_err(|err| match err {}).boxed())
                .unwrap();

            Ok(res)
        }
        _ => Err(err),
    }
}
