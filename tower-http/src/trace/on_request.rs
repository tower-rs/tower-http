use super::DEFAULT_MESSAGE_LEVEL;
use http::Request;
use tracing::Level;
use tracing::Span;

/// Trait used to tell [`Trace`] what to do when a request is received.
///
/// [`Trace`]: super::Trace
pub trait OnRequest<B> {
    /// Do the thing.
    ///
    /// `span` is the `tracing` [`Span`] corresponding to this request, produced
    /// the closure passed to [`TraceLayer::make_span_with`]. It can be used to
    /// [record field values][record] that weren't known when the span was
    /// created.
    ///
    /// [`Span`]: https://docs.rs/tracing/latest/tracing/span/index.html
    /// [record]: https://docs.rs/tracing/latest/tracing/span/struct.Span.html#method.record
    fn on_request(&mut self, request: &Request<B>, span: &Span);
}

impl<B> OnRequest<B> for () {
    #[inline]
    fn on_request(&mut self, _: &Request<B>, _: &Span) {}
}

impl<B, F> OnRequest<B> for F
where
    F: FnMut(&Request<B>, &Span),
{
    fn on_request(&mut self, request: &Request<B>, span: &Span) {
        self(request, span)
    }
}

/// The default [`OnRequest`] implementation used by [`Trace`].
///
/// [`Trace`]: super::Trace
#[derive(Clone, Debug)]
pub struct DefaultOnRequest {
    level: Level,
}

impl Default for DefaultOnRequest {
    fn default() -> Self {
        Self {
            level: DEFAULT_MESSAGE_LEVEL,
        }
    }
}

impl DefaultOnRequest {
    /// Create a new `DefaultOnRequest`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the [`Level`] used for [tracing events].
    ///
    /// Defaults to [`Level::DEBUG`].
    ///
    /// [tracing events]: https://docs.rs/tracing/latest/tracing/#events
    /// [`Level::DEBUG`]: https://docs.rs/tracing/latest/tracing/struct.Level.html#associatedconstant.DEBUG
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }
}

impl<B> OnRequest<B> for DefaultOnRequest {
    fn on_request(&mut self, _: &Request<B>, _: &Span) {
        match self.level {
            Level::ERROR => {
                tracing::event!(Level::ERROR, "started processing request");
            }
            Level::WARN => {
                tracing::event!(Level::WARN, "started processing request");
            }
            Level::INFO => {
                tracing::event!(Level::INFO, "started processing request");
            }
            Level::DEBUG => {
                tracing::event!(Level::DEBUG, "started processing request");
            }
            Level::TRACE => {
                tracing::event!(Level::TRACE, "started processing request");
            }
        }
    }
}
