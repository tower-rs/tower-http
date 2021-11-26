use super::DEFAULT_MESSAGE_LEVEL;
use crate::{classify::grpc_errors_as_failures::ParsedGrpcStatus, LatencyUnit};
use http::header::HeaderMap;
use std::time::Duration;
use tracing::{Level, Span};

/// Trait used to tell [`Trace`] what to do when a stream closes.
///
/// See the [module docs](../trace/index.html#on_eos) for details on exactly when the `on_eos`
/// callback is called.
///
/// [`Trace`]: super::Trace
pub trait OnEos {
    /// Do the thing.
    ///
    /// `stream_duration` is the duration since the response was sent.
    ///
    /// `span` is the `tracing` [`Span`], corresponding to this request, produced by the closure
    /// passed to [`TraceLayer::make_span_with`]. It can be used to [record field values][record]
    /// that weren't known when the span was created.
    ///
    /// [`Span`]: https://docs.rs/tracing/latest/tracing/span/index.html
    /// [record]: https://docs.rs/tracing/latest/tracing/span/struct.Span.html#method.record
    /// [`TraceLayer::make_span_with`]: crate::trace::TraceLayer::make_span_with
    fn on_eos(self, trailers: Option<&HeaderMap>, stream_duration: Duration, span: &Span);
}

impl OnEos for () {
    #[inline]
    fn on_eos(self, _: Option<&HeaderMap>, _: Duration, _: &Span) {}
}

impl<F> OnEos for F
where
    F: FnOnce(Option<&HeaderMap>, Duration, &Span),
{
    fn on_eos(self, trailers: Option<&HeaderMap>, stream_duration: Duration, span: &Span) {
        self(trailers, stream_duration, span)
    }
}

/// The default [`OnEos`] implementation used by [`Trace`].
///
/// [`Trace`]: super::Trace
#[derive(Clone, Debug)]
pub struct DefaultOnEos {
    level: Level,
    latency_unit: LatencyUnit,
}

impl Default for DefaultOnEos {
    fn default() -> Self {
        Self {
            level: DEFAULT_MESSAGE_LEVEL,
            latency_unit: LatencyUnit::Millis,
        }
    }
}

impl DefaultOnEos {
    /// Create a new [`DefaultOnEos`].
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

    /// Set the [`LatencyUnit`] latencies will be reported in.
    ///
    /// Defaults to [`LatencyUnit::Millis`].
    pub fn latency_unit(mut self, latency_unit: LatencyUnit) -> Self {
        self.latency_unit = latency_unit;
        self
    }
}

// Repeating this pattern match for each case is tedious. So we do it with a quick and
// dirty macro.
//
// Tracing requires all these parts to be declared statically. You cannot easily build
// events dynamically.
#[allow(unused_macros)]
macro_rules! log_pattern_match {
    (
        $this:expr, $stream_duration:expr, $status:expr, [$($level:ident),*]
    ) => {
        match ($this.level, $this.latency_unit, $status) {
            $(
                (Level::$level, LatencyUnit::Seconds, None) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} s", $stream_duration.as_secs_f64()),
                        "end of stream"
                    );
                }
                (Level::$level, LatencyUnit::Seconds, Some(status)) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} s", $stream_duration.as_secs_f64()),
                        status = status,
                        "end of stream"
                    );
                }

                (Level::$level, LatencyUnit::Millis, None) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} ms", $stream_duration.as_millis()),
                        "end of stream"
                    );
                }
                (Level::$level, LatencyUnit::Millis, Some(status)) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} ms", $stream_duration.as_millis()),
                        status = status,
                        "end of stream"
                    );
                }

                (Level::$level, LatencyUnit::Micros, None) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} μs", $stream_duration.as_micros()),
                        "end of stream"
                    );
                }
                (Level::$level, LatencyUnit::Micros, Some(status)) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} μs", $stream_duration.as_micros()),
                        status = status,
                        "end of stream"
                    );
                }

                (Level::$level, LatencyUnit::Nanos, None) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} ns", $stream_duration.as_nanos()),
                        "end of stream"
                    );
                }
                (Level::$level, LatencyUnit::Nanos, Some(status)) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} ns", $stream_duration.as_nanos()),
                        status = status,
                        "end of stream"
                    );
                }
            )*
        }
    };
}

impl OnEos for DefaultOnEos {
    fn on_eos(self, trailers: Option<&HeaderMap>, stream_duration: Duration, _span: &Span) {
        let status = trailers.and_then(|trailers| {
            match crate::classify::grpc_errors_as_failures::classify_grpc_metadata(
                trailers,
                crate::classify::GrpcCode::Ok.into_bitmask(),
            ) {
                ParsedGrpcStatus::Success
                | ParsedGrpcStatus::HeaderNotString
                | ParsedGrpcStatus::HeaderNotInt => Some(0),
                ParsedGrpcStatus::NonSuccess(status) => Some(status.get()),
                ParsedGrpcStatus::GrpcStatusHeaderMissing => None,
            }
        });

        log_pattern_match!(
            self,
            stream_duration,
            status,
            [ERROR, WARN, INFO, DEBUG, TRACE]
        );
    }
}
