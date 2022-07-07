pub(super) mod layer;
pub(super) mod service;

#[cfg(test)]
mod tests {
    use super::service::RequestDecompression;
    use crate::decompression::DecompressionBody;
    use bytes::BytesMut;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use http::{header, Response};
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

        let mut svc = RequestDecompression::new(service_fn(handle_asserts_on_body));
        let _ = svc.ready().await.unwrap().call(req).await.unwrap();
    }

    #[tokio::test]
    async fn should_not_decode_unencoded_body() {
        let req = Request::builder()
            .body(Body::from("Hello, World!"))
            .unwrap();

        let mut svc = RequestDecompression::new(service_fn(handle_asserts_on_body));
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
        let svc = RequestDecompression::new(svc);

        let make_service = Shared::new(svc);

        let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
        let server = Server::bind(&addr).serve(make_service);
        server.await.unwrap();
    }
}
