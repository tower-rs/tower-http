use super::layer::RequestDecompressionLayer;
use crate::{
    compression_utils::AcceptEncoding, compression_utils::WrapBody,
    content_encoding::SupportedEncodings, decompression::body::BodyInner,
    decompression::DecompressionBody,
};
use http::{header, Request, Response};
use http_body::Body;
use std::task::{Context, Poll};
use tower_service::Service;

#[derive(Debug, Clone)]
pub struct RequestDecompression<S> {
    pub(super) inner: S,
    pub(super) accept: AcceptEncoding,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for RequestDecompression<S>
where
    S: Service<Request<DecompressionBody<ReqBody>>, Response = Response<ResBody>>,
    ReqBody: Body,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let (mut parts, body) = req.into_parts();

        let req =
            if let header::Entry::Occupied(entry) = parts.headers.entry(header::CONTENT_ENCODING) {
                let body = match entry.get().as_bytes() {
                    #[cfg(feature = "decompression-gzip")]
                    b"gzip" if self.accept.gzip() => {
                        DecompressionBody::new(BodyInner::gzip(WrapBody::new(body)))
                    }
                    #[cfg(feature = "decompression-deflate")]
                    b"deflate" if self.accept.deflate() => {
                        DecompressionBody::new(BodyInner::deflate(WrapBody::new(body)))
                    }
                    #[cfg(feature = "decompression-br")]
                    b"br" if self.accept.br() => {
                        DecompressionBody::new(BodyInner::brotli(WrapBody::new(body)))
                    }
                    _ => {
                        return self.inner.call(Request::from_parts(
                            parts,
                            DecompressionBody::new(BodyInner::identity(body)),
                        ))
                    }
                };

                entry.remove();
                parts.headers.remove(header::CONTENT_LENGTH);

                Request::from_parts(parts, body)
            } else {
                Request::from_parts(parts, DecompressionBody::new(BodyInner::identity(body)))
            };

        self.inner.call(req)
    }
}

impl<S> RequestDecompression<S> {
    /// Creates a new `RequestDecompression` wrapping the `service`.
    pub fn new(service: S) -> Self {
        Self {
            inner: service,
            accept: AcceptEncoding::default(),
        }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `RequestDecompression` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer() -> RequestDecompressionLayer {
        RequestDecompressionLayer::new()
    }

    /// Sets whether to support gzip encoding.
    #[cfg(feature = "decompression-gzip")]
    pub fn gzip(mut self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to support Deflate encoding.
    #[cfg(feature = "decompression-deflate")]
    pub fn deflate(mut self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to support Brotli encoding.
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
