use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{Either, Full};
use pin_project_lite::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};
use tower::Service;

use http_body_util::BodyExt;
use std::error::Error;
use tower::ServiceBuilder;
use tower::ServiceExt;

// think of Either as an enum that implements Body if both arms implement Body
// this allows us to combine multple bodies
type ResponseBody<B> = Either<B, Full<Bytes>>;

// some helper to go along with it
fn map_resp<B>(resp: Response<B>) -> Response<ResponseBody<B>> {
    resp.map(|body| Either::Left(body))
}

fn new_err_resp<B>(status: StatusCode, body: &'static str) -> Response<ResponseBody<B>> {
    Response::builder()
        .status(status)
        .body(Either::Right(Full::from(body)))
        .unwrap()
}

#[derive(Clone)]
pub struct RequireHeader<S> {
    inner: S,
    header_name: &'static str,
}

impl<S> RequireHeader<S> {
    pub fn new(inner: S, header_name: &'static str) -> Self {
        Self { inner, header_name }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for RequireHeader<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = RequireHeaderFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if !req.headers().contains_key(self.header_name) {
            return RequireHeaderFuture::HeaderNotFound;
        }
        RequireHeaderFuture::Ok {
            fut: self.inner.call(req),
        }
    }
}

pin_project! {
    #[project = EnumProj]
    pub enum RequireHeaderFuture<F> {
        Ok{ #[pin] fut: F },
        HeaderNotFound,
    }
}

impl<F, E, ResBody> Future for RequireHeaderFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResponseBody<ResBody>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EnumProj::Ok { fut } => {
                let res = ready!(fut.poll(cx));

                // we use our helper to unify the response body types
                // its also possible to return a custom error response here if the inner service failed
                Poll::Ready(res.map(|resp| map_resp(resp)))
            }
            EnumProj::HeaderNotFound => {
                Poll::Ready(Ok(new_err_resp(StatusCode::BAD_REQUEST, "missing header")))
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let inner_service = tower::service_fn(|_req: Request<Full<Bytes>>| async {
        Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::from("Hello, World!"))))
    });

    let mut service = ServiceBuilder::new()
        .layer_fn(|inner| RequireHeader::new(inner, "x-api-key"))
        .service(inner_service);

    let req_bad = Request::builder().body(Full::<Bytes>::default()).unwrap();
    let res_bad = service.ready().await?.call(req_bad).await?;
    assert_eq!(res_bad.status(), StatusCode::BAD_REQUEST);

    let body = res_bad.into_body().collect().await.unwrap();
    assert_eq!(body.to_bytes(), "missing header");

    let req_good = Request::builder()
        .header("x-api-key", "secret")
        .body(Full::<Bytes>::default())
        .unwrap();
    let res_good = service.ready().await?.call(req_good).await?;
    assert_eq!(res_good.status(), StatusCode::OK);

    Ok(())
}
