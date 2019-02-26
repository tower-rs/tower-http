use Body;
use futures::Poll;
use tokio_buf::{BufStream, SizeHint};

/// Wraps a `Body` instance, implementing `tokio_buf::BufStream`.
///
/// See [`into_buf_stream`] function documentation for more details.
///
/// [`into_buf_stream`]: #
pub struct IntoBufStream<T> {
    inner: T,
}

impl<T> IntoBufStream<T> {
    pub(crate) fn new(inner: T) -> IntoBufStream<T> {
        IntoBufStream { inner }
    }
}

impl<T> BufStream for IntoBufStream<T>
where
    T: Body,
{
    type Item = T::Item;
    type Error = T::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll_buf()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
