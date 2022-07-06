use super::decompression::BodyInner;
use crate::{
    compression_utils::{AcceptEncoding, WrapBody},
    content_encoding::SupportedEncodings,
    decompression::DecompressionBody,
};
use http::{header, Request, Response};
use http_body::Body;
use std::task::{Context, Poll};
use tower_layer::Layer;
use tower_service::Service;

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
    pub fn gzip(mut self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to request the Deflate encoding.
    #[cfg(feature = "decompression-deflate")]
    pub fn deflate(mut self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to request the Brotli encoding.
    #[cfg(feature = "decompression-br")]
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
}

#[derive(Debug, Clone)]
pub struct Decompression<S> {
    inner: S,
    accept: AcceptEncoding,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for Decompression<S>
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

impl<S> Decompression<S> {
    /// Creates a new `Decompression` wrapping the `service`.
    pub fn new(service: S) -> Self {
        Self {
            inner: service,
            accept: AcceptEncoding::default(),
        }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `Decompression` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer() -> DecompressionLayer {
        DecompressionLayer::new()
    }

    /// Sets whether to request the gzip encoding.
    #[cfg(feature = "decompression-gzip")]
    pub fn gzip(mut self, enable: bool) -> Self {
        self.accept.set_gzip(enable);
        self
    }

    /// Sets whether to request the Deflate encoding.
    #[cfg(feature = "decompression-deflate")]
    pub fn deflate(mut self, enable: bool) -> Self {
        self.accept.set_deflate(enable);
        self
    }

    /// Sets whether to request the Brotli encoding.
    #[cfg(feature = "decompression-br")]
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use http::Response;
    use http_body::Body as _;
    use hyper::{Body, Error, Request, Server};
    use std::io::Write;
    use std::net::SocketAddr;
    use tower::make::Shared;
    use tower::{service_fn, Service, ServiceExt};

    #[tokio::test]
    async fn should_decode_gzip_encoded_body() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"Hello, World!").unwrap();
        let body = encoder.finish().unwrap();
        let req = Request::builder()
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(body))
            .unwrap();

        let mut svc = Decompression::new(service_fn(handle_asserts_on_body));
        let _ = svc.ready().await.unwrap().call(req).await.unwrap();
    }

    #[tokio::test]
    async fn should_not_decode_unencoded_body() {
        let req = Request::builder()
            .body(Body::from("Hello, World!"))
            .unwrap();

        let mut svc = Decompression::new(service_fn(handle_asserts_on_body));
        let _ = svc.ready().await.unwrap().call(req).await.unwrap();
    }

    async fn handle_asserts_on_body(
        req: Request<DecompressionBody<Body>>,
    ) -> Result<Response<Body>, Error> {
        let mut body = req.into_body();
        let mut data = BytesMut::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.unwrap();
            data.extend_from_slice(&chunk[..]);
        }
        let decompressed_data = String::from_utf8(data.freeze().to_vec())
            .expect("Data should be decoded and therefore valid utf-8");
        assert_eq!(decompressed_data, "Hello, World!");

        Ok(Response::new(Body::empty()))
    }

    #[allow(dead_code)]
    async fn is_compatible_with_hyper() {
        let svc = service_fn(handle_asserts_on_body);
        let svc = Decompression::new(svc);

        let make_service = Shared::new(svc);

        let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
        let server = Server::bind(&addr).serve(make_service);
        server.await.unwrap();
    }
}
