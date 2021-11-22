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
use tokio::io::{AsyncRead, AsyncReadExt, Take};
use tokio_util::io::ReaderStream;

use futures_util::Stream;

mod parse_range;
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

// Returns the preferred_encoding encoding and modifies the path extension
// to the corresponding file extension for the encoding.
fn preferred_encoding(
    path: &mut PathBuf,
    negotiated_encoding: &[(Encoding, f32)],
) -> Option<Encoding> {
    let preferred_encoding = Encoding::preferred_encoding(negotiated_encoding);
    if let Some(file_extension) =
        preferred_encoding.and_then(|encoding| encoding.to_file_extension())
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
    preferred_encoding
}

// Attempts to open the file with any of the possible negotiated_encodings in the
// preferred order. If none of the negotiated_encodings have a corresponding precompressed
// file the uncompressed file is used as a fallback.
async fn open_file_with_fallback(
    mut path: PathBuf,
    mut negotiated_encoding: Vec<(Encoding, f32)>,
) -> io::Result<(File, Option<Encoding>)> {
    let (file, encoding) = loop {
        // Get the preferred encoding among the negotiated ones.
        let encoding = preferred_encoding(&mut path, &negotiated_encoding);
        match (File::open(&path).await, encoding) {
            (Ok(file), maybe_encoding) => break (file, maybe_encoding),
            (Err(err), Some(encoding)) if err.kind() == io::ErrorKind::NotFound => {
                // Remove the extension corresponding to a precompressed file (.gz, .br, .zz)
                // to reset the path before the next iteration.
                path.set_extension(OsStr::new(""));
                // Remove the encoding from the negotiated_encodings since the file doesn't exist
                negotiated_encoding
                    .retain(|(negotiated_encoding, _)| *negotiated_encoding != encoding);
                continue;
            }
            (Err(err), _) => return Err(err),
        };
    };
    Ok((file, encoding))
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

    fn with_capacity_limited(
        read: T,
        capacity: usize,
        max_read_bytes: u64,
    ) -> AsyncReadBody<Take<T>> {
        AsyncReadBody {
            reader: ReaderStream::with_capacity(read.take(max_read_bytes as u64), capacity),
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
