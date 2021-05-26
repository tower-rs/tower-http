//! Middlewares for adding metrics to services.
//!
//! Supported metrics:
//!
//! - [In-flight requests][]: Measure the number of requests a service is currently processing.
//! - [Traffic][]: Measure how many responses or errors a service is producing.
//!
//! [In-flight requests]: in_flight_requests
//! [Traffic]: traffic

pub mod in_flight_requests;
pub mod traffic;

#[doc(inline)]
pub use self::{
    in_flight_requests::{InFlightRequests, InFlightRequestsLayer},
    traffic::{Traffic, TrafficLayer},
};
