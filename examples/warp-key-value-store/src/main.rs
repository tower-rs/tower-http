use bytes::Bytes;
use hyper::{
    body::HttpBody,
    header::{self, HeaderValue},
    Body, Request, Response, Server, StatusCode,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use structopt::StructOpt;
use tower::{make::Shared, ServiceBuilder};
use tower_http::{
    add_extension::AddExtensionLayer, compression::CompressionLayer,
    sensitive_header::SetSensitiveHeaderLayer, set_response_header::SetResponseHeaderLayer,
};
use warp::{filters, path};
use warp::{Filter, Rejection, Reply};

/// Simple key/value store with an HTTP API
#[derive(Debug, StructOpt)]
struct Config {
    /// The port to listen on
    #[structopt(long, short = "p", default_value = "3000")]
    port: u16,
}

type Database = Arc<RwLock<HashMap<String, Bytes>>>;

#[tokio::main]
async fn main() {
    // Setup tracing
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let config = Config::from_args();

    // Create a `TcpListener`
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = TcpListener::bind(addr).unwrap();

    // Run our service
    serve_forever(listener).await.expect("server error");
}

// Run our service with the given `TcpListener`.
//
// We make this a separate function so we're able to call it from tests.
async fn serve_forever(listener: TcpListener) -> Result<(), hyper::Error> {
    // Build our database for holding the key/value pairs
    let db: Database = Arc::new(RwLock::new(HashMap::new()));

    // Build or warp `Filter` by combining each individual filter
    let filter = get().or(set());

    // Convert our `Filter` into a `Service`
    let warp_service = warp::service(filter);

    // Apply middlewares to our service.
    let service = ServiceBuilder::new()
        // Set a timeout
        .timeout(Duration::from_secs(10))
        // Share the database with each handler via a request extension
        .layer(AddExtensionLayer::new(db))
        // Compress responses
        .layer(CompressionLayer::new())
        // If the response has a known size set the `Content-Length` header
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_LENGTH,
            content_length_from_response,
        ))
        // Set a `Content-Type` if there isn't one already.
        .layer(SetResponseHeaderLayer::<_, Request<Body>>::if_not_present(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        ))
        // Mark the `Authorization` header as sensitive so it doesn't show in logs
        .layer(SetSensitiveHeaderLayer::new(header::AUTHORIZATION))
        // Build our final `Service`
        .service(warp_service);

    // Run the service using hyper
    let addr = listener.local_addr().unwrap();

    tracing::info!("Listening on {}", addr);

    Server::from_tcp(listener)
        .unwrap()
        .serve(Shared::new(service))
        .await?;

    Ok(())
}

// Filter for looking up a key
pub fn get() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::get()
        .and(path!(String))
        .and(filters::ext::get::<Database>())
        .map(|path: String, db: Database| {
            let state = db.read().unwrap();

            if let Some(value) = state.get(&path).cloned() {
                Response::new(Body::from(value))
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap()
            }
        })
}

// Filter for setting a key/value pair
pub fn set() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::post()
        .and(path!(String))
        .and(filters::ext::get::<Database>())
        .and(filters::body::bytes())
        .map(|path: String, db: Database, value: Bytes| {
            let mut state = db.write().unwrap();

            state.insert(path, value);

            Response::new(Body::empty())
        })
}

fn content_length_from_response<B>(response: &Response<B>) -> Option<HeaderValue>
where
    B: HttpBody,
{
    if let Some(size) = response.body().size_hint().exact() {
        Some(HeaderValue::from_str(&size.to_string()).unwrap())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[tokio::test]
    async fn get_and_set_value() {
        let addr = run_in_background();

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
    fn run_in_background() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Could not bind ephemeral socket");
        let addr = listener.local_addr().unwrap();

        // just for debugging
        eprintln!("Listening on {}", addr);

        tokio::spawn(async move {
            serve_forever(listener).await.unwrap();
        });

        addr
    }
}
