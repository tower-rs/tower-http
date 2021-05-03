use super::DEFAULT_MESSAGE_LEVEL;
use crate::LatencyUnit;
use http::Response;
use std::time::Duration;
use tracing::Level;

/// Trait used to tell [`Trace`] what to do when a response has been produced.
///
/// [`Trace`]: super::Trace
pub trait OnResponse<B> {
    /// Do the thing.
    ///
    /// `latency` is the duration since the request was received.
    fn on_response(self, response: &Response<B>, latency: Duration);
}

impl<B> OnResponse<B> for () {
    #[inline]
    fn on_response(self, _: &Response<B>, _: Duration) {}
}

impl<B, F> OnResponse<B> for F
where
    F: FnOnce(&Response<B>, Duration),
{
    fn on_response(self, response: &Response<B>, latency: Duration) {
        self(response, latency)
    }
}

/// The default [`OnResponse`] implementation used by [`Trace`].
///
/// [`Trace`]: super::Trace
#[derive(Clone, Debug)]
pub struct DefaultOnResponse {
    level: Level,
    latency_unit: LatencyUnit,
    include_headers: bool,
}

impl Default for DefaultOnResponse {
    fn default() -> Self {
        Self {
            level: DEFAULT_MESSAGE_LEVEL,
            latency_unit: LatencyUnit::Millis,
            include_headers: false,
        }
    }
}

impl DefaultOnResponse {
    /// Create a new `DefaultOnResponse`.
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

    /// Include response headers on the [`Event`].
    ///
    /// By default headers are not included.
    ///
    /// [`Event`]: tracing::Event
    pub fn include_headers(mut self, include_headers: bool) -> Self {
        self.include_headers = include_headers;
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
        $this:expr, $res:expr, $latency:expr, $include_headers:expr, [$($level:ident),*]
    ) => {
        match ($this.level, $include_headers, $this.latency_unit) {
            $(
                (Level::$level, true, LatencyUnit::Millis) => {
                    tracing::event!(
                        Level::$level,
                        latency = format_args!("{} ms", $latency.as_millis()),
                        status = status($res),
                        response_headers = ?$res.headers(),
                        "finished processing request"
                    );
                }
                (Level::$level, false, LatencyUnit::Millis) => {
                    tracing::event!(
                        Level::$level,
                        latency = format_args!("{} ms", $latency.as_millis()),
                        status = status($res),
                        "finished processing request"
                    );
                }
                (Level::$level, true, LatencyUnit::Micros) => {
                    tracing::event!(
                        Level::$level,
                        latency = format_args!("{} μs", $latency.as_micros()),
                        status = status($res),
                        response_headers = ?$res.headers(),
                        "finished processing request"
                    );
                }
                (Level::$level, false, LatencyUnit::Micros) => {
                    tracing::event!(
                        Level::$level,
                        latency = format_args!("{} μs", $latency.as_micros()),
                        status = status($res),
                        "finished processing request"
                    );
                }
                (Level::$level, true, LatencyUnit::Nanos) => {
                    tracing::event!(
                        Level::$level,
                        latency = format_args!("{} ns", $latency.as_nanos()),
                        status = status($res),
                        response_headers = ?$res.headers(),
                        "finished processing request"
                    );
                }
                (Level::$level, false, LatencyUnit::Nanos) => {
                    tracing::event!(
                        Level::$level,
                        latency = format_args!("{} ns", $latency.as_nanos()),
                        status = status($res),
                        "finished processing request"
                    );
                }
            )*
        }
    };
}

impl<B> OnResponse<B> for DefaultOnResponse {
    fn on_response(self, response: &Response<B>, latency: Duration) {
        todo!()

        // log_pattern_match!(
        //     self,
        //     response,
        //     latency,
        //     self.include_headers,
        //     [ERROR, WARN, INFO, DEBUG, TRACE]
        // );
    }
}

// fn status<B>(res: &Response<B>) -> i32 {
//     let is_grpc = res
//         .headers()
//         .get(http::header::CONTENT_TYPE)
//         .map_or(false, |value| value == "application/grpc");

//     if is_grpc {
//         if let Some(Err(status)) = crate::classify::classify_grpc_metadata(res.headers()) {
//             status
//         } else {
//             // 0 is success in gRPC
//             0
//         }
//     } else {
//         res.status().as_u16().into()
//     }
// }
