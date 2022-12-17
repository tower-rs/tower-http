use super::{
    open_file::{FileOpened, FileRequestExtent, OpenFileOutput},
    ResponseBody,
};
use crate::{
    services::fs::{AsyncReadBody, Backend, Metadata as _},
    BoxError,
};
use bytes::Bytes;
use futures_util::{
    future::{BoxFuture, FutureExt, TryFutureExt},
    ready,
};
use http::{
    header::{self, ALLOW},
    HeaderValue, Request, Response, StatusCode,
};
use http_body::{Body, Empty, Full};
use pin_project_lite::pin_project;
use std::{
    convert::Infallible,
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tower_service::Service;

pin_project! {
    /// Response future of [`ServeDir::try_call`].
    pub struct ResponseFuture<ReqBody, F, B: Backend> {
        #[pin]
        pub(super) inner: ResponseFutureInner<ReqBody, F, B>,
    }
}

impl<ReqBody, F, B: Backend> ResponseFuture<ReqBody, F, B> {
    pub(super) fn open_file_future(
        future: BoxFuture<'static, io::Result<OpenFileOutput<B>>>,
        fallback_and_request: Option<(F, Request<ReqBody>)>,
    ) -> Self {
        Self {
            inner: ResponseFutureInner::OpenFileFuture {
                future,
                fallback_and_request,
            },
        }
    }

    pub(super) fn invalid_path(fallback_and_request: Option<(F, Request<ReqBody>)>) -> Self {
        Self {
            inner: ResponseFutureInner::InvalidPath {
                fallback_and_request,
            },
        }
    }

    pub(super) fn method_not_allowed() -> Self {
        Self {
            inner: ResponseFutureInner::MethodNotAllowed,
        }
    }
}

pin_project! {
    #[project = ResponseFutureInnerProj]
    pub(super) enum ResponseFutureInner<ReqBody, F, B: Backend> {
        OpenFileFuture {
            #[pin]
            future: BoxFuture<'static, io::Result<OpenFileOutput<B>>>,
            fallback_and_request: Option<(F, Request<ReqBody>)>,
        },
        FallbackFuture {
            future: BoxFuture<'static, Result<Response<ResponseBody>, Infallible>>,
        },
        InvalidPath {
            fallback_and_request: Option<(F, Request<ReqBody>)>,
        },
        MethodNotAllowed,
    }
}

impl<F, B, ReqBody, ResBody> Future for ResponseFuture<ReqBody, F, B>
where
    F: Service<Request<ReqBody>, Response = Response<ResBody>, Error = Infallible> + Clone,
    F::Future: Send + 'static,
    ResBody: http_body::Body<Data = Bytes> + Send + 'static,
    ResBody::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    B: Backend,
{
    type Output = io::Result<Response<ResponseBody>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let mut this = self.as_mut().project();

            let new_state = match this.inner.as_mut().project() {
                ResponseFutureInnerProj::OpenFileFuture {
                    future: open_file_future,
                    fallback_and_request,
                } => match ready!(open_file_future.poll(cx)) {
                    Ok(OpenFileOutput::FileOpened(file_output)) => {
                        break Poll::Ready(Ok(build_response(*file_output)));
                    }

                    Ok(OpenFileOutput::Redirect { location }) => {
                        let mut res = response_with_status(StatusCode::TEMPORARY_REDIRECT);
                        res.headers_mut().insert(http::header::LOCATION, location);
                        break Poll::Ready(Ok(res));
                    }

                    Ok(OpenFileOutput::FileNotFound) => {
                        if let Some((mut fallback, request)) = fallback_and_request.take() {
                            call_fallback(&mut fallback, request)
                        } else {
                            break Poll::Ready(Ok(not_found()));
                        }
                    }

                    Ok(OpenFileOutput::PreconditionFailed) => {
                        break Poll::Ready(Ok(response_with_status(
                            StatusCode::PRECONDITION_FAILED,
                        )));
                    }

                    Ok(OpenFileOutput::NotModified) => {
                        break Poll::Ready(Ok(response_with_status(StatusCode::NOT_MODIFIED)));
                    }

                    Err(err) => {
                        if let io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied =
                            err.kind()
                        {
                            if let Some((mut fallback, request)) = fallback_and_request.take() {
                                call_fallback(&mut fallback, request)
                            } else {
                                break Poll::Ready(Ok(not_found()));
                            }
                        } else {
                            break Poll::Ready(Err(err));
                        }
                    }
                },

                ResponseFutureInnerProj::FallbackFuture { future } => {
                    break Pin::new(future).poll(cx).map_err(|err| match err {})
                }

                ResponseFutureInnerProj::InvalidPath {
                    fallback_and_request,
                } => {
                    if let Some((mut fallback, request)) = fallback_and_request.take() {
                        call_fallback(&mut fallback, request)
                    } else {
                        break Poll::Ready(Ok(not_found()));
                    }
                }

                ResponseFutureInnerProj::MethodNotAllowed => {
                    let mut res = response_with_status(StatusCode::METHOD_NOT_ALLOWED);
                    res.headers_mut()
                        .insert(ALLOW, HeaderValue::from_static("GET,HEAD"));
                    break Poll::Ready(Ok(res));
                }
            };

            this.inner.set(new_state);
        }
    }
}

fn response_with_status(status: StatusCode) -> Response<ResponseBody> {
    Response::builder()
        .status(status)
        .body(empty_body())
        .unwrap()
}

fn not_found() -> Response<ResponseBody> {
    response_with_status(StatusCode::NOT_FOUND)
}

pub(super) fn call_fallback<F, ReqBody, FResBody, B>(
    fallback: &mut F,
    req: Request<ReqBody>,
) -> ResponseFutureInner<ReqBody, F, B>
where
    F: Service<Request<ReqBody>, Response = Response<FResBody>, Error = Infallible> + Clone,
    F::Future: Send + 'static,
    FResBody: http_body::Body<Data = Bytes> + Send + 'static,
    FResBody::Error: Into<BoxError>,
    B: Backend,
{
    let future = fallback
        .call(req)
        .map_ok(|response| {
            response
                .map(|body| {
                    body.map_err(|err| match err.into().downcast::<io::Error>() {
                        Ok(err) => *err,
                        Err(err) => io::Error::new(io::ErrorKind::Other, err),
                    })
                    .boxed_unsync()
                })
                .map(ResponseBody::new)
        })
        .boxed();

    ResponseFutureInner::FallbackFuture { future }
}

fn build_response<B>(output: FileOpened<B>) -> Response<ResponseBody>
where
    B: Backend,
{
    let (maybe_file, size) = match output.extent {
        FileRequestExtent::Full(file, meta) => (Some(file), meta.len()),
        FileRequestExtent::Head(meta) => (None, meta.len()),
    };

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, output.mime_header_value)
        .header(header::ACCEPT_RANGES, "bytes");

    if let Some(encoding) = output.maybe_encoding {
        builder = builder.header(header::CONTENT_ENCODING, encoding.into_header_value());
    }

    if let Some(last_modified) = output.last_modified {
        builder = builder.header(header::LAST_MODIFIED, last_modified.0.to_string());
    }

    match output.maybe_range {
        Some(Ok(ranges)) => {
            if let Some(range) = ranges.first() {
                if ranges.len() > 1 {
                    builder
                        .header(header::CONTENT_RANGE, format!("bytes */{}", size))
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .body(body_from_bytes(Bytes::from(
                            "Cannot serve multipart range requests",
                        )))
                        .unwrap()
                } else {
                    let body = if let Some(file) = maybe_file {
                        let range_size = range.end() - range.start() + 1;
                        ResponseBody::new(
                            AsyncReadBody::with_capacity_limited(
                                file,
                                output.chunk_size,
                                range_size,
                            )
                            .boxed_unsync(),
                        )
                    } else {
                        empty_body()
                    };

                    builder
                        .header(
                            header::CONTENT_RANGE,
                            format!("bytes {}-{}/{}", range.start(), range.end(), size),
                        )
                        .header(header::CONTENT_LENGTH, range.end() - range.start() + 1)
                        .status(StatusCode::PARTIAL_CONTENT)
                        .body(body)
                        .unwrap()
                }
            } else {
                builder
                    .header(header::CONTENT_RANGE, format!("bytes */{}", size))
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .body(body_from_bytes(Bytes::from(
                        "No range found after parsing range header, please file an issue",
                    )))
                    .unwrap()
            }
        }

        Some(Err(_)) => builder
            .header(header::CONTENT_RANGE, format!("bytes */{}", size))
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .body(empty_body())
            .unwrap(),

        // Not a range request
        None => {
            let body = if let Some(file) = maybe_file {
                ResponseBody::new(
                    AsyncReadBody::with_capacity(file, output.chunk_size).boxed_unsync(),
                )
            } else {
                empty_body()
            };

            builder
                .header(header::CONTENT_LENGTH, size.to_string())
                .body(body)
                .unwrap()
        }
    }
}

fn body_from_bytes(bytes: Bytes) -> ResponseBody {
    let body = Full::from(bytes).map_err(|err| match err {}).boxed_unsync();
    ResponseBody::new(body)
}

fn empty_body() -> ResponseBody {
    let body = Empty::new().map_err(|err| match err {}).boxed_unsync();
    ResponseBody::new(body)
}
