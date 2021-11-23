use axum::{
    body::Bytes,
    error_handling::HandleErrorLayer,
    extract::{ConnectInfo, Extension, MatchedPath, Path},
    http::{header, uri::Uri, Extensions, HeaderMap, HeaderValue, Request, StatusCode},
    response::IntoResponse,
    routing::get,
    BoxError, Router,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::Duration,
};
use structopt::StructOpt;
use tower::ServiceBuilder;
use tower_http::{
    request_id::{MakeRequestId, RequestId},
    trace::{
        otel::server::{ExtractClientIp, ExtractMatchedPath, OtelConfig, SetOtelParent},
        TraceLayer,
    },
    ServiceBuilderExt,
};
use tracing::Span;
use tracing_subscriber::{prelude::*, EnvFilter};
use uuid::Uuid;

/// Simple key/value store with an HTTP API
#[derive(Debug, StructOpt)]
struct Config {
    /// The port to listen on
    #[structopt(long, short = "p", default_value = "3000")]
    port: u16,

    /// Setup opentelemetry
    #[structopt(long)]
    otel: bool,
}

#[derive(Clone, Debug)]
struct State {
    db: Arc<RwLock<HashMap<String, Bytes>>>,
}

#[tokio::main]
async fn main() {
    // Parse command line arguments
    let config = Config::from_args();

    // Setup opentelemetry
    let otel_layer = config.otel.then(|| {
        opentelemetry::global::set_text_map_propagator(
            opentelemetry::sdk::propagation::TraceContextPropagator::new(),
        );
        let tracer = opentelemetry_jaeger::new_pipeline()
            .with_service_name("server")
            .install_batch(opentelemetry::runtime::Tokio)
            .unwrap();
        tracing_opentelemetry::layer().with_tracer(tracer)
    });

    // Setup tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .with(otel_layer)
        .init();

    // Run our service
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("Listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app().into_make_service_with_connect_info::<SocketAddr, _>())
        .await
        .expect("server error");
}

fn handle_errors(err: BoxError) -> (StatusCode, String) {
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

    let otel_config = AxumOtelConfig;

    // Build our middleware stack
    let middleware = ServiceBuilder::new()
        // Set `x-request-id`
        .set_x_request_id(RequestUuid)
        // Mark the `Authorization` and `Cookie` headers as sensitive so it doesn't show in logs
        .sensitive_request_headers(sensitive_headers.clone())
        // Add high level tracing/logging to all requests
        .layer(
            TraceLayer::new_for_http().opentelemetry_server(
                OtelConfig::default()
                    .extract_matched_path_with(otel_config)
                    .extract_client_ip_with(otel_config)
                    .set_otel_parent_with(otel_config),
            ),
        )
        .sensitive_response_headers(sensitive_headers)
        // Propagate `x-request-id` from requests to responses
        .propagate_x_request_id()
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
        .layer(middleware)
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

#[derive(Copy, Clone)]
struct AxumOtelConfig;

impl ExtractMatchedPath for AxumOtelConfig {
    fn extract_matched_path<'a>(&self, uri: &'a Uri, extensions: &'a Extensions) -> Cow<'a, str> {
        if let Some(matched_path) = extensions.get::<MatchedPath>() {
            matched_path.as_str().into()
        } else {
            uri.path().to_owned().into()
        }
    }
}

impl ExtractClientIp for AxumOtelConfig {
    fn extract_client_ip<'a>(
        &self,
        _headers: &'a HeaderMap,
        extensions: &'a Extensions,
    ) -> Option<Cow<'a, str>> {
        extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.to_string().into())
    }
}

impl SetOtelParent for AxumOtelConfig {
    fn set_otel_parent(&self, headers: &HeaderMap, span: &Span) {
        use opentelemetry::trace::TraceContextExt as _;
        use tracing_opentelemetry::OpenTelemetrySpanExt as _;

        struct RequestHeaderCarrier<'a>(&'a HeaderMap);

        impl<'a> opentelemetry::propagation::Extractor for RequestHeaderCarrier<'a> {
            fn get(&self, key: &str) -> Option<&str> {
                self.0.get(key).and_then(|v| v.to_str().ok())
            }

            fn keys(&self) -> Vec<&str> {
                self.0.keys().map(|header| header.as_str()).collect()
            }
        }

        let parent_context = opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.extract(&RequestHeaderCarrier(headers))
        });
        span.set_parent(parent_context);
        // If we have a remote parent span, this will be the parent's trace identifier.
        // If not, it will be the newly generated trace identifier with this request as root span.
        let trace_id = span.context().span().span_context().trace_id().to_hex();
        span.record("trace_id", &tracing::field::display(trace_id));
    }
}

#[derive(Copy, Clone)]
struct RequestUuid;

impl MakeRequestId for RequestUuid {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        let uuid = HeaderValue::from_str(&Uuid::new_v4().to_string()).unwrap();
        Some(RequestId::new(uuid))
    }
}

// See https://github.com/tokio-rs/axum/blob/main/examples/testing/src/main.rs for an example of
// how to test axum apps
