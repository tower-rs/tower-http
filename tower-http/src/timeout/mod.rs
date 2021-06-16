//! Middleware that adds timeouts to request or response bodies.

pub mod response_body;

#[doc(inline)]
pub use self::response_body::{ResponseBodyTimeout, ResponseBodyTimeoutLayer};
