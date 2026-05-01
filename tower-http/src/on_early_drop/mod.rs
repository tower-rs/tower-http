//! A utility to execute a closure when a value is dropped before completion.
//!
//! This module provides functionality to detect when a client disconnects before receiving a
//! full response. This is particularly useful for monitoring and logging early connection terminations,
//! which might happen due to client timeouts, disconnections, or cancellations.
//!
//! When a TCP connection is closed prematurely, HTTP services typically drop the corresponding service future
//! without providing any notification mechanism to the service. Without this middleware, when a
//! client closes the connection before a request completes (e.g., browser navigation away,
//! network issues), the entire future chain is dropped with no indication of what happened.
//! This can lead to "disappearing requests" that never show up in logs or metrics.
//!
//! This middleware layer leverages the `Drop` behavior to detect these situations and execute
//! a callback, allowing services to properly handle these early terminations.
//!
//! # Use Cases
//!
//! - **Logging**: Capture information about requests that were terminated early
//! - **Metrics**: Track client disconnects and incomplete requests
//! - **Recovery Handling**: Process interrupted requests for analysis, replay, or fallback processing
//!
//! # Architecture
//!
//! The module consists of four main components:
//!
//! - [`guard`]: Core guard implementation that tracks operation completion status
//! - [`future`]: Specialized future implementation that handles early drops without extra allocations
//! - [`layer`]: Tower layer that applies the early drop detection to a service
//! - [`service`]: Service implementation that uses the guard to monitor requests
//!
//! # Examples
//!
//! ## Using the Layer
//!
//! ```
//! use tower_http::on_early_drop::layer::OnEarlyDropLayer;
//! use tower::{ServiceBuilder, service_fn};
//! use http::{Request, Response};
//! use std::convert::Infallible;
//!
//! async fn handler(_: Request<String>) -> Result<Response<String>, Infallible> {
//!     Ok(Response::new(String::from("Hello, world!")))
//! }
//!
//! // Create a service with the OnEarlyDropLayer
//! let service = ServiceBuilder::new()
//!     .layer(OnEarlyDropLayer::new(|req: &Request<String>| {
//!         // Access information from the request if needed
//!         let uri = req.uri().to_string();
//!         let method = req.method().to_string();
//!         move || {
//!             println!("Request was dropped before completion: {} {}", method, uri);
//!             // In production, you might:
//!             // - Log the event with request details
//!             // - Increment a "dropped_requests" metric counter
//!         }
//!     }))
//!     .service_fn(handler);
//! ```
//!
//! ## Using the Guard Directly
//!
//! ```
//! use tower_http::on_early_drop::guard::OnEarlyDropGuard;
//! use std::time::Instant;
//! use http::{Request, Response};
//!
//! async fn handle_request(req: Request<String>) -> Response<String> {
//!     // Record the start time for latency tracking
//!     let start_time = Instant::now();
//!
//!     // Create a guard that will execute if the request is dropped early
//!     let mut guard = OnEarlyDropGuard::new(move || {
//!         // Calculate latency at drop time
//!         let latency = start_time.elapsed();
//!         println!("Request dropped after {:?}", latency);
//!     });
//!
//!     // Execute the main logic
//!     let response = process_request(req).await;
//!
//!     // Mark as completed so the guard doesn't fire
//!     guard.completed();
//!
//!     response
//! }
//!
//! async fn process_request(req: Request<String>) -> Response<String> {
//!     // Actual request processing here
//!     Response::new(String::from("Response content"))
//! }
//! ```
//!
//! ### Advanced Use Case: Business Logic Context
//!
//! There are several scenarios where directly using the guard is beneficial:
//!
//! 1. When you need to include business logic context that's only available during request processing
//! 2. When you need to customize the callback behavior based on runtime information
//!
//! ```
//! use tower_http::on_early_drop::guard::OnEarlyDropGuard;
//! use http::{Request, Response};
//!
//! async fn process_with_context(req: Request<String>) -> Response<String> {
//!     // Business logic context only available during request processing
//!     let request_id = String::from("req-123456");
//!
//!     // Clone the request to avoid borrowing issues
//!     let req_headers = req.headers().clone();
//!
//!     // Parse user ID from request - only available after parsing the request
//!     let user_id = req_headers
//!         .get("x-user-id")
//!         .and_then(|h| h.to_str().ok())
//!         .unwrap_or("anonymous");
//!
//!     // Clone values that will be moved into the closure but also used later
//!     let request_id_for_closure = request_id.clone();
//!
//!     // Create guard with access to the business context
//!     let mut guard = OnEarlyDropGuard::new(move || {
//!         // This closure now has access to business context like request_id and user_id
//!         // that was only available during request processing
//!         println!("Request dropped for user: {}, request_id: {}", user_id, request_id_for_closure);
//!
//!         // In a real application, you might:
//!         // - Log with structured context
//!         // - Record metrics with business context
//!         // - Perform cleanup operations specific to this request
//!     });
//!
//!     // Process the request with its business context
//!     let result = perform_business_logic(req, &request_id, &user_id).await;
//!
//!     // If we got here, request wasn't dropped early
//!     guard.completed();
//!
//!     result
//! }
//!
//! # async fn perform_business_logic(_req: Request<String>, _req_id: &str, _user_id: &str) -> Response<String> {
//! #     Response::new(String::from("Response with context"))
//! # }
//! ```

pub mod future;
pub mod guard;
pub mod layer;
pub mod service;
