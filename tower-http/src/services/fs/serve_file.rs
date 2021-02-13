//! Service that serves a file.

use super::AsyncReadBody;
use futures_util::ready;
use http::{header, HeaderValue, Response};
use mime::Mime;
use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::fs::File;
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
pub struct ResponseFuture {
    open_file_future: Pin<Box<dyn Future<Output = io::Result<File>> + Send + Sync + 'static>>,
    mime: Option<HeaderValue>,
}

impl Future for ResponseFuture {
    type Output = io::Result<Response<AsyncReadBody<File>>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let file = ready!(Pin::new(&mut self.open_file_future).poll(cx)?);
        let body = AsyncReadBody::new(file);

        let mut res = Response::new(body);
        res.headers_mut()
            .insert(header::CONTENT_TYPE, self.mime.take().unwrap());

        Poll::Ready(Ok(res))
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use http::Request;
    use http_body::Body as _;
    use hyper::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn basic() {
        let svc = ServeFile::new("../README.md");

        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = res.into_body().data().await.unwrap().unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert!(body.starts_with("# Tower HTTP"));
    }
}
