//! File system related services.

use bytes::{Bytes, BytesMut};
use futures_core::ready;
use http::{HeaderMap, Response, StatusCode};
use http_body::{combinators::BoxBody, Body, Empty};
use pin_project::pin_project;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::AsyncRead;
use tokio_util::io::poll_read_buf;

mod serve_dir;
mod serve_file;

pub use self::{
    serve_dir::{
        ResponseBody as ServeDirResponseBody, ResponseFuture as ServeDirResponseFuture, ServeDir,
    },
    serve_file::{
        ResponseBody as ServeFileResponseBody, ResponseFuture as ServeFileResponseFuture, ServeFile,
    },
};

// NOTE: This could potentially be upstreamed to `http-body`.
/// Adapter that turns an `impl AsyncRead` to an `impl Body`.
#[pin_project]
pub struct AsyncReadBody<T> {
    #[pin]
    inner: T,
}

impl<T> AsyncReadBody<T> {
    /// Create a new [`AsyncReadBody`] wrapping the given reader.
    fn new(read: T) -> Self {
        Self { inner: read }
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
        let mut buf = BytesMut::new();
        let read = ready!(poll_read_buf(self.project().inner, cx, &mut buf)?);

        if read == 0 {
            Poll::Ready(None)
        } else {
            Poll::Ready(Some(Ok(buf.freeze())))
        }
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
