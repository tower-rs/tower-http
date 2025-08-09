use bytes::Bytes;
use clap::Parser;
use hyper::{
    body::Body as HttpBody,
    header::{self, HeaderValue},
    Response, StatusCode,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    add_extension::AddExtensionLayer,
    compression::CompressionLayer,
    sensitive_headers::SetSensitiveHeadersLayer,
    set_header::SetResponseHeaderLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};
use warp::{filters, path};
use warp::{Filter, Rejection, Reply};

/// Simple key/value store with an HTTP API
#[derive(Debug, Parser)]
struct Config {
    /// The port to listen on
    #[arg(short = 'p', long, default_value = "3000")]
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
    let config = Config::parse();

    // Create a `TcpListener`
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = TcpListener::bind(addr).await.unwrap();

    // Run our service
    serve_forever(listener).await.expect("server error");
}

// Run our service with the given `TcpListener`.
//
// We make this a separate function so we're able to call it from tests.
async fn serve_forever(listener: TcpListener) -> std::io::Result<()> {
    // Build our database for holding the key/value pairs
    let state = State {
        db: Arc::new(RwLock::new(HashMap::new())),
    };

    // Build or warp `Filter` by combining each individual filter
    let filter = error().or(get()).or(set());

    // Convert our `Filter` into a `Service`
    let warp_service = warp::service(filter);

    // Apply middleware to our service.
    let service = ServiceBuilder::new()
        // Add high level tracing/logging to all requests
        .layer(
            TraceLayer::new_for_http()
                .on_body_chunk(|chunk: &Bytes, latency: Duration, _: &tracing::Span| {
                    tracing::trace!(size_bytes = chunk.len(), latency = ?latency, "sending body chunk")
                })
                .make_span_with(DefaultMakeSpan::new().include_headers(true))
                .on_response(DefaultOnResponse::new().include_headers(true).latency_unit(LatencyUnit::Micros)),
        )
        // Set a timeout
        .timeout(Duration::from_secs(10))
        // Share the state with each handler via a request extension
        .layer(AddExtensionLayer::new(state))
        // Compress responses
        .layer(CompressionLayer::new())
        // If the response has a known size set the `Content-Length` header
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_LENGTH,
            content_length_from_response,
        ))
        // Set a `Content-Type` if there isn't one already.
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        ))
        // Mark the `Authorization` and `Cookie` headers as sensitive so it doesn't show in logs
        .layer(SetSensitiveHeadersLayer::new(vec![
            header::AUTHORIZATION,
            header::COOKIE,
        ]))
        // Build our final `Service`
        .service(warp_service);

    // Run the service using hyper
    let addr = listener.local_addr().unwrap();

    tracing::info!("Listening on {}", addr);

    let service = hyper_util::service::TowerToHyperService::new(service);

    loop {
        let (tcp, _) = listener.accept().await?;
        let io = hyper_util::rt::TokioIo::new(tcp);
        let service = service.clone();

        tokio::spawn(async move {
            if let Err(err) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                tracing::error!(?err, "Error occurred on serving connection");
            }
        });
    }
}

// Filter for looking up a key
pub fn get() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(path!(String))
        .and(filters::ext::get::<State>())
        .map(|path: String, state: State| {
            let state = state.db.read().unwrap();

            if let Some(value) = state.get(&path).cloned() {
                Response::new(value)
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Bytes::new())
                    .unwrap()
            }
        })
}

// Filter for setting a key/value pair
pub fn set() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::post()
        .and(path!(String))
        .and(filters::ext::get::<State>())
        .and(filters::body::bytes())
        .map(|path: String, state: State, value: Bytes| {
            let mut state = state.db.write().unwrap();

            state.insert(path, value);

            Response::new(Bytes::new())
        })
}

// Test filter that always fails
pub fn error() -> impl Filter<Extract = (&'static str,), Error = Rejection> + Clone {
    warp::get()
        .and(path!("debug" / "error"))
        .and_then(|| async move { Err(warp::reject::custom(InternalError)) })
}

#[derive(Debug)]
struct InternalError;

impl warp::reject::Reject for InternalError {}

fn content_length_from_response<B>(response: &Response<B>) -> Option<HeaderValue>
where
    B: HttpBody,
{
    response
        .body()
        .size_hint()
        .exact()
        .map(|size| HeaderValue::from_str(&size.to_string()).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn get_and_set_value() {
        let addr = run_in_background().await;

        let client = reqwest::Client::builder().gzip(true).build().unwrap();

        let response = client
            .get(&format!("http://{}/foo", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let response = client
            .post(&format!("http://{}/foo", addr))
            .body("Hello, World!")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = client
            .get(&format!("http://{}/foo", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.unwrap();
        assert_eq!(body, "Hello, World!");
    }

    // Run our service in a background task.
    async fn run_in_background() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Could not bind ephemeral socket");
        let addr = listener.local_addr().unwrap();

        // just for debugging
        eprintln!("Listening on {}", addr);

        tokio::spawn(async move {
            serve_forever(listener).await.unwrap();
        });

        addr
    }
}
