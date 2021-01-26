use crate::common::*;
use futures::future::{ready, Ready};
use hyper::Body;

#[cfg(all(
    feature = "add-extension",
    feature = "compression",
    feature = "metrics",
    feature = "propagate-header",
    feature = "sensitive-header",
    feature = "set-response-header",
    feature = "trace",
    feature = "util",
    feature = "wrap-in-span",
))]
mod huge_stacks_do_compile;

#[derive(Copy, Clone)]
struct EchoService;

impl Service<Request<Body>> for EchoService {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        ready(Ok(Response::new(req.into_body())))
    }
}
