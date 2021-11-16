use super::{CompressionBody, CompressionLayer, ResponseFuture};
use crate::{compression_utils::AcceptEncoding, content_encoding::Encoding};
use http::{Request, Response};
use http_body::Body;
use std::task::{Context, Poll};
use tower_service::Service;
use crate::compression::compression_predicate::{CompressionPredicate, DefaultCompressionPredicate};

/// Compress response bodies of the underlying service.
///
/// This uses the `Accept-Encoding` header to pick an appropriate encoding and adds the
/// `Content-Encoding` header to responses.
///
/// See the [module docs](crate::compression) for more details.
#[derive(Clone, Copy)]
pub struct Compression<S, P = DefaultCompressionPredicate> {
    pub(crate) inner: S,
    pub(crate) accept: AcceptEncoding,
    pub(crate) compression_predicate: P,
}

impl<S> Compression<S, DefaultCompressionPredicate> {
    /// Creates a new `Compression` wrapping the `service`.
    pub fn new(service: S) -> Compression<S, DefaultCompressionPredicate> {
        Self {
            inner: service,
            accept: AcceptEncoding::default(),
            compression_predicate: DefaultCompressionPredicate::default(),
        }
    }
}

impl<S, P> Compression<S, P> {
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
    pub fn gzip(mut self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to enable the Deflate encoding.
    #[cfg(feature = "compression-deflate")]
    #[cfg_attr(docsrs, doc(cfg(feature = "compression-deflate")))]
    pub fn deflate(mut self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to enable the Brotli encoding.
    #[cfg(feature = "compression-br")]
    #[cfg_attr(docsrs, doc(cfg(feature = "compression-br")))]
    pub fn br(mut self, enable: bool) -> Self {
        self.accept.set_br(enable);
        self
    }

    /// Disables the gzip encoding.
    ///
    /// This method is available even if the `gzip` crate feature is disabled.
    pub fn no_gzip(mut self) -> Self {
        self.accept.set_gzip(false);
        self
    }

    /// Disables the Deflate encoding.
    ///
    /// This method is available even if the `deflate` crate feature is disabled.
    pub fn no_deflate(mut self) -> Self {
        self.accept.set_deflate(false);
        self
    }

    /// Disables the Brotli encoding.
    ///
    /// This method is available even if the `br` crate feature is disabled.
    pub fn no_br(mut self) -> Self {
        self.accept.set_br(false);
        self
    }

    /// Replace the current compression predicate.
    ///
    /// The default predicate is [`DefaultCompressionPredicate`] which disables compression of gRPC
    /// (gRPC has its own protocol specific compression system) and responses who's
    /// mime type starts with `image/`.
    ///
    /// # Example
    ///
    /// For some reason compressing JSON is undesired
    ///
    /// ```
    /// use tower_http::compression::{Compression, compression_predicate::NotForContentType};
    /// use tower::util::service_fn;
    ///
    /// // Placeholder service_fn
    /// let service = service_fn(|_: ()| async {
    ///     Ok::<_, std::io::Error>(http::Response::new(()))
    /// });
    /// let service = Compression::new(service)
    ///     .compress_when(NotForContentType::new("application/json"));
    /// ```
    pub fn compress_when<C>(self, compression_predicate: C) -> Compression<S, C>
    where
        C: CompressionPredicate,
    {
        Compression {
            inner: self.inner,
            accept: self.accept,
            compression_predicate
        }
    }
}

impl<ReqBody, ResBody, S, P> Service<Request<ReqBody>> for Compression<S, P>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body,
    P: CompressionPredicate,
{
    type Response = Response<CompressionBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, P>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let encoding = Encoding::from_headers(req.headers(), self.accept);

        ResponseFuture {
            inner: self.inner.call(req),
            encoding,
            compression_predicate: self.compression_predicate.clone()
        }
    }
}
