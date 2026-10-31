//! File system related services.

use bytes::Bytes;
use futures_core::Stream;
use http_body::{Body, Frame};
use pin_project_lite::pin_project;
use std::{
    io,
    pin::Pin,
    task::{ready, Context, Poll},
};
use tokio::io::{AsyncRead, AsyncReadExt, Take};
use tokio_util::io::ReaderStream;

mod serve_dir;
mod serve_file;

pub use self::{
    serve_dir::{
        future::ResponseFuture as ServeFileSystemResponseFuture,
        Backend,
        DefaultServeDirFallback,
        File,
        Metadata,
        // The response body and future are used for both ServeDir and ServeFile
        ResponseBody as ServeFileSystemResponseBody,
        ServeDir,
        TokioBackend,
        TokioFile,
    },
    serve_file::ServeFile,
};

pin_project! {
    // NOTE: This could potentially be upstreamed to `http-body`.
    /// Adapter that turns an [`impl AsyncRead`][tokio::io::AsyncRead] to an [`impl Body`][http_body::Body].
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
    pub fn with_capacity(read: T, capacity: usize) -> Self {
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

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match std::task::ready!(self.project().reader.poll_next(cx)) {
            Some(Ok(chunk)) => Poll::Ready(Some(Ok(Frame::data(chunk)))),
            Some(Err(err)) => Poll::Ready(Some(Err(err))),
            None => Poll::Ready(None),
        }
    }
}

type BoxedFile = Box<dyn AsyncRead + Unpin + Send>;

/// Body of a file response whose first chunk was read before the response was returned.
///
/// Knowing the length means no read is needed to find the end of the file, and that the body can
/// tell hyper it has ended along with the last chunk.
struct FileBody {
    first_chunk: Option<Bytes>,
    rest: Option<ReaderStream<Take<BoxedFile>>>,
    rest_len: u64,
}

impl FileBody {
    fn new(file: BoxedFile, first_chunk: Bytes, len: u64, chunk_size: usize) -> Self {
        // Nothing read means the file is shorter than its metadata said.
        let rest_len = match first_chunk.len() as u64 {
            0 => 0,
            read => len.saturating_sub(read),
        };
        Self {
            first_chunk: Some(first_chunk).filter(|chunk| !chunk.is_empty()),
            rest: (rest_len > 0).then(|| {
                let capacity = rest_len.min(chunk_size as u64) as usize;
                ReaderStream::with_capacity(file.take(rest_len), capacity)
            }),
            rest_len,
        }
    }
}

impl Body for FileBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();

        if let Some(chunk) = this.first_chunk.take() {
            return Poll::Ready(Some(Ok(Frame::data(chunk))));
        }
        let Some(rest) = &mut this.rest else {
            return Poll::Ready(None);
        };

        let item = ready!(Pin::new(rest).poll_next(cx));
        this.rest_len = match &item {
            Some(Ok(chunk)) => this.rest_len.saturating_sub(chunk.len() as u64),
            Some(Err(_)) | None => 0,
        };
        if this.rest_len == 0 {
            this.rest = None;
        }
        Poll::Ready(item.map(|res| res.map(Frame::data)))
    }

    fn is_end_stream(&self) -> bool {
        self.first_chunk.is_none() && self.rest.is_none()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        let first = self
            .first_chunk
            .as_ref()
            .map_or(0, |chunk| chunk.len() as u64);
        http_body::SizeHint::with_exact(first + self.rest_len)
    }
}
