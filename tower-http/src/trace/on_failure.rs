use super::DEFAULT_ERROR_LEVEL;
use crate::LatencyUnit;
use std::{fmt, time::Duration};
use tracing::Level;

/// Trait used to tell [`Trace`] what to do when a request fails.
///
/// [`Trace`]: super::Trace
pub trait OnFailure<FailureClass> {
    /// Do the thing.
    ///
    /// `latency` is the duration since the request was received.
    fn on_failure(&mut self, failure_classification: FailureClass, latency: Duration);
}

impl<FailureClass> OnFailure<FailureClass> for () {
    #[inline]
    fn on_failure(&mut self, _: FailureClass, _: Duration) {}
}

impl<F, FailureClass> OnFailure<FailureClass> for F
where
    F: FnMut(FailureClass, Duration),
{
    fn on_failure(&mut self, failure_classification: FailureClass, latency: Duration) {
        self(failure_classification, latency)
    }
}

/// The default [`OnFailure`] implementation used by [`Trace`].
///
/// [`Trace`]: super::Trace
#[derive(Clone, Debug)]
pub struct DefaultOnFailure {
    level: Level,
    latency_unit: LatencyUnit,
}

impl Default for DefaultOnFailure {
    fn default() -> Self {
        Self {
            level: DEFAULT_ERROR_LEVEL,
            latency_unit: LatencyUnit::Millis,
        }
    }
}

impl DefaultOnFailure {
    /// Create a new `DefaultOnFailure`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the [`Level`] used for [tracing events].
    ///
    /// Defaults to [`Level::DEBUG`].
    ///
    /// [tracing events]: https://docs.rs/tracing/latest/tracing/#events
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
macro_rules! log_pattern_match {
    (
        $this:expr, $failure_classification:expr, $latency:expr, [$($level:ident),*]
    ) => {
        match ($this.level, $this.latency_unit) {
            $(
                (Level::$level, LatencyUnit::Millis) => {
                    tracing::event!(
                        Level::$level,
                        classification = tracing::field::display($failure_classification),
                        latency = format_args!("{} ms", $latency.as_millis()),
                        "response failed"
                    );
                }
                (Level::$level, LatencyUnit::Micros) => {
                    tracing::event!(
                        Level::$level,
                        classification = tracing::field::display($failure_classification),
                        latency = format_args!("{} μs", $latency.as_micros()),
                        "response failed"
                    );
                }
                (Level::$level, LatencyUnit::Nanos) => {
                    tracing::event!(
                        Level::$level,
                        classification = tracing::field::display($failure_classification),
                        latency = format_args!("{} ns", $latency.as_nanos()),
                        "response failed"
                    );
                }
            )*
        }
    };
}

impl<FailureClass> OnFailure<FailureClass> for DefaultOnFailure
where
    FailureClass: fmt::Display,
{
    fn on_failure(&mut self, failure_classification: FailureClass, latency: Duration) {
        log_pattern_match!(
            self,
            &failure_classification,
            latency,
            [ERROR, WARN, INFO, DEBUG, TRACE]
        );
    }
}
