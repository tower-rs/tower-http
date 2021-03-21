use super::DEFAULT_MESSAGE_LEVEL;
use crate::LatencyUnit;
use http::header::HeaderMap;
use std::time::Duration;
use tracing::Level;

pub trait OnEos {
    fn on_eos(self, trailers: Option<&HeaderMap>, stream_duration: Duration);
}

impl OnEos for () {
    #[inline]
    fn on_eos(self, _: Option<&HeaderMap>, _: Duration) {}
}

impl<F> OnEos for F
where
    F: FnOnce(Option<&HeaderMap>, Duration),
{
    fn on_eos(self, trailers: Option<&HeaderMap>, stream_duration: Duration) {
        self(trailers, stream_duration)
    }
}

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
    pub fn new() -> Self {
        Self::default()
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
        $this:expr, $stream_duration:expr, [$($level:ident),*]
    ) => {
        match ($this.level, $this.latency_unit) {
            $(
                (Level::$level, LatencyUnit::Millis) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} ms", $stream_duration.as_millis()),
                        "end of stream"
                    );
                }
                (Level::$level, LatencyUnit::Nanos) => {
                    tracing::event!(
                        Level::$level,
                        stream_duration = format_args!("{} ns", $stream_duration.as_nanos()),
                        "end of stream"
                    );
                }
            )*
        }
    };
}

impl OnEos for DefaultOnEos {
    fn on_eos(self, _trailers: Option<&HeaderMap>, stream_duration: Duration) {
        log_pattern_match!(self, stream_duration, [ERROR, WARN, INFO, DEBUG, TRACE]);
    }
}
