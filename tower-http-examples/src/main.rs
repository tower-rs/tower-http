#![allow(dead_code, unused_imports)]

use hyper::{
    header::{self, HeaderName, HeaderValue},
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server,
};
use std::{convert::Infallible, sync::Arc};
use tower::ServiceBuilder;
use tower_http::{
    add_extension::AddExtensionLayer,
    compression::CompressionLayer,
    metrics::MetricsLayer,
    propagate_header::PropagateHeaderLayer,
    sensitive_header::SensitiveHeaderLayer,
    set_response_header::SetResponseHeaderLayer,
    trace::TraceLayer,
    util::{DebugEnterLeaveLayer, LayerExt},
    wrap_in_span::WrapInSpanLayer,
    LatencyUnit,
};
use uuid::Uuid;

const REQUEST_ID: &str = "x-request-id";

struct State {
    thing: i32,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let state = Arc::new(State { thing: 1337 });

    let svc = ServiceBuilder::new()
        .layer(
            WrapInSpanLayer::new(tracing::debug_span!("example-service"))
                .debug_enter_leave("wrap-in-span"),
        )
        .layer(MetricsLayer::new().debug_enter_leave("metrics"))
        .layer(
            TraceLayer::new()
                .record_headers(false)
                .latency_unit(LatencyUnit::Nanos)
                .record_full_uri(false)
                .debug_enter_leave("tracing"),
        )
        .layer(
            SensitiveHeaderLayer::new(header::AUTHORIZATION).debug_enter_leave("sensitive-header"),
        )
        .layer(
            SetResponseHeaderLayer::new(|_res: &Response<Body>| {
                let header = HeaderName::from_static(REQUEST_ID);
                let value = HeaderValue::from_str(&Uuid::new_v4().to_string()).unwrap();
                (header, value)
            })
            .override_existing(false)
            .debug_enter_leave("set-response-header"),
        )
        .layer(
            PropagateHeaderLayer::new(HeaderName::from_static(REQUEST_ID))
                .debug_enter_leave("request-id"),
        )
        .layer(CompressionLayer::new().debug_enter_leave("compression"))
        .layer(AddExtensionLayer::new(state).debug_enter_leave("state"))
        .service(service_fn(handle));

    let make_svc = make_service_fn(|_| {
        let svc = svc.clone();
        async move { Ok::<_, Infallible>(svc) }
    });

    let addr = ([127, 0, 0, 1], 3000).into();
    let server = Server::bind(&addr).serve(make_svc);
    if let Err(err) = server.await {
        panic!("server error: {}", err);
    }
}

async fn handle(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    tracing::debug!("processing request");

    let mut res = Response::new(Body::from("<h1>Hello, World!</h1>"));

    res.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));

    Ok(res)
}
