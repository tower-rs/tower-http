use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{Either, Full};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Service, ServiceBuilder, ServiceExt};

type BoxBody = http_body_util::Full<Bytes>;
type ResponseBody<B> = Either<B, BoxBody>;

#[derive(Clone)]
pub struct RequireHeader<S> {
    inner: S,
    header_name: &'static str,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for RequireHeader<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Send,
    ReqBody: Send + 'static,
    ResBody: Send + 'static,
{
    type Response = Response<ResponseBody<ResBody>>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if !req.headers().contains_key(self.header_name) {
            let body = Full::from("Missing required header");
            let res = Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Either::Right(body))
                .unwrap();

            return Box::pin(std::future::ready(Ok(res)));
        }

        let mut inner = self.inner.clone();
        Box::pin(async move {
            let res = inner.call(req).await?;
            Ok(res.map(Either::Left))
        })
    }
}

impl<S> RequireHeader<S> {
    pub fn new(inner: S, header_name: &'static str) -> Self {
        Self { inner, header_name }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let inner_service = tower::service_fn(|_req: Request<Full<Bytes>>| async {
        Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::from("Hello, World!"))))
    });

    let mut service = ServiceBuilder::new()
        .layer_fn(|inner| RequireHeader::new(inner, "x-api-key"))
        .service(inner_service);

    let req_bad = Request::builder().body(Full::<Bytes>::default()).unwrap();
    let res_bad = service.ready().await?.call(req_bad).await?;
    println!("Bad: {}", res_bad.status());

    let req_good = Request::builder()
        .header("x-api-key", "secret")
        .body(Full::<Bytes>::default())
        .unwrap();
    let res_good = service.ready().await?.call(req_good).await?;
    println!("Good: {}", res_good.status());

    Ok(())
}
