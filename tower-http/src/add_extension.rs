//! Middleware that clones a value into each request's [extensions].
//!
//! [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html

use http::Request;
use std::task::{Context, Poll};
use tower_layer::Layer;
use tower_service::Service;

/// [`Layer`] for adding some shareable value to [request extensions].
///
/// [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
#[derive(Clone, Copy, Debug)]
pub struct AddExtensionLayer<T> {
    value: T,
}

impl<T> AddExtensionLayer<T> {
    /// Create a new [`AddExtensionLayer`].
    pub fn new(value: T) -> Self {
        AddExtensionLayer { value }
    }
}

impl<S, T> Layer<S> for AddExtensionLayer<T>
where
    T: Clone,
{
    type Service = AddExtension<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        AddExtension {
            inner,
            value: self.value.clone(),
        }
    }
}

/// Middleware for adding some shareable value to [request extensions].
///
/// [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
#[derive(Clone, Copy, Debug)]
pub struct AddExtension<S, T> {
    inner: S,
    value: T,
}

impl<S, T> AddExtension<S, T> {
    /// Create a new [`AddExtension`].
    pub fn new(inner: S, value: T) -> Self {
        Self { inner, value }
    }

    /// Gets a reference to the underlying service.
    pub fn get_ref(&self) -> &S {
        &self.inner
    }

    /// Gets a mutable reference to the underlying service.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Gets a mutable reference to the underlying service.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<ResBody, S, T> Service<Request<ResBody>> for AddExtension<S, T>
where
    S: Service<Request<ResBody>>,
    T: Clone + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ResBody>) -> Self::Future {
        req.extensions_mut().insert(self.value.clone());
        self.inner.call(req)
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use http::Response;
    use hyper::Body;
    use std::{convert::Infallible, sync::Arc};
    use tower::{service_fn, ServiceBuilder, ServiceExt};

    struct State(i32);

    #[tokio::test]
    async fn basic() {
        let state = Arc::new(State(1));

        let svc = ServiceBuilder::new()
            .layer(AddExtensionLayer::new(state))
            .service(service_fn(|req: Request<Body>| async move {
                let state = req.extensions().get::<Arc<State>>().unwrap();
                Ok::<_, Infallible>(Response::new(state.0))
            }));

        let res = svc
            .oneshot(Request::new(Body::empty()))
            .await
            .unwrap()
            .into_body();

        assert_eq!(1, res);
    }
}
