use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use async_trait::async_trait;
use bytes::Bytes;
use futures::TryStream;
use http_body::{Body as _, Frame};
use http_body_util::BodyExt;
use pin_project_lite::pin_project;
use sync_wrapper::SyncWrapper;
use tower::BoxError;

type BoxBody = http_body_util::combinators::UnsyncBoxBody<Bytes, BoxError>;

#[derive(Debug)]
pub(crate) struct Body(BoxBody);

impl Body {
    pub(crate) fn new<B>(body: B) -> Self
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        Self(body.map_err(Into::into).boxed_unsync())
    }

    pub(crate) fn empty() -> Self {
        Self::new(http_body_util::Empty::new())
    }

    pub(crate) fn from_stream<S>(stream: S) -> Self
    where
        S: TryStream + Send + 'static,
        S::Ok: Into<Bytes>,
        S::Error: Into<BoxError>,
    {
        Self::new(StreamBody {
            stream: SyncWrapper::new(stream),
        })
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::empty()
    }
}

macro_rules! body_from_impl {
    ($ty:ty) => {
        impl From<$ty> for Body {
            fn from(buf: $ty) -> Self {
                Self::new(http_body_util::Full::from(buf))
            }
        }
    };
}

body_from_impl!(&'static [u8]);
body_from_impl!(std::borrow::Cow<'static, [u8]>);
body_from_impl!(Vec<u8>);

body_from_impl!(&'static str);
body_from_impl!(std::borrow::Cow<'static, str>);
body_from_impl!(String);

body_from_impl!(Bytes);

impl http_body::Body for Body {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.0).poll_frame(cx)
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.0.size_hint()
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }
}

pin_project! {
    struct StreamBody<S> {
        #[pin]
        stream: SyncWrapper<S>,
    }
}

impl<S> http_body::Body for StreamBody<S>
where
    S: TryStream,
    S::Ok: Into<Bytes>,
    S::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let stream = self.project().stream.get_pin_mut();
        match futures_util::ready!(stream.try_poll_next(cx)) {
            Some(Ok(chunk)) => Poll::Ready(Some(Ok(Frame::data(chunk.into())))),
            Some(Err(err)) => Poll::Ready(Some(Err(err.into()))),
            None => Poll::Ready(None),
        }
    }
}

// copied from hyper
pub(crate) async fn to_bytes<T>(body: T) -> Result<Bytes, T::Error>
where
    T: http_body::Body,
{
    futures_util::pin_mut!(body);
    Ok(body.collect().await?.to_bytes())
}

pub(crate) trait TowerHttpBodyExt: http_body::Body + Unpin {
    /// Returns future that resolves to next data chunk, if any.
    fn data(&mut self) -> Data<'_, Self>
    where
        Self: Unpin + Sized,
    {
        Data(self)
    }
}

impl<B> TowerHttpBodyExt for B where B: http_body::Body + Unpin {}

pub(crate) struct Data<'a, T>(pub(crate) &'a mut T);

impl<'a, T> Future for Data<'a, T>
where
    T: http_body::Body + Unpin,
{
    type Output = Option<Result<T::Data, T::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match futures_util::ready!(Pin::new(&mut self.0).poll_frame(cx)) {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => return Poll::Ready(Some(Ok(data))),
                    Err(_frame) => {}
                },
                Some(Err(err)) => return Poll::Ready(Some(Err(err))),
                None => return Poll::Ready(None),
            }
        }
    }
}
