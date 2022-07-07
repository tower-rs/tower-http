use super::service::RequestDecompression;
use crate::compression_utils::AcceptEncoding;
use tower_layer::Layer;

#[derive(Debug, Default, Clone)]
pub struct RequestDecompressionLayer {
    accept: AcceptEncoding,
}

impl<S> Layer<S> for RequestDecompressionLayer {
    type Service = RequestDecompression<S>;

    fn layer(&self, service: S) -> Self::Service {
        RequestDecompression {
            inner: service,
            accept: self.accept,
        }
    }
}

impl RequestDecompressionLayer {
    /// Creates a new `RequestDecompressionLayer`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets whether to support
    /// gzip encoding.
    #[cfg(feature = "decompression-gzip")]
    pub fn gzip(mut self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to support
    /// Deflate encoding.
    #[cfg(feature = "decompression-deflate")]
    pub fn deflate(mut self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to support
    /// Brotli encoding.
    #[cfg(feature = "decompression-br")]
    pub fn br(mut self, enable: bool) -> Self {
        self.accept.set_br(enable);
        self
    }

    /// Disables support for gzip encoding.
    ///
    /// This method is available even if the `gzip` crate feature is disabled.
    pub fn no_gzip(mut self) -> Self {
        self.accept.set_gzip(false);
        self
    }

    /// Disables support for Deflate encoding.
    ///
    /// This method is available even if the `deflate` crate feature is disabled.
    pub fn no_deflate(mut self) -> Self {
        self.accept.set_deflate(false);
        self
    }

    /// Disables support for Brotli encoding.
    ///
    /// This method is available even if the `br` crate feature is disabled.
    pub fn no_br(mut self) -> Self {
        self.accept.set_br(false);
        self
    }
}
