use bytes::Bytes;
use clap::Parser;
use futures_util::StreamExt;
use http::{header, HeaderValue};
use http_body::Body as HttpBody;
use proto::{
    key_value_store_client::KeyValueStoreClient, key_value_store_server, GetReply, GetRequest,
    SetReply, SetRequest, SubscribeReply, SubscribeRequest,
};
use std::{
    collections::HashMap,
    iter::once,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::TcpListener,
    sync::broadcast::{self, Sender},
};
use tokio_stream::{
    wrappers::{BroadcastStream, TcpListenerStream},
    Stream,
};
use tonic::{async_trait, body::Body, transport::Channel, Code, Request, Response, Status};
use tower::{BoxError, Service, ServiceBuilder};
use tower_http::{
    classify::{GrpcCode, GrpcErrorsAsFailures, SharedClassifier},
    compression::CompressionLayer,
    decompression::DecompressionLayer,
    sensitive_headers::SetSensitiveHeadersLayer,
    set_header::SetRequestHeaderLayer,
    trace::{DefaultMakeSpan, TraceLayer},
};

mod proto {
    tonic::include_proto!("key_value_store");
}

/// Simple key/value store with an HTTP API
#[derive(Debug, Parser)]
struct Config {
    /// The port to listen on
    #[arg(short = 'p', long, default_value = "3000")]
    port: u16,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    /// Run the gRPC server
    Server,
    /// Get the value at some key
    Get {
        #[arg(short = 'k', long)]
        key: String,
    },
    /// Set a value at some key.
    ///
    /// The value will be read from stdin.
    Set {
        #[arg(short = 'k', long)]
        key: String,
    },
    /// Subscribe to a stream of inserted keys
    Subscribe,
}

#[tokio::main]
async fn main() {
    // Setup tracing
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let config = Config::parse();

    // The server address
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

    match config.command {
        Command::Server => {
            // Create a `TcpListener`
            let listener = TcpListener::bind(addr).await.unwrap();

            // Run our service
            serve_forever(listener).await.expect("server error");
        }
        Command::Get { key } => {
            // Create a client for our server
            let mut client = make_client(addr).await.unwrap();

            // Issue a `GetRequest`
            let result = client.get(GetRequest { key }).await;

            match result {
                // If it succeeds print the value
                Ok(response) => {
                    let value_bytes = response.into_inner().value;
                    let value = String::from_utf8_lossy(&value_bytes[..]);
                    print!("{}", value);
                }
                // If not found we shouldn't panic
                Err(status) if status.code() == Code::NotFound => {
                    eprintln!("not found");
                    std::process::exit(1);
                }
                // Panic on other errors
                Err(status) => {
                    panic!("{:?}", status);
                }
            }
        }
        Command::Set { key } => {
            // Create a client for our server
            let mut client = make_client(addr).await.unwrap();

            // Read the value from stdin
            let mut stdin = tokio::io::stdin();
            let mut value = Vec::new();
            stdin.read_to_end(&mut value).await.unwrap();

            // Issue a `SetRequest`
            client.set(SetRequest { key, value }).await.unwrap();

            // All good :+1:
            println!("OK");
        }
        Command::Subscribe => {
            // Create a client for our server
            let mut client = make_client(addr).await.unwrap();

            // Create a subscription
            let mut stream = client
                .subscribe(SubscribeRequest {})
                .await
                .unwrap()
                .into_inner();

            println!("Stream created!");

            // Await new items
            while let Some(item) = stream.next().await {
                let item = item.unwrap();
                println!("key inserted: {:?}", item.key);
            }
        }
    }
}

// We make this a separate function so we're able to call it from tests.
async fn serve_forever(listener: TcpListener) -> Result<(), Box<dyn std::error::Error>> {
    // Build our database for holding the key/value pairs
    let db = Arc::new(RwLock::new(HashMap::new()));

    let (tx, rx) = broadcast::channel(1024);

    // Drop the first receiver to avoid retaining messages in the channel
    drop(rx);

    // Build our tonic `Service`
    let service = key_value_store_server::KeyValueStoreServer::new(ServerImpl { db, tx });

    // Response classifier that doesn't consider `Ok`, `Invalid Argument`, or `Not Found` as
    // failures
    let classifier = GrpcErrorsAsFailures::new()
        .with_success(GrpcCode::InvalidArgument)
        .with_success(GrpcCode::NotFound);

    // Build our middleware stack
    let layer = ServiceBuilder::new()
        // Set a timeout
        .timeout(Duration::from_secs(10))
        // Compress responses
        .layer(CompressionLayer::new())
        // Mark the `Authorization` header as sensitive so it doesn't show in logs
        .layer(SetSensitiveHeadersLayer::new(once(header::AUTHORIZATION)))
        // Log all requests and responses
        .layer(
            TraceLayer::new(SharedClassifier::new(classifier))
                .make_span_with(DefaultMakeSpan::new().include_headers(true)),
        )
        .into_inner();

    // Build and run the server
    let addr = listener.local_addr()?;
    tracing::info!("Listening on {}", addr);
    tonic::transport::Server::builder()
        .layer(layer)
        .add_service(service)
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await?;

    Ok(())
}

// Implementation of the server trait generated by tonic
#[derive(Debug, Clone)]
struct ServerImpl {
    db: Arc<RwLock<HashMap<String, Bytes>>>,
    tx: Sender<SubscribeReply>,
}

#[async_trait]
impl key_value_store_server::KeyValueStore for ServerImpl {
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetReply>, Status> {
        let key = request.into_inner().key;

        if let Some(value) = self.db.read().unwrap().get(&key).cloned() {
            let reply = GetReply {
                value: value.to_vec(),
            };

            Ok(Response::new(reply))
        } else {
            Err(Status::not_found("key not found"))
        }
    }

    async fn set(&self, request: Request<SetRequest>) -> Result<Response<SetReply>, Status> {
        let SetRequest { key, value } = request.into_inner();
        let value = Bytes::from(value);

        // SendError is only possible when there are no subscribers - so can safely be ignored here
        let _send = self.tx.send(SubscribeReply { key: key.clone() });

        self.db.write().unwrap().insert(key, value);

        Ok(Response::new(SetReply {}))
    }

    type SubscribeStream =
        Pin<Box<dyn Stream<Item = Result<SubscribeReply, Status>> + Send + Sync + 'static>>;

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let SubscribeRequest {} = request.into_inner();

        let rx = self.tx.subscribe();
        let stream = BroadcastStream::new(rx)
            .filter_map(|item| async move {
                // ignore receive errors
                item.ok()
            })
            .map(Ok);
        let stream = Box::pin(stream) as Self::SubscribeStream;
        let res = Response::new(stream);

        Ok(res)
    }
}

// Build a client with a few middleware applied and connect to the server
async fn make_client(
    addr: SocketAddr,
) -> Result<
    KeyValueStoreClient<
        impl Service<
                http::Request<Body>,
                Response = http::Response<impl HttpBody<Data = Bytes, Error = impl Into<BoxError>>>,
                Error = impl Into<BoxError>,
            > + Clone
            + Send
            + Sync
            + 'static,
    >,
    tonic::transport::Error,
> {
    let uri = format!("http://{}", addr)
        .parse::<tonic::transport::Uri>()
        .unwrap();

    // We have to use a `tonic::transport::Channel` as it implements `Service` so we can apply
    // middleware to it
    let channel = Channel::builder(uri).connect().await?;

    // Apply middleware to our client
    let channel = ServiceBuilder::new()
        // Decompress response bodies
        .layer(DecompressionLayer::new())
        // Set a `User-Agent` header
        .layer(SetRequestHeaderLayer::overriding(
            header::USER_AGENT,
            HeaderValue::from_static("tonic-key-value-store"),
        ))
        // Log all requests and responses
        .layer(
            TraceLayer::new_for_grpc().make_span_with(DefaultMakeSpan::new().include_headers(true)),
        )
        // Build our final `Service`
        .service(channel);

    // Construct our tonic client
    Ok(KeyValueStoreClient::new(channel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_and_set_value() {
        let addr = run_in_background().await;

        let mut client = make_client(addr).await.unwrap();

        let mut stream = client
            .subscribe(SubscribeRequest {})
            .await
            .unwrap()
            .into_inner();

        let key = "foo".to_string();
        let value = vec![1_u8, 3, 3, 7];

        let status = client
            .get(GetRequest { key: key.clone() })
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::NotFound);

        client
            .set(SetRequest {
                key: key.clone(),
                value: value.clone(),
            })
            .await
            .unwrap();

        let server_value = client
            .get(GetRequest { key: key.clone() })
            .await
            .unwrap()
            .into_inner()
            .value;
        assert_eq!(value, server_value);

        let streamed_key = tokio::time::timeout(Duration::from_millis(100), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .key;
        assert_eq!(streamed_key, "foo");
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
