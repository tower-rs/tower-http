use super::Compression;
use tower_layer::Layer;

/// Compress response bodies of the underlying service.
///
/// This uses the `Accept-Encoding` header to pick an appropriate encoding and adds the
/// `Content-Encoding` header to responses.
///
/// See the [module docs](crate::compression) for more details.
#[derive(Clone, Debug, Default)]
pub struct CompressionLayer {
    _priv: (),
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
        Self::default()
    }
}
