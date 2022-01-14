use axum::{
    body::Bytes,
    error_handling::HandleErrorLayer,
    extract::{Extension, Path},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::get,
    BoxError, Router,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::Duration,
};
use structopt::StructOpt;
use tower::ServiceBuilder;
use tower_http::{
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
    LatencyUnit, ServiceBuilderExt,
};

/// Simple key/value store with an HTTP API
#[derive(Debug, StructOpt)]
struct Config {
    /// The port to listen on
    #[structopt(long, short = "p", default_value = "3000")]
    port: u16,
}

#[derive(Clone, Debug)]
struct State {
    db: Arc<RwLock<HashMap<String, Bytes>>>,
}

#[tokio::main]
async fn main() {
    // Setup tracing
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let config = Config::from_args();

    // Run our service
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("Listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app().into_make_service())
        .await
        .expect("server error");
}

async fn handle_errors(err: BoxError) -> impl IntoResponse {
    if err.is::<tower::timeout::error::Elapsed>() {
        (
            StatusCode::REQUEST_TIMEOUT,
            "Request took too long".to_string(),
        )
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Unhandled internal error: {}", err),
        )
    }
}

fn app() -> Router {
    // Build our database for holding the key/value pairs
    let state = State {
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
        // Handle errors
        .layer(HandleErrorLayer::new(handle_errors))
        // Set a timeout
        .timeout(Duration::from_secs(10))
        // Share the state with each handler via a request extension
        .add_extension(state)
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
        .layer(middleware.into_inner())
}

async fn get_key(path: Path<String>, state: Extension<State>) -> impl IntoResponse {
    let state = state.db.read().unwrap();

    if let Some(value) = state.get(&*path).cloned() {
        Ok(value)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn set_key(Path(path): Path<String>, state: Extension<State>, value: Bytes) {
    let mut state = state.db.write().unwrap();
    state.insert(path, value);
}

// See https://github.com/tokio-rs/axum/blob/main/examples/testing/src/main.rs for an example of
// how to test axum apps
