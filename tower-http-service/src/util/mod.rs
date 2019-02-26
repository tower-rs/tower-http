//! Types and utilities for working with `HttpService` and `Body`.

mod as_service;
mod body_ext;
mod into_buf_stream;
mod into_service;

pub use self::as_service::AsService;
pub use self::body_ext::BodyExt;
pub use self::into_service::IntoService;
pub use self::into_buf_stream::IntoBufStream;
