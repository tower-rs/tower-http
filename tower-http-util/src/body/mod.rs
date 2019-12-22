//! Types and utilities for working with `Body`.

use http_body::Body;

/// An extension trait for `Body` providing additional adapters.
pub trait BodyExt: Body {}

impl<T: Body> BodyExt for T {}
