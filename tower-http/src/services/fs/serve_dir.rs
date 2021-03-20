use super::AsyncReadBody;
use bytes::Bytes;
use futures_util::ready;
use http::{header, HeaderValue, Request, Response, StatusCode};
use http_body::{combinators::BoxBody, Body, Empty};
use std::{
    convert::Infallible,
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::fs::File;
use tower_service::Service;

/// Service that serves files from a given directory and all its sub directories.
///
/// The `Content-Type` will be guessed from the file extension.
///
/// An empty response with status `404 Not Found` will be returned if:
///
/// - The file doesn't exist
/// - Any segment of the path contains `..`
/// - Any segment of the path contains a backslash
/// - We don't have necessary permissions to read the file
#[derive(Clone, Debug)]
pub struct ServeDir {
    base: PathBuf,
    append_index_html_on_directories: bool,
}

impl ServeDir {
    /// Create a new [`ServeDir`].
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let mut base = PathBuf::from(".");
        base.push(path.as_ref());

        Self {
            base,
            append_index_html_on_directories: true,
        }
    }

    /// If the requested path is a directory append `index.html`.
    ///
    /// This is useful for static sites.
    ///
    /// Defaults to `true`.
    pub fn append_index_html_on_directories(mut self, append: bool) -> Self {
        self.append_index_html_on_directories = append;
        self
    }
}

impl<ReqBody> Service<Request<ReqBody>> for ServeDir {
    type Response = Response<BoxBody<Bytes, io::Error>>;
    type Error = io::Error;
    type Future = ResponseFuture;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        // build and validate the path
        let path = req.uri().path().to_string();
        let path = path.trim_start_matches('/');
        let mut full_path = self.base.clone();
        let mut valid = true;
        for seg in path.split('/') {
            if seg.starts_with("..") || seg.contains('\\') {
                valid = false;
                break;
            } else {
                full_path.push(seg);
            }
        }

        let inner = if valid {
            let append_index_html_on_directories = self.append_index_html_on_directories;
            let open_file_future = Box::pin(async move {
                if append_index_html_on_directories {
                    let is_dir = tokio::fs::metadata(full_path.clone())
                        .await
                        .map(|m| m.is_dir())
                        .unwrap_or(false);

                    if is_dir {
                        full_path.push("index.html");
                    }
                }

                let guess = mime_guess::from_path(&full_path);
                let mime = guess
                    .first()
                    .and_then(|mime| HeaderValue::from_str(&mime.to_string()).ok())
                    .unwrap_or_else(|| {
                        HeaderValue::from_str(&mime::APPLICATION_OCTET_STREAM.to_string()).unwrap()
                    });

                let file = File::open(full_path).await?;
                Ok((file, mime))
            });

            Inner::Valid(open_file_future)
        } else {
            Inner::Invalid
        };

        ResponseFuture { inner }
    }
}

enum Inner {
    Valid(Pin<Box<dyn Future<Output = io::Result<(File, HeaderValue)>> + Send + Sync + 'static>>),
    Invalid,
}

/// Response future of [`ServeDir`].
pub struct ResponseFuture {
    inner: Inner,
}

impl Future for ResponseFuture {
    type Output = io::Result<Response<BoxBody<Bytes, io::Error>>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.inner {
            Inner::Valid(open_file_future) => {
                let (file, mime) = match ready!(Pin::new(open_file_future).poll(cx)) {
                    Ok(inner) => inner,
                    Err(err) => match err.kind() {
                        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                            let res = Response::builder()
                                .status(StatusCode::NOT_FOUND)
                                .body(
                                    Empty::new()
                                        .map_err(|_err: Infallible| unreachable!())
                                        .boxed(),
                                )
                                .unwrap();

                            return Poll::Ready(Ok(res));
                        }
                        _ => return Poll::Ready(Err(err)),
                    },
                };
                let body = AsyncReadBody::new(file).boxed();

                let mut res = Response::new(body);
                res.headers_mut().insert(header::CONTENT_TYPE, mime);

                Poll::Ready(Ok(res))
            }
            Inner::Invalid => {
                let res = Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(
                        Empty::new()
                            .map_err(|_err: Infallible| unreachable!())
                            .boxed(),
                    )
                    .unwrap();

                Poll::Ready(Ok(res))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use http::{Request, StatusCode};
    use http_body::Body as HttpBody;
    use hyper::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn basic() {
        let svc = ServeDir::new("..");

        let req = Request::builder()
            .uri("/README.md")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/markdown");

        let body = body_into_text(res.into_body()).await;

        let contents = std::fs::read_to_string("../README.md").unwrap();
        assert_eq!(body, contents);
    }

    #[tokio::test]
    async fn access_to_sub_dirs() {
        let svc = ServeDir::new("..");

        let req = Request::builder()
            .uri("/tower-http/Cargo.toml")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/x-toml");

        let body = body_into_text(res.into_body()).await;

        let contents = std::fs::read_to_string("Cargo.toml").unwrap();
        assert_eq!(body, contents);
    }

    #[tokio::test]
    async fn not_found() {
        let svc = ServeDir::new("..");

        let req = Request::builder()
            .uri("/not-found")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);

        let body = body_into_text(res.into_body()).await;
        assert!(body.is_empty());
    }

    async fn body_into_text<B>(mut body: B) -> String
    where
        B: HttpBody<Data = bytes::Bytes> + Unpin,
        B::Error: std::fmt::Debug,
    {
        let mut buf = String::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.unwrap();
            buf.push_str(&String::from_utf8(chunk.to_vec()).unwrap());
        }
        buf
    }
}
