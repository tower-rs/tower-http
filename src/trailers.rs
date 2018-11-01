use futures::Poll;
use http::HeaderMap;

/// Poll for an optional **single** `HeaderMap` of trailers.
pub trait BodyTrailers {
    /// Error that may occur when polling for trailers.
    type TrailersError;

    /// Poll for an optional **single** `HeaderMap` of trailers.
    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError>;
}
