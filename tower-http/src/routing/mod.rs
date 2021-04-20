#![allow(warnings)]

use futures_util::{future::Either, ready};
use http::{Request, Response};
use std::future::Future;
use std::task::{Context, Poll};
use tower_service::Service;

mod match_request;

pub use self::match_request::{accept_all, exact_path, MatchRequest};

#[derive(Clone)]
pub struct Router<S> {
    service: S,
}

// TODO(david): impl Debug for Router<S>

impl<S> Router<S> {
    pub fn new(bottom_service: S) -> Self {
        Self {
            service: bottom_service,
        }
    }

    pub fn add_service<M, H, B>(self, match_request: M, handler: H) -> Router<Route<M, H, S>>
    where
        M: MatchRequest<B>,
        H: Service<(M::Output, Request<B>)>,
        S: Service<Request<B>>,
    {
        Router {
            service: Route {
                match_request,
                handler,
                fallback: self.service,
                handler_ready: false,
                fallback_ready: false,
            },
        }
    }

    pub fn add_service_ignore_match_output<M, H, B>(
        self,
        match_request: M,
        handler: H,
    ) -> Router<Route<M, IgnoreMatchOutput<H>, S>>
    where
        M: MatchRequest<B>,
        H: Service<Request<B>>,
        S: Service<Request<B>>,
    {
        self.add_service(match_request, IgnoreMatchOutput(handler))
    }
}

pub struct Route<MatchRequest, Handler, Fallback> {
    match_request: MatchRequest,
    handler: Handler,
    fallback: Fallback,
    handler_ready: bool,
    fallback_ready: bool,
}

impl<ReqBody, MatchRequestT, HandlerT, FallbackT> Service<Request<ReqBody>>
    for Route<MatchRequestT, HandlerT, FallbackT>
where
    MatchRequestT: MatchRequest<ReqBody>,
    HandlerT: Service<(MatchRequestT::Output, Request<ReqBody>)>,
    FallbackT: Service<Request<ReqBody>, Response = HandlerT::Response, Error = HandlerT::Error>,
{
    type Response = HandlerT::Response;
    type Error = HandlerT::Error;
    type Future = ResponseFuture<HandlerT::Future, FallbackT::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        loop {
            if !self.handler_ready {
                ready!(self.handler.poll_ready(cx))?;
                self.handler_ready = true;
            }

            if !self.fallback_ready {
                ready!(self.fallback.poll_ready(cx))?;
                self.fallback_ready = true;
            }

            if self.handler_ready && self.fallback_ready {
                return Poll::Ready(Ok(()));
            }
        }
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let future = if let Some(match_output) = self.match_request.match_request(&req) {
            self.handler_ready = false;
            Either::Left(self.handler.call((match_output, req)))
        } else {
            self.fallback_ready = false;
            Either::Right(self.fallback.call(req))
        };

        ResponseFuture(future)
    }
}

opaque_future! {
    pub type ResponseFuture<A, B> = Either<A, B>;
}

impl<R, S> Service<R> for Router<S>
where
    S: Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    #[inline]
    fn call(&mut self, req: R) -> Self::Future {
        self.service.call(req)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct IgnoreMatchOutput<S>(S);

impl<T, R, S> Service<(T, R)> for IgnoreMatchOutput<S>
where
    S: Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    #[inline]
    fn call(&mut self, (_, req): (T, R)) -> Self::Future {
        self.0.call(req)
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use hyper::Body;
    use std::convert::Infallible;
    use tower::{service_fn, ServiceExt};

    #[tokio::test]
    async fn basic() {
        let mut router = Router::new(service_fn(bottom))
            .add_service(exact_path("/"), service_fn(root))
            .add_service_ignore_match_output(exact_path("/about"), service_fn(about));

        let res = router
            .ready()
            .await
            .unwrap()
            .call(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), 200);

        let res = router
            .ready()
            .await
            .unwrap()
            .call(
                Request::builder()
                    .uri("/about")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), 200);

        let res = router
            .ready()
            .await
            .unwrap()
            .call(
                Request::builder()
                    .uri("/foobar")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), 404);
    }

    async fn bottom(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
        let res = Response::builder().status(404).body(Body::empty()).unwrap();
        Ok(res)
    }

    async fn root((_, req): ((), Request<Body>)) -> Result<Response<Body>, Infallible> {
        assert_eq!(req.uri().path(), "/");
        let res = Response::builder().status(200).body(Body::empty()).unwrap();
        Ok(res)
    }

    async fn about(req: Request<Body>) -> Result<Response<Body>, Infallible> {
        assert_eq!(req.uri().path(), "/about");
        let res = Response::builder().status(200).body(Body::empty()).unwrap();
        Ok(res)
    }
}
