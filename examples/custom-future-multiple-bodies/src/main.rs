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
use tower::ServiceBuilder;
use tower::ServiceExt;

// think of Either as an enum that implements Body if both arms implement Body
// this allows us to combine multple bodies
type ResponseBody<B> = Either<B, Full<Bytes>>;

// helper to construct an error response
fn rejection<B>(status: StatusCode, body: &'static str) -> Response<ResponseBody<B>> {
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
    // Either requires both bodies to use the same underlying buffer type
    ResBody: http_body::Body<Data = Bytes>,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = RequireHeaderFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if !req.headers().contains_key(self.header_name) {
            return RequireHeaderFuture::MissingHeader;
        }
        RequireHeaderFuture::Future {
            future: self.inner.call(req),
        }
    }
}

pin_project! {
    #[project = ResFutProj]
    pub enum RequireHeaderFuture<F> {
        Future{ #[pin] future: F },
        MissingHeader,
    }
}

impl<F, E, ResBody> Future for RequireHeaderFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResponseBody<ResBody>>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = match self.project() {
            // `?` propagates an inner-service error. to turn it into a response
            // instead, replace `?` with
            //     .unwrap_or_else(|_| rejection(StatusCode::BAD_GATEWAY, "..."))
            // to convert every error, or `.or_else(..)` to convert only some and
            // keep propagating the rest.
            ResFutProj::Future { future } => ready!(future.poll(cx))?.map(Either::Left),
            ResFutProj::MissingHeader => rejection(StatusCode::BAD_REQUEST, "missing header"),
        };

        Poll::Ready(Ok(res))
    }
}

#[tokio::main]
async fn main() -> Result<(), tower::BoxError> {
    let inner_service = tower::service_fn(|_req: Request<Full<Bytes>>| async {
        Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::from("Hello, World!"))))
    });

    let required_header = "x-api-key";

    let mut service = ServiceBuilder::new()
        .layer_fn(|inner| RequireHeader::new(inner, required_header))
        .service(inner_service);

    println!(
        "calling the service without the required {} header",
        required_header
    );

    let req_bad = Request::builder().body(Full::<Bytes>::default())?;
    let res_bad = service.ready().await?.call(req_bad).await?;

    let res_code = res_bad.status();
    let body = res_bad.into_body().collect().await?.to_bytes();

    println!(
        "response: {}, {}",
        res_code.as_str(),
        std::str::from_utf8(&body)?
    );

    println!(
        "calling the service with the required {} header",
        required_header
    );

    let req_good = Request::builder()
        .header("x-api-key", "secret")
        .body(Full::<Bytes>::default())?;

    let res_good = service.ready().await?.call(req_good).await?;

    let res_code = res_good.status();

    println!("response: {}", res_code.as_str());

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn reject_missing_header() {
        let inner_service = tower::service_fn(|_req: Request<Full<Bytes>>| async {
            Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::from(
                "Hello, World!",
            ))))
        });

        let mut service = ServiceBuilder::new()
            .layer_fn(|inner| RequireHeader::new(inner, "x-api-key"))
            .service(inner_service);

        let req_bad = Request::builder().body(Full::<Bytes>::default()).unwrap();
        let res_bad = service.ready().await.unwrap().call(req_bad).await.unwrap();
        assert_eq!(res_bad.status(), StatusCode::BAD_REQUEST);

        let body = res_bad.into_body().collect().await.unwrap();
        assert_eq!(body.to_bytes(), "missing header");
    }

    #[tokio::test]
    async fn accept_correct_header() {
        let inner_service = tower::service_fn(|_req: Request<Full<Bytes>>| async {
            Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::from(
                "Hello, World!",
            ))))
        });

        let mut service = ServiceBuilder::new()
            .layer_fn(|inner| RequireHeader::new(inner, "x-api-key"))
            .service(inner_service);

        let req_good = Request::builder()
            .header("x-api-key", "secret")
            .body(Full::<Bytes>::default())
            .unwrap();

        let res_good = service.ready().await.unwrap().call(req_good).await.unwrap();
        assert_eq!(res_good.status(), StatusCode::OK);
    }
}
