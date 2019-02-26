use bytes::Buf;
use futures::{Async, Poll};
use http::HeaderMap;
use tokio_buf::{SizeHint, BufStream};

pub trait Body {
    /// Values yielded by the `Body`.
    type Item: Buf;

    /// The error type this `BufStream` might generate.
    type Error;

    /// Attempt to pull out the next buffer of this stream, registering the
    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

    /// Returns the bounds on the remaining length of the stream.
    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }

    /// Poll for an optional **single** `HeaderMap` of trailers.
    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::Error>;
}

impl<T: BufStream> Body for T {
    type Item = T::Item;
    type Error = T::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        BufStream::poll_buf(self)
    }

    fn size_hint(&self) -> SizeHint {
        BufStream::size_hint(self)
    }

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::Error> {
        Ok(Async::Ready(None))
    }
}
