use super::{Compression, MIN_SIZE_DEFAULT};
use crate::compression_utils::AcceptEncoding;
use tower_layer::Layer;

/// Compress response bodies of the underlying service.
///
/// This uses the `Accept-Encoding` header to pick an appropriate encoding and adds the
/// `Content-Encoding` header to responses.
///
/// See the [module docs](crate::compression) for more details.
#[derive(Clone, Debug, Default)]
pub struct CompressionLayer {
    accept: AcceptEncoding,
    min_size: u64,
}

impl<S> Layer<S> for CompressionLayer {
    type Service = Compression<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Compression::new(inner)
    }
}

impl CompressionLayer {
    /// Create a new [`CompressionLayer`]
    pub fn new() -> Self {
        Self {
            accept: Default::default(),
            min_size: MIN_SIZE_DEFAULT,
        }
    }

    /// Sets whether to enable the gzip encoding.
    #[cfg(feature = "compression-gzip")]
    #[cfg_attr(docsrs, doc(cfg(feature = "compression-gzip")))]
    pub fn gzip(self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to enable the Deflate encoding.
    #[cfg(feature = "compression-deflate")]
    #[cfg_attr(docsrs, doc(cfg(feature = "compression-deflate")))]
    pub fn deflate(self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to enable the Brotli encoding.
    #[cfg(feature = "compression-br")]
    #[cfg_attr(docsrs, doc(cfg(feature = "compression-br")))]
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

    /// Configures the minimum size at which the layer starts compressing response
    /// bodies.
    ///
    /// Any response smaller than `min` will not be compressed. The response size
    /// is determined by inspecting [`Body::size_hint()`] and the `content-length`
    /// header.
    ///
    /// Passing `0` makes the layer compress every response.
    ///
    /// The default is 32 bytes.
    pub fn min_size(mut self, min: u64) -> Self {
        self.min_size = min;
        self
    }
}
