pub(super) mod future;
pub(super) mod layer;
pub(super) mod service;

#[cfg(test)]
mod tests {
    use super::service::RequestDecompression;
    use crate::decompression::DecompressionBody;
    use bytes::BytesMut;
    use flate2::{write::GzEncoder, Compression};
    use http::{header, Response, StatusCode};
    use http_body::Body as _;
    use hyper::{Body, Error, Request, Server};
    use std::io::Write;
    use std::net::SocketAddr;
    use tower::{make::Shared, service_fn, Service, ServiceExt};

    #[tokio::test]
    async fn decompress_accepted_encoding() {
        let req = request_gzip();
        let mut svc = RequestDecompression::new(service_fn(assert_request_is_decompressed));
        let _ = svc.ready().await.unwrap().call(req).await.unwrap();
    }

    #[tokio::test]
    async fn support_unencoded_body() {
        let req = Request::builder().body(Body::from("Hello?")).unwrap();
        let mut svc = RequestDecompression::new(service_fn(assert_request_is_decompressed));
        let _ = svc.ready().await.unwrap().call(req).await.unwrap();
    }

    #[tokio::test]
    async fn unaccepted_content_encoding_returns_unsupported_media_type() {
        let req = request_gzip();
        let mut svc = RequestDecompression::new(service_fn(should_not_be_called)).gzip(false);
        let res = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(StatusCode::UNSUPPORTED_MEDIA_TYPE, res.status());
    }

    #[tokio::test]
    async fn pass_through_unsupported_encoding_when_enabled() {
        let req = request_gzip();
        let mut svc = RequestDecompression::new(service_fn(assert_request_is_passed_through))
            .pass_through_unaccepted(true)
            .gzip(false);
        let _ = svc.ready().await.unwrap().call(req).await.unwrap();
    }

    async fn assert_request_is_decompressed(
        req: Request<DecompressionBody<Body>>,
    ) -> Result<Response<Body>, Error> {
        let (parts, mut body) = req.into_parts();
        let body = read_body(&mut body).await;

        assert_eq!(body, b"Hello?");
        assert!(!parts.headers.contains_key(header::CONTENT_ENCODING));

        Ok(Response::new(Body::from("Hello, World!")))
    }

    async fn assert_request_is_passed_through(
        req: Request<DecompressionBody<Body>>,
    ) -> Result<Response<Body>, Error> {
        let (parts, mut body) = req.into_parts();
        let body = read_body(&mut body).await;

        assert_ne!(body, b"Hello?");
        assert!(parts.headers.contains_key(header::CONTENT_ENCODING));

        Ok(Response::new(Body::empty()))
    }

    async fn should_not_be_called(
        _: Request<DecompressionBody<Body>>,
    ) -> Result<Response<Body>, Error> {
        panic!("Inner service should not be called");
    }

    fn request_gzip() -> Request<Body> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"Hello?").unwrap();
        let body = encoder.finish().unwrap();
        Request::builder()
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(body))
            .unwrap()
    }

    async fn read_body(body: &mut DecompressionBody<Body>) -> Vec<u8> {
        let mut data = BytesMut::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.unwrap();
            data.extend_from_slice(&chunk[..]);
        }
        data.freeze().to_vec()
    }

    #[allow(dead_code)]
    async fn is_compatible_with_hyper() {
        let svc = service_fn(assert_request_is_decompressed);
        let svc = RequestDecompression::new(svc);

        let make_service = Shared::new(svc);

        let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
        let server = Server::bind(&addr).serve(make_service);
        server.await.unwrap();
    }
}
