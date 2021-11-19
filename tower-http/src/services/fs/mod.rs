//! File system related services.

use bytes::Bytes;
use http::{HeaderMap, Response, StatusCode};
use http_body::{combinators::BoxBody, Body, Empty};
use pin_project_lite::pin_project;
use std::{future::Future, path::PathBuf};
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

use crate::content_encoding::{encodings, preferred_encoding, Encoding, SupportedEncodings};

pub use self::{
    serve_dir::{
        ResponseBody as ServeDirResponseBody, ResponseFuture as ServeDirResponseFuture, ServeDir,
    },
    serve_file::{
        ResponseBody as ServeFileResponseBody, ResponseFuture as ServeFileResponseFuture, ServeFile,
    },
};

#[derive(Clone, Debug)]
struct EncodingCandidates {
    precompressed_variants: Option<PrecompressedVariants>,
    acceptted_encodings: Vec<(Encoding, f32)>,
}

impl EncodingCandidates {
    fn new(precompressed_variants: Option<PrecompressedVariants>, headers: &HeaderMap) -> Self {
        let acceptted_encodings = if let Some(precompressed_variants) = precompressed_variants {
            encodings(headers, precompressed_variants)
        } else {
            Vec::new()
        };
        Self {
            precompressed_variants,
            acceptted_encodings,
        }
    }
    fn negotiated_first(&self) -> Option<Encoding> {
        preferred_encoding(self.acceptted_encodings.iter())
    }
    fn negotiated_second(&self) -> Option<Encoding> {
        if let Some(first) = self.negotiated_first() {
            let acceptted_encodings = self.acceptted_encodings.iter().filter(|x| x.0 != first);
            preferred_encoding(acceptted_encodings)
        } else {
            None
        }
    }
    fn negotiated_third(&self) -> Option<Encoding> {
        if let Some(first) = self.negotiated_first() {
            if let Some(second) = self.negotiated_second() {
                let acceptted_encodings = self
                    .acceptted_encodings
                    .iter()
                    .filter(|x| x.0 != first && x.0 != second);
                preferred_encoding(acceptted_encodings)
            } else {
                None
            }
        } else {
            None
        }
    }
    fn negotiated_encodings(&self) -> (Option<Encoding>, Option<Encoding>, Option<Encoding>) {
        (
            self.negotiated_first(),
            self.negotiated_second(),
            self.negotiated_third(),
        )
    }
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

type FileFuture =
    Pin<Box<dyn Future<Output = io::Result<(File, Option<Encoding>)>> + Send + Sync + 'static>>;

// Attempts to open the file with negitiated encoding cascading but
// fallbacks to the uncompressed variant if it can't be found
async fn open_file_with_fallback(
    uncompressed_path: PathBuf,
    negitiated_encodings: (Option<Encoding>, Option<Encoding>, Option<Encoding>),
) -> io::Result<(File, Option<Encoding>)> {
    let mut loop_count = 0;
    let result = loop {
        let mut path = uncompressed_path.clone();
        let negotiated_encoding = if loop_count == 0 {
            negitiated_encodings.0
        } else if loop_count == 1 {
            negitiated_encodings.1
        } else if loop_count == 2 {
            negitiated_encodings.2
        } else {
            None
        };
        if let Some(file_extension) =
            negotiated_encoding.and_then(|encoding| encoding.to_file_extension())
        {
            let new_extension = uncompressed_path
                .extension()
                .map(|extension| {
                    let mut os_string = extension.to_os_string();
                    os_string.push(file_extension);
                    os_string
                })
                .unwrap_or_else(|| file_extension.to_os_string());
            path.set_extension(new_extension);
        };
        match File::open(&path).await {
            Ok(file) => break (file, negotiated_encoding),
            Err(err) if err.kind() == io::ErrorKind::NotFound && negotiated_encoding.is_some() => {
                loop_count += 1;
                continue;
            }
            Err(err) => return Err(err),
        };
    };
    Ok(result)
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
