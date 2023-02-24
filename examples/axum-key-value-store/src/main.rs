use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use clap::Parser;
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, RwLock},
    time::Duration,
};
use tower::ServiceBuilder;
use tower_http::{
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
    LatencyUnit, ServiceBuilderExt,
};

/// Simple key/value store with an HTTP API
#[derive(Debug, Parser)]
struct Config {
    /// The port to listen on
    #[clap(short = 'p', long, default_value = "3000")]
    port: u16,
}

#[derive(Clone, Debug)]
struct AppState {
    db: Arc<RwLock<HashMap<String, Bytes>>>,
}

#[tokio::main]
async fn main() {
    // Setup tracing
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let config = Config::parse();

    // Run our service
    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, config.port));
    tracing::info!("Listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app().into_make_service())
        .await
        .expect("server error");
}

fn app() -> Router {
    // Build our database for holding the key/value pairs
    let state = AppState {
        db: Arc::new(RwLock::new(HashMap::new())),
    };

    let sensitive_headers: Arc<[_]> = vec![header::AUTHORIZATION, header::COOKIE].into();

    // Build our middleware stack
    let middleware = ServiceBuilder::new()
        // Mark the `Authorization` and `Cookie` headers as sensitive so it doesn't show in logs
        .sensitive_request_headers(sensitive_headers.clone())
        // Add high level tracing/logging to all requests
        .layer(
            TraceLayer::new_for_http()
                .on_body_chunk(|chunk: &Bytes, latency: Duration, _: &tracing::Span| {
                    tracing::trace!(size_bytes = chunk.len(), latency = ?latency, "sending body chunk")
                })
                .make_span_with(DefaultMakeSpan::new().include_headers(true))
                .on_response(DefaultOnResponse::new().include_headers(true).latency_unit(LatencyUnit::Micros)),
        )
        .sensitive_response_headers(sensitive_headers)
        // Set a timeout
        .layer(TimeoutLayer::new(Duration::from_secs(10)))
        // Box the response body so it implements `Default` which is required by axum
        .map_response_body(axum::body::boxed)
        // Compress responses
        .compression()
        // Set a `Content-Type` if there isn't one already.
        .insert_response_header_if_not_present(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );

    // Build route service
    Router::new()
        .route("/:key", get(get_key).post(set_key))
        .layer(middleware)
        .with_state(state)
}

async fn get_key(path: Path<String>, state: State<AppState>) -> impl IntoResponse {
    let state = state.db.read().unwrap();

    if let Some(value) = state.get(&*path).cloned() {
        Ok(value)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn set_key(Path(path): Path<String>, state: State<AppState>, value: Bytes) {
    let mut state = state.db.write().unwrap();
    state.insert(path, value);
}

// See https://github.com/tokio-rs/axum/blob/main/examples/testing/src/main.rs for an example of
// how to test axum apps
