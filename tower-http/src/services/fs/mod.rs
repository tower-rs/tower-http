//! File system related services.

use bytes::Bytes;
use http::{HeaderMap, Response, StatusCode};
use http_body::{combinators::BoxBody, Body, Empty};
use pin_project_lite::pin_project;
use std::{
    io,
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll},
};
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

fn check_precompressed_file(
    precompressed_variants: PrecompressedVariants,
    uncompressed_file_path: &PathBuf,
) -> PrecompressedVariants {
    fn check_file_exists(uncompressed_file_path: &PathBuf, encoding: Encoding) -> bool {
        let mut file_path: PathBuf = PathBuf::from(uncompressed_file_path);
        let compressed_ext = encoding.to_file_extension().unwrap();
        let mut ext = file_path.extension().unwrap_or_default().to_owned();
        let ext = if ext.is_empty() {
            compressed_ext.to_owned()
        } else {
            ext.push(compressed_ext);
            ext
        };
        file_path.set_extension(ext);
        file_path.exists()
    }
    let gzip = if precompressed_variants.gzip {
        check_file_exists(uncompressed_file_path, Encoding::Gzip)
    } else {
        false
    };
    let deflate = if precompressed_variants.deflate {
        check_file_exists(uncompressed_file_path, Encoding::Deflate)
    } else {
        false
    };
    let br = if precompressed_variants.br {
        check_file_exists(uncompressed_file_path, Encoding::Brotli)
    } else {
        false
    };
    PrecompressedVariants { gzip, deflate, br }
}

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
