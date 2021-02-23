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
    use http::Response;
    use hyper::{Body, Client, Request};
    use tower::{Service, ServiceExt};

    #[allow(dead_code)]
    async fn test_something() {
        let mut client = Decompression::new(Client::new());

        let req = Request::new(Body::empty());

        let _: Response<DecompressionBody<Body>> =
            client.ready_and().await.unwrap().call(req).await.unwrap();
    }
}
