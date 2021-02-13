//! Various utilities for building HTTP services.

use bytes::{Buf, Bytes};
use http_body::Body;
use std::{
    convert::Infallible,
    error::Error as StdError,
    fmt,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

/// Combine two types into one.
#[derive(Debug)]
pub enum Either<A, B> {
    #[allow(missing_docs)]
    Left(A),
    #[allow(missing_docs)]
    Right(B),
}

impl<A, B> Body for Either<A, B>
where
    A: Body + Unpin,
    B: Body<Data = A::Data> + Unpin,
{
    type Data = A::Data;
    type Error = Either<A::Error, B::Error>;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.get_mut() {
            Either::Left(inner) => Pin::new(inner)
                .poll_data(cx)
                .map(|opt| opt.map(|res| res.map_err(Either::Left))),
            Either::Right(inner) => Pin::new(inner)
                .poll_data(cx)
                .map(|opt| opt.map(|res| res.map_err(Either::Right))),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        match self.get_mut() {
            Either::Left(inner) => Pin::new(inner).poll_trailers(cx).map_err(Either::Left),
            Either::Right(inner) => Pin::new(inner).poll_trailers(cx).map_err(Either::Right),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            Either::Left(inner) => inner.is_end_stream(),
            Either::Right(inner) => inner.is_end_stream(),
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        match self {
            Either::Left(inner) => inner.size_hint(),
            Either::Right(inner) => inner.size_hint(),
        }
    }
}

impl<A, B> fmt::Display for Either<A, B>
where
    A: fmt::Display,
    B: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Either::Left(inner) => inner.fmt(f),
            Either::Right(inner) => inner.fmt(f),
        }
    }
}

impl<A, B> StdError for Either<A, B>
where
    A: StdError,
    B: StdError,
{
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Either::Left(inner) => inner.source(),
            Either::Right(inner) => inner.source(),
        }
    }
}

/// A [`Body`] that doesn't contain any data.
pub struct EmptyBody<D = Bytes> {
    _marker: PhantomData<D>,
}

impl<D> EmptyBody<D> {
    /// Create a new [`EmptyBody`].
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<D> Default for EmptyBody<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D> fmt::Debug for EmptyBody<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EmptyBody")
    }
}

impl<D> Body for EmptyBody<D>
where
    D: Buf,
{
    type Data = D;
    type Error = Infallible;

    fn poll_data(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        Poll::Ready(None)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        Poll::Ready(Ok(None))
    }
}
