use super::Decompression;
use crate::compression_utils::AcceptEncoding;
use tower_layer::Layer;

/// Decompresses response bodies of the underlying service.
///
/// This adds the `Accept-Encoding` header to requests and transparently decompresses response
/// bodies based on the `Content-Encoding` header.
///
/// See the [module docs](crate::decompression) for more details.
#[derive(Debug, Default, Clone)]
pub struct DecompressionLayer {
    accept: AcceptEncoding,
}

impl<S> Layer<S> for DecompressionLayer {
    type Service = Decompression<S>;

    fn layer(&self, service: S) -> Self::Service {
        Decompression {
            inner: service,
            accept: self.accept,
        }
    }
}

impl DecompressionLayer {
    /// Creates a new `DecompressionLayer`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets whether to request the gzip encoding.
    #[cfg(feature = "decompression-gzip")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression-gzip")))]
    pub fn gzip(self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to request the Deflate encoding.
    #[cfg(feature = "decompression-deflate")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression-deflate")))]
    pub fn deflate(self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to request the Brotli encoding.
    #[cfg(feature = "decompression-br")]
    #[cfg_attr(docsrs, doc(cfg(feature = "decompression-br")))]
    pub fn br(self, enable: bool) -> Self {
        self.accept.set_br(enable);
        self
    }

    /// Disables the gzip encoding.
    ///
    /// This method is available even if the `gzip` crate feature is disabled.
    pub fn no_gzip(self) -> Self {
        self.accept.set_gzip(false);
        self
    }

    /// Disables the Deflate encoding.
    ///
    /// This method is available even if the `deflate` crate feature is disabled.
    pub fn no_deflate(self) -> Self {
        self.accept.set_deflate(false);
        self
    }

    /// Disables the Brotli encoding.
    ///
    /// This method is available even if the `br` crate feature is disabled.
    pub fn no_br(self) -> Self {
        self.accept.set_br(false);
        self
    }
}
