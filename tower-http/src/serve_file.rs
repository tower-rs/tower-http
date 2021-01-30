//! Service that serves a file.

use bytes::{Bytes, BytesMut};
use futures_util::ready;
use http::{header, HeaderMap, HeaderValue, Response};
use http_body::Body;
use mime::Mime;
use pin_project::pin_project;
use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{fs::File, io::AsyncRead};
use tokio_util::io::poll_read_buf;
use tower_service::Service;

/// Service that serves a file.
#[derive(Clone, Debug)]
pub struct ServeFile {
    path: PathBuf,
    mime: HeaderValue,
}

impl ServeFile {
    /// Create a new [`ServeFile`].
    ///
    /// The `Content-Type` will be guessed from the file extension.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let guess = mime_guess::from_path(&path);
        let mime = guess
            .first()
            .and_then(|mime| HeaderValue::from_str(&mime.to_string()).ok())
            .unwrap_or_else(|| {
                HeaderValue::from_str(&mime::APPLICATION_OCTET_STREAM.to_string()).unwrap()
            });

        let path = path.as_ref().to_owned();

        Self { path, mime }
    }

    /// Create a new [`ServeFile`] with a specific mime type.
    ///
    /// # Panics
    ///
    /// Will panic if the mime type isn't a valid [header value].
    ///
    /// [header value]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html
    pub fn new_with_mime<P: AsRef<Path>>(path: P, mime: Mime) -> Self {
        let mime =
            HeaderValue::from_str(&mime.to_string()).expect("mime isn't a valid header value");
        let path = path.as_ref().to_owned();

        Self { path, mime }
    }
}

impl<R> Service<R> for ServeFile {
    type Response = Response<AsyncReadBody<File>>;
    type Error = io::Error;
    type Future = ResponseFuture;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: R) -> Self::Future {
        let open_file_future = Box::pin(File::open(self.path.clone()));

        ResponseFuture {
            open_file_future,
            mime: Some(self.mime.clone()),
        }
    }
}

/// Response future of [`ServeFile`].
#[pin_project]
pub struct ResponseFuture {
    #[pin]
    open_file_future: Pin<Box<dyn Future<Output = io::Result<File>> + Send + Sync + 'static>>,
    mime: Option<HeaderValue>,
}

impl Future for ResponseFuture {
    type Output = io::Result<Response<AsyncReadBody<File>>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let file = ready!(this.open_file_future.poll(cx)?);
        let body = AsyncReadBody::new(file);

        let mut res = Response::new(body);
        res.headers_mut()
            .insert(header::CONTENT_TYPE, this.mime.take().unwrap());

        Poll::Ready(Ok(res))
    }
}

/// Adapter that turns an `impl AsyncRead` to an `impl Body`.
#[pin_project]
pub struct AsyncReadBody<T> {
    #[pin]
    inner: T,
}

impl<T> AsyncReadBody<T> {
    fn new(inner: T) -> Self {
        Self { inner }
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
