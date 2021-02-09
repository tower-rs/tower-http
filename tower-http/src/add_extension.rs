use http::Request;
use std::task::{Context, Poll};
use tower_layer::Layer;
use tower_service::Service;

#[derive(Clone, Copy, Debug)]
pub struct AddExtensionLayer<T> {
    value: T,
}

impl<T> AddExtensionLayer<T> {
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

#[derive(Clone, Copy, Debug)]
pub struct AddExtension<S, T> {
    inner: S,
    value: T,
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
