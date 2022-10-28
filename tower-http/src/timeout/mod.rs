//! Middleware for setting timeouts on requests and responses.

mod body;
mod service;

pub use body::TimeoutBody;
pub use body::TimeoutError;
pub use service::RequestBodyTimeout;
pub use service::RequestBodyTimeoutLayer;
pub use service::ResponseBodyTimeout;
pub use service::ResponseBodyTimeoutLayer;
pub use service::Timeout;
pub use service::TimeoutLayer;
