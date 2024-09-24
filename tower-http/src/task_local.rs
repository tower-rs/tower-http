//! Middleware to set tokio task-local data.

use std::future::Future;

use http::{Request, Response};
use tokio::task::{futures::TaskLocalFuture, LocalKey};
use tower_layer::Layer;
use tower_service::Service;

#[derive(Debug, Clone, Copy)]
pub struct SetTaskLocalLayer<T: 'static> {
    key: &'static LocalKey<T>,
    value: T,
}

impl<T> SetTaskLocalLayer<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(key: &'static LocalKey<T>, value: T) -> Self {
        SetTaskLocalLayer { key, value }
    }
}

impl<S, T> Layer<S> for SetTaskLocalLayer<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Service = SetTaskLocal<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        SetTaskLocal::new(inner, self.key, self.value.clone())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SetTaskLocal<S, T: 'static> {
    inner: S,
    key: &'static LocalKey<T>,
    value: T,
}

impl<S, T> SetTaskLocal<S, T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(inner: S, key: &'static LocalKey<T>, value: T) -> Self {
        Self { inner, key, value }
    }
}

impl<S, T, ReqBody, ResBody> Service<Request<ReqBody>> for SetTaskLocal<S, T>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    T: Clone + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<T, S>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.key.scope(self.value.clone(), self.inner.call(req))
    }
}

opaque_future! {
    /// Response future of [`SetTaskLocal`].
    pub type ResponseFuture<T, ReqBody, S> = TaskLocalFuture<T, S::Future>
    where
        S: Service<Request<ReqBody>>;
}
