use super::{CompressionBody, CompressionLayer, Encoding, ResponseFuture, MIN_SIZE_DEFAULT};
use crate::compression_utils::AcceptEncoding;
use http::{Request, Response};
use http_body::Body;
use std::task::{Context, Poll};
use tower_service::Service;

/// Compress response bodies of the underlying service.
///
/// This uses the `Accept-Encoding` header to pick an appropriate encoding and adds the
/// `Content-Encoding` header to responses.
///
/// See the [module docs](crate::compression) for more details.
#[derive(Clone, Copy)]
pub struct Compression<S> {
    pub(crate) inner: S,
    pub(crate) accept: AcceptEncoding,
    pub(crate) min_size: u16,
}

impl<S> Compression<S> {
    /// Creates a new `Compression` wrapping the `service`.
    pub fn new(service: S) -> Self {
        Self {
            inner: service,
            accept: AcceptEncoding::default(),
            min_size: MIN_SIZE_DEFAULT,
        }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `Compression` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer() -> CompressionLayer {
        CompressionLayer::new()
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

    /// Configures the minimum size at which the service starts compressing response
    /// bodies.
    ///
    /// Any response smaller than `min` will not be compressed. The response size
    /// is determined by inspecting [`Body::size_hint()`] and the `content-length`
    /// header.
    ///
    /// Passing `0` makes the service compress every response.
    ///
    /// The default is 32 bytes.
    pub fn min_size(mut self, min: u16) -> Self {
        self.min_size = min;
        self
    }
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for Compression<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
{
    type Response = Response<CompressionBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let encoding = Encoding::from_headers(req.headers(), self.accept);

        ResponseFuture {
            inner: self.inner.call(req),
            encoding,
            min_size: self.min_size,
        }
    }
}
