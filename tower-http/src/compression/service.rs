use crate::accept_encoding::AcceptEncoding;
use http::{Request, Response};
use http_body::Body;
use std::task::{Context, Poll};
use tower_service::Service;

use super::{CompressionBody, Encoding, ResponseFuture};

/// Compress response bodies of the underlying service.
///
/// This uses the `Accept-Encoding` header to pick an appropriate encoding and adds the
/// `Content-Encoding` header to responses.
#[derive(Clone, Copy)]
pub struct Compression<S> {
    pub(crate) inner: S,
    pub(crate) accept: AcceptEncoding,
}

impl<S> Compression<S> {
    /// Creates a new `Compression` wrapping the `service`.
    pub fn new(service: S) -> Self {
        Self {
            inner: service,
            accept: AcceptEncoding::default(),
        }
    }

    define_inner_service_accessors!();

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
        }
    }
}
