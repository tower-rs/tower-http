use super::body::create_error_response;
use super::ResponseBody;
use futures_core::ready;
use http::Response;
use http_body::{Body, LengthLimitError};
use pin_project_lite::pin_project;
use std::error::Error as StdError;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pin_project! {
    /// Response future for [`RequestBodyLimit`].
    ///
    /// [`RequestBodyLimit`]: super::RequestBodyLimit
    pub struct ResponseFuture<F> {
        #[pin]
        inner: ResponseFutureInner<F>,
    }
}

impl<F> ResponseFuture<F> {
    pub(crate) fn payload_too_large() -> Self {
        Self {
            inner: ResponseFutureInner::PayloadTooLarge,
        }
    }

    pub(crate) fn new(future: F) -> Self {
        Self {
            inner: ResponseFutureInner::Future { future },
        }
    }
}

pin_project! {
    #[project = ResFutProj]
    enum ResponseFutureInner<F> {
        PayloadTooLarge,
        Future {
            #[pin]
            future: F,
        }
    }
}

impl<ResBody, F, E> Future for ResponseFuture<F>
where
    ResBody: Body,
    F: Future<Output = Result<Response<ResBody>, E>>,
    E: StdError + 'static,
{
    type Output = Result<Response<ResponseBody<ResBody>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let val = match self.project().inner.project() {
            ResFutProj::PayloadTooLarge => Ok(create_error_response()),
            ResFutProj::Future { future } => match ready!(future.poll(cx)) {
                Ok(data) => {
                    let (parts, body) = data.into_parts();
                    let body = ResponseBody::new(body);
                    let resp = Response::from_parts(parts, body);

                    Ok(resp)
                }
                Err(err) => {
                    if is_length_limit_error(&err) {
                        Ok(create_error_response())
                    } else {
                        Err(err)
                    }
                }
            },
        };

        Poll::Ready(val)
    }
}

fn is_length_limit_error(err: &(dyn StdError + 'static)) -> bool {
    let mut source = Some(err);
    while let Some(err) = source {
        if let Some(_) = err.downcast_ref::<LengthLimitError>() {
            return true;
        }
        source = err.source();
    }
    false
}
