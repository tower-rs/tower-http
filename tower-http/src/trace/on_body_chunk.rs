use std::time::Duration;
use tracing::Span;

/// Trait used to tell [`Trace`] what to do when a body chunk has been sent.
///
/// [`Trace`]: super::Trace
pub trait OnBodyChunk<B> {
    /// Do the thing.
    ///
    /// `latency` is the duration since the response was sent or since the last body chunk as sent.
    ///
    /// `current_span` can be used to record field values that weren't known when the span was
    /// created.
    ///
    /// If you're using [hyper] as your server `B` will most likely be [`Bytes`].
    ///
    /// [hyper]: https://hyper.rs
    /// [`Bytes`]: https://docs.rs/bytes/latest/bytes/struct.Bytes.html
    fn on_body_chunk(&mut self, chunk: &B, latency: Duration, current_span: &Span);
}

impl<B, F> OnBodyChunk<B> for F
where
    F: FnMut(&B, Duration, &Span),
{
    fn on_body_chunk(&mut self, chunk: &B, latency: Duration, current_span: &Span) {
        self(chunk, latency, current_span)
    }
}

impl<B> OnBodyChunk<B> for () {
    #[inline]
    fn on_body_chunk(&mut self, _: &B, _: Duration, _: &Span) {}
}

/// The default [`OnBodyChunk`] implementation used by [`Trace`].
///
/// Simply does nothing.
///
/// [`Trace`]: super::Trace
#[derive(Debug, Default, Clone)]
pub struct DefaultOnBodyChunk {
    _priv: (),
}

impl DefaultOnBodyChunk {
    /// Create a new `DefaultOnBodyChunk`.
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl<B> OnBodyChunk<B> for DefaultOnBodyChunk {
    #[inline]
    fn on_body_chunk(&mut self, _: &B, _: Duration, _: &Span) {}
}
