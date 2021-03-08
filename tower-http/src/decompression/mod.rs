//! Middleware that decompresses response bodies.

mod body;
mod future;
mod layer;
mod service;

pub use self::{
    body::DecompressionBody, future::ResponseFuture, layer::DecompressionLayer,
    service::Decompression,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::Compression;
    use bytes::BytesMut;
    use http::Response;
    use http_body::Body as _;
    use hyper::{Body, Client, Error, Request};
    use tower::{service_fn, Service, ServiceExt};

    #[tokio::test]
    async fn works() {
        let mut client = Decompression::new(Compression::new(service_fn(handle)));

        let req = Request::builder()
            .header("accept-encoding", "gzip")
            .body(Body::empty())
            .unwrap();
        let res = client.ready().await.unwrap().call(req).await.unwrap();

        // read the body, it will be decompressed automatically
        let mut body = res.into_body();
        let mut data = BytesMut::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.unwrap();
            data.extend_from_slice(&chunk[..]);
        }
        let decompressed_data = String::from_utf8(data.freeze().to_vec()).unwrap();

        assert_eq!(decompressed_data, "Hello, World!");
    }

    async fn handle(_req: Request<Body>) -> Result<Response<Body>, Error> {
        Ok(Response::new(Body::from("Hello, World!")))
    }

    #[allow(dead_code)]
    async fn is_compatible_with_hyper() {
        let mut client = Decompression::new(Client::new());

        let req = Request::new(Body::empty());

        let _: Response<DecompressionBody<Body>> =
            client.ready().await.unwrap().call(req).await.unwrap();
    }
}
