use crate::LatencyUnit;
use std::{fmt, time::Duration};
use tracing::Level;

use super::DEFAULT_ERROR_LEVEL;

pub trait OnFailure<FailureClass> {
    fn on_failure(self, failure_classification: FailureClass, latency: Duration);
}

impl<FailureClass> OnFailure<FailureClass> for () {
    #[inline]
    fn on_failure(self, _: FailureClass, _: Duration) {}
}

impl<F, FailureClass> OnFailure<FailureClass> for F
where
    F: FnOnce(FailureClass, Duration),
{
    fn on_failure(self, failure_classification: FailureClass, latency: Duration) {
        self(failure_classification, latency)
    }
}

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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn level(self, level: Level) -> Self {
        Self { level, ..self }
    }

    pub fn latency_unit(self, latency_unit: LatencyUnit) -> Self {
        Self {
            latency_unit,
            ..self
        }
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
    fn on_failure(self, failure_classification: FailureClass, latency: Duration) {
        log_pattern_match!(
            self,
            &failure_classification,
            latency,
            [ERROR, WARN, INFO, DEBUG, TRACE]
        );
    }
}
