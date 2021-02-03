//! File system related services.

use bytes::{Bytes, BytesMut};
use futures_core::ready;
use http::HeaderMap;
use http_body::Body;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::AsyncRead;
use tokio_util::io::poll_read_buf;

mod serve_file;

pub use self::serve_file::ServeFile;

// NOTE: This could potentially be upstreamed to `http-body`.
/// Adapter that turns an `impl AsyncRead` to an `impl Body`.
pub struct AsyncReadBody<T> {
    inner: T,
}

impl<T> AsyncReadBody<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T> Body for AsyncReadBody<T>
where
    T: AsyncRead + Unpin,
{
    type Data = Bytes;
    type Error = io::Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let mut buf = BytesMut::new();
        let read = ready!(poll_read_buf(Pin::new(&mut self.inner), cx, &mut buf)?);

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
