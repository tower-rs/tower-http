use super::{ResponseBody, ResponseFuture};
use http::{Request, Response};
use http_body::{Body, Limited};
use std::error::Error as StdError;
use std::task::{Context, Poll};
use std::{any, fmt};
use tower_service::Service;

/// Middleware that intercepts requests with body lengths greater than the
/// configured limit and converts them into `413 Payload Too Large` responses.
///
/// See the [module docs](crate::limit) for an example.
#[derive(Clone, Copy)]
pub struct RequestBodyLimit<S> {
    pub(crate) inner: S,
    pub(crate) limit: usize,
}

impl<S> RequestBodyLimit<S> {
    define_inner_service_accessors!();

    /// Create a new `RequestBodyLimit` with the given body length limit.
    pub fn new(inner: S, limit: usize) -> Self {
        Self { inner, limit }
    }
}

impl<S> fmt::Debug for RequestBodyLimit<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestBodyLimit")
            .field("service", &format_args!("{}", any::type_name::<S>()))
            .field("limit", &self.limit)
            .finish()
    }
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for RequestBodyLimit<S>
where
    ResBody: Body,
    S: Service<Request<Limited<ReqBody>>, Response = Response<ResBody>>,
    S::Error: StdError + 'static,
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

        match content_length {
            Some(len) if len > self.limit => ResponseFuture::payload_too_large(),
            _ => {
                let body = Limited::new(body, self.limit);
                let req = Request::from_parts(parts, body);
                let future = self.inner.call(req);

                ResponseFuture::new(future)
            }
        }
    }
}
