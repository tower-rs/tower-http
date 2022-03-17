use crate::services::{fs::ServeFileSystemResponseFuture, ServeDir, ServeFile};
use bytes::Bytes;
use futures_util::ready;
use http::{HeaderMap, Method, Request, Response, StatusCode, Uri};
use http_body::{combinators::UnsyncBoxBody, Body as _, Empty};
use pin_project_lite::pin_project;
use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tower_service::Service;

/// Service for serving single page apps.
///
/// This wraps a [`ServeDir`] used to serve static files from a directory. If the request path
/// doesn't match a file an index file will be served using [`ServeFile`].
///
/// # Example
///
/// Imagine we have a directory called `dist` that contains `index.html` and `main.js`:
///
/// ```rust
/// use tower_http::fs::Spa;
///
/// let service = Spa::new("dist");
///
/// # async {
/// // Run our service using `hyper`
/// let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 3000));
/// hyper::Server::bind(&addr)
///     .serve(tower::make::Shared::new(service))
///     .await
///     .expect("server error");
/// # };
/// ```
///
/// Requests will be routed like so:
///
/// - `GET` to `/`, `/foo`, `/foo/bar`, or `/index.html` will all send `index.html`.
/// - `GET /main.js` will send `main.js`.
///
/// This is a common routing setup used with single page application.
#[derive(Debug, Clone)]
pub struct Spa {
    serve_dir: ServeDir,
    serve_index: ServeFile,
    path: PathBuf,
}

impl Spa {
    /// Create a new `Spa` serving files from directory at the given path.
    pub fn new<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        Self {
            serve_dir: ServeDir::new(path),
            serve_index: ServeFile::new(Path::new(path).join("index.html")),
            path: path.to_owned(),
        }
    }

    /// Set the path to the index file.
    ///
    /// The path must be relative to the path passed to [`Spa::new`].
    ///
    /// Defaults to `index.html`.
    pub fn index_file(mut self, path: &str) -> Self {
        self.serve_index = ServeFile::new(self.path.join(path));
        self
    }
}

impl<ReqBody> Service<Request<ReqBody>> for Spa {
    type Response = Response<UnsyncBoxBody<Bytes, io::Error>>;
    type Error = io::Error;
    type Future = ResponseFuture;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // `ServeDir` and `ServeFile` are always ready
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();

        ResponseFuture {
            state: State::ServeDir {
                future: self.serve_dir.call(req),
                serve_index: self.serve_index.clone(),
                parts: Some(Parts {
                    method,
                    uri,
                    headers,
                }),
            },
        }
    }
}

pin_project! {
    /// Response future for [`Spa`].
    pub struct ResponseFuture {
        #[pin]
        state: State,
    }
}

pin_project! {
    #[project = StateProj]
    enum State {
        ServeDir {
            #[pin]
            future: ServeFileSystemResponseFuture,
            serve_index: ServeFile,
            parts: Option<Parts>,
        },
        ServeFile {
            #[pin]
            future: ServeFileSystemResponseFuture,
        },
    }
}

struct Parts {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
}

impl Future for ResponseFuture {
    type Output = Result<Response<UnsyncBoxBody<Bytes, io::Error>>, io::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let mut this = self.as_mut().project();

            let new_state = match this.state.as_mut().project() {
                StateProj::ServeDir {
                    future,
                    serve_index,
                    parts,
                } => {
                    let res = ready!(future.poll(cx)?);

                    if res.status() == StatusCode::NOT_FOUND {
                        let Parts {
                            uri,
                            method,
                            headers,
                        } = parts.take().expect("future polled after completion");

                        let mut req = Request::new(Empty::<Bytes>::new());
                        *req.uri_mut() = uri;
                        *req.method_mut() = method;
                        *req.headers_mut() = headers;

                        State::ServeFile {
                            future: serve_index.call(req),
                        }
                    } else {
                        return Poll::Ready(Ok(res.map(|body| body.boxed_unsync())));
                    }
                }
                StateProj::ServeFile { future } => {
                    return future
                        .poll(cx)
                        .map(|result| result.map(|res| res.map(|body| body.boxed_unsync())));
                }
            };

            this.state.set(new_state);
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn basic() {
        todo!("write some tests")
    }
}
