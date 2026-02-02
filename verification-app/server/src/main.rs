use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode},
    response::{Response, IntoResponse},
    routing::{get, post},
    Router,
};
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{CorsLayer, Any},
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveHeadersLayer,
    timeout::TimeoutLayer,
    trace::TraceLayer,
    validate_request::ValidateRequestHeaderLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = Router::new()
        .route("/", get(|| async { "Tower HTTP Verification Server" }))
        // Compression (Isolated)
        .merge(
            Router::new()
                .route("/compression/large", get(large_response))
                .layer(CompressionLayer::new())
        )
        // Timeout (Isolated)
        .merge(
            Router::new()
                .route("/timeout/sleep", get(sleep_response))
                .layer(TimeoutLayer::new(Duration::from_secs(2)))
        )
        // Auth (Isolated)
        .merge(
             Router::new()
                .route("/auth/protected", get(|| async { "You are authorized!" }))
                .layer(ValidateRequestHeaderLayer::bearer("secret-token"))
        )
        // Limit (Isolated)
        .merge(
            Router::new()
                .route("/limit/upload", post(|| async { "Upload received" }))
                .layer(RequestBodyLimitLayer::new(1024))
        )
        // Request ID (Isolated)
        .merge(
            Router::new()
                .route("/request-id", get(|headers: axum::http::HeaderMap| async move {
                    let id = headers.get("x-request-id").and_then(|h| h.to_str().ok()).unwrap_or("none");
                    format!("Request ID: {}", id)
                }))
                .layer(
                    ServiceBuilder::new()
                        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                        .layer(PropagateRequestIdLayer::x_request_id())
                )
        )
        // Global Middleware
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(Any)
                        .allow_headers(Any)
                        .expose_headers(Any),
                )
                .layer(SetSensitiveHeadersLayer::new(std::iter::once(
                    axum::http::header::AUTHORIZATION,
                )))
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn large_response() -> impl IntoResponse {
    let data = "A".repeat(100_000);
    (
        [(axum::http::header::CONTENT_TYPE, "text/plain")],
        data,
    )
}

async fn sleep_response() -> &'static str {
    tokio::time::sleep(Duration::from_secs(5)).await;
    "Slept for 5 seconds (should have timed out)"
}
