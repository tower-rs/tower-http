use super::{ResponseBody, ResponseFuture};
use http::{Request, Response};
use http_body::{Body, Limited};
use std::task::{Context, Poll};
use tower_service::Service;

/// Middleware that intercepts requests with body lengths greater than the
/// configured limit and converts them into `413 Payload Too Large` responses.
///
/// See the [module docs](crate::limit) for an example.
#[derive(Clone, Copy, Debug)]
pub struct RequestBodyLimit<S> {
    pub(crate) inner: S,
    pub(crate) limit: usize,
}

impl<S> RequestBodyLimit<S> {
    /// Create a new `RequestBodyLimit` with the given body length limit.
    pub fn new(inner: S, limit: usize) -> Self {
        Self { inner, limit }
    }

    define_inner_service_accessors!();
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for RequestBodyLimit<S>
where
    ResBody: Body,
    S: Service<Request<Limited<ReqBody>>, Response = Response<ResBody>>,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let (parts, body) = req.into_parts();
        let content_length = parts
            .headers
            .get(http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok()?.parse::<usize>().ok());

        let body_limit = match content_length {
            Some(len) if len > self.limit => return ResponseFuture::payload_too_large(),
            Some(len) => self.limit.min(len),
            None => self.limit,
        };

        let body = Limited::new(body, body_limit);
        let req = Request::from_parts(parts, body);
        let future = self.inner.call(req);

        ResponseFuture::new(future)
    }
}
