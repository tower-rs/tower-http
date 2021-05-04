//! Middlewares for adding metrics to services.
//!
//! Supported metrics:
//!
//! - [In-flight requests][]: Measure the number of requests a service is currently processing.
//!
//! [In-flight requests]: in_flight_requests

pub mod in_flight_requests;

// requests per scond
// error rate
// throughput, bytes per second/minute

#[doc(inline)]
pub use self::in_flight_requests::{InFlightRequests, InFlightRequestsLayer};
