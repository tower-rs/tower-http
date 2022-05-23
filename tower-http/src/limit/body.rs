use bytes::Buf;
use futures_core::ready;
use http::{HeaderMap, HeaderValue, Response, StatusCode};
use http_body::{Body, SizeHint};
use pin_project_lite::pin_project;
use std::convert::TryFrom;
use std::pin::Pin;
use std::task::{Context, Poll};

pin_project! {
    /// Response body for [`RequestBodyLimit`].
    ///
    /// [`RequestBodyLimit`]: super::RequestBodyLimit
    pub struct ResponseBody<B>
    where
        B: Body,
    {
        #[pin]
        inner: ResponseBodyInner<B>
    }
}

impl<B> ResponseBody<B>
where
    B: Body,
{
    fn payload_too_large() -> Self {
        Self {
            inner: ResponseBodyInner::PayloadTooLarge {
                data: Some(ResponseData::payload_too_large()),
            },
        }
    }

    pub(crate) fn new(body: B) -> Self {
        Self {
            inner: ResponseBodyInner::Body { body },
        }
    }
}

pin_project! {
    #[project = BodyProj]
    enum ResponseBodyInner<B>
    where
        B: Body,
    {
        PayloadTooLarge {
            data: Option<ResponseData<B>>,
        },
        Body {
            #[pin]
            body: B
        }
    }
}

impl<B> Body for ResponseBody<B>
where
    B: Body,
{
    type Data = ResponseData<B>;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.project().inner.project() {
            BodyProj::PayloadTooLarge { data } => Poll::Ready(Ok(data.take()).transpose()),
            BodyProj::Body { body } => {
                let or_data = ready!(body.poll_data(cx));
                Poll::Ready(or_data.map(|r_data| r_data.map(|data| ResponseData::new(data))))
            }
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        match self.project().inner.project() {
            BodyProj::PayloadTooLarge { .. } => Poll::Ready(Ok(None)),
            BodyProj::Body { body } => body.poll_trailers(cx),
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self.inner {
            ResponseBodyInner::PayloadTooLarge { data } => data.is_none(),
            ResponseBodyInner::Body { body } => body.is_end_stream(),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.inner {
            ResponseBodyInner::PayloadTooLarge { data } => data
                .as_ref()
                .map(|data| {
                    // The static payload will always be small.
                    let rem = u64::try_from(data.remaining()).unwrap();
                    SizeHint::with_exact(rem)
                })
                .unwrap_or_else(|| SizeHint::with_exact(0)),
            ResponseBodyInner::Body { body } => body.size_hint(),
        }
    }
}

/// Response data for [`RequestBodyLimit`].
///
/// [`RequestBodyLimit`]: super::RequestBodyLimit
pub struct ResponseData<B>
where
    B: Body,
{
    inner: ResponseDataInner<B>,
}

enum ResponseDataInner<B>
where
    B: Body,
{
    PayloadTooLarge { sent: &'static [u8] },
    Data { data: B::Data },
}

impl<B> ResponseData<B>
where
    B: Body,
{
    fn payload_too_large() -> Self {
        Self {
            inner: ResponseDataInner::PayloadTooLarge { sent: BODY },
        }
    }

    fn new(data: B::Data) -> Self {
        Self {
            inner: ResponseDataInner::Data { data },
        }
    }
}

impl<B> Buf for ResponseData<B>
where
    B: Body,
{
    fn remaining(&self) -> usize {
        match &self.inner {
            ResponseDataInner::PayloadTooLarge { sent } => sent.remaining(),
            ResponseDataInner::Data { data } => data.remaining(),
        }
    }

    fn chunk(&self) -> &[u8] {
        match &self.inner {
            ResponseDataInner::PayloadTooLarge { sent } => sent.chunk(),
            ResponseDataInner::Data { data } => data.chunk(),
        }
    }

    fn advance(&mut self, cnt: usize) {
        match &mut self.inner {
            ResponseDataInner::PayloadTooLarge { sent } => sent.advance(cnt),
            ResponseDataInner::Data { data } => data.advance(cnt),
        }
    }
}

const BODY: &[u8] = b"length limit exceeded";

pub(super) fn create_error_response<B>() -> Response<ResponseBody<B>>
where
    B: Body,
{
    let mut res = Response::new(ResponseBody::payload_too_large());
    *res.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;

    #[allow(clippy::declare_interior_mutable_const)]
    const TEXT_PLAIN: HeaderValue = HeaderValue::from_static("text/plain; charset=utf-8");
    const CLOSE: HeaderValue = HeaderValue::from_static("close");
    res.headers_mut()
        .insert(http::header::CONTENT_TYPE, TEXT_PLAIN);
    res.headers_mut().insert(http::header::CONNECTION, CLOSE);

    res
}
