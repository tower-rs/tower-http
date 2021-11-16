/*
Copyright (c) 2021 Tokio Contributors

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
 */
use bytes::{Bytes, BytesMut};
use futures_core::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use pin_project_lite::pin_project;
use tokio::io::AsyncRead;
use tokio_util::io::poll_read_buf;

const DEFAULT_CAPACITY: usize = 4096;

pin_project! {
    #[derive(Debug)]
    pub(crate) struct ReaderStream<R> {
        // Reader itself.
        //
        // This value is `None` if the stream has terminated.
        #[pin]
        reader: Option<R>,
        // Working buffer, used to optimize allocations.
        buf: BytesMut,
        capacity: usize,
        max_read_bytes: Option<usize>,
        read_bytes: usize,
    }
}

impl<R: AsyncRead> ReaderStream<R> {
    /// Convert an [`AsyncRead`] into a [`Stream`] with item type
    /// `Result<Bytes, std::io::Error>`.
    ///
    /// [`AsyncRead`]: tokio::io::AsyncRead
    /// [`Stream`]: futures_core::Stream
    pub(crate) fn new(reader: R) -> Self {
        ReaderStream {
            reader: Some(reader),
            buf: BytesMut::new(),
            capacity: DEFAULT_CAPACITY,
            max_read_bytes: None,
            read_bytes: 0,
        }
    }

    /// Convert an [`AsyncRead`] into a [`Stream`] with item type
    /// `Result<Bytes, std::io::Error>`,
    /// with a specific read buffer initial capacity.
    ///
    /// [`AsyncRead`]: tokio::io::AsyncRead
    /// [`Stream`]: futures_core::Stream
    pub(crate) fn with_capacity(reader: R, capacity: usize) -> Self {
        ReaderStream {
            reader: Some(reader),
            buf: BytesMut::with_capacity(capacity),
            capacity,
            max_read_bytes: None,
            read_bytes: 0,
        }
    }

    pub(crate) fn with_capacity_limited(reader: R, capacity: usize, max_read_bytes: usize) -> Self {
        ReaderStream {
            reader: Some(reader),
            buf: BytesMut::with_capacity(capacity),
            capacity,
            max_read_bytes: Some(max_read_bytes),
            read_bytes: 0
        }
    }
}

impl<R: AsyncRead> Stream for ReaderStream<R> {
    type Item = std::io::Result<Bytes>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();

        let reader = match this.reader.as_pin_mut() {
            Some(r) => r,
            None => return Poll::Ready(None),
        };
        if this.buf.capacity() == 0 {
            this.buf.reserve(*this.capacity);
        }

        match poll_read_buf(reader, cx, &mut this.buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                self.project().reader.set(None);
                Poll::Ready(Some(Err(err)))
            }
            Poll::Ready(Ok(0)) => {
                self.project().reader.set(None);
                Poll::Ready(None)
            }
            Poll::Ready(Ok(_)) => {
                if let Some(max_read) = this.max_read_bytes {
                    let bytes_left = *max_read - *this.read_bytes;
                    if this.buf.capacity() > bytes_left {
                        this.read_bytes = max_read;
                        let chunk = this.buf.split_to(bytes_left);
                        self.project().reader.set(None);
                        return Poll::Ready(Some(Ok(chunk.freeze())))
                    }
                }
                let chunk = this.buf.split();
                *this.read_bytes += chunk.len();
                Poll::Ready(Some(Ok(chunk.freeze())))
            }
        }
    }
}
