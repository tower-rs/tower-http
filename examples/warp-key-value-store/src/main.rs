use bytes::Bytes;
use hyper::{Body, Request, Response, Server, StatusCode};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::{collections::HashMap, pin::Pin};
use structopt::StructOpt;
use tower::{make::Shared, Service, ServiceBuilder};
use tower_http::{add_extension::AddExtensionLayer, compression::CompressionLayer};
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

    // // Build our database for holding the key/value pairs
    // let db: Database = Arc::new(RwLock::new(HashMap::new()));

    // // Build or warp `Filter` by combining each individual filter
    // let filter = get().or(set());

    // // Convert our `Filter` into a `Service`
    // let warp_service = warp::service(filter);

    // let service = ServiceBuilder::new()
    //     // Set a timeout
    //     .timeout(Duration::from_secs(10))
    //     // Share the database with each handler via a request extension
    //     .layer(AddExtensionLayer::new(db))
    //     // Compress responses
    //     .layer(CompressionLayer::new())
    //     // Build our final `Service`
    //     .service(warp_service);

    // Run the service using hyper
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));

    tracing::info!("Listening on {}", addr);

    Server::bind(&addr)
        // .serve(Shared::new(service))
        .serve(Shared::new(MySvc))
        .with_graceful_shutdown(MyFuture)
        .await
        .unwrap();
}

struct MyFuture;

impl std::future::Future for MyFuture {
    type Output = ();

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        todo!()
    }
}

#[derive(Clone, Copy)]
struct MySvc;

impl Service<Request<Body>> for MySvc {
    type Response = Response<MyBody>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        todo!()
    }
}

struct MyBody {
    cell: std::cell::UnsafeCell<()>,
}

impl hyper::body::HttpBody for MyBody {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Self::Data, Self::Error>>> {
        todo!()
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<Option<hyper::HeaderMap>, Self::Error>> {
        todo!()
    }
}

// /// Filter for looking up a key
// pub fn get() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
//     warp::get()
//         .and(path!(String))
//         .and(filters::ext::get::<Database>())
//         .map(|path: String, db: Database| {
//             let state = db.read().unwrap();

//             if let Some(value) = state.get(&path).cloned() {
//                 Response::new(Body::from(value))
//             } else {
//                 Response::builder()
//                     .status(StatusCode::NOT_FOUND)
//                     .body(Body::empty())
//                     .unwrap()
//             }
//         })
// }

// /// Filter for setting a key/value pair
// pub fn set() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
//     warp::post()
//         .and(path!(String))
//         .and(filters::ext::get::<Database>())
//         .and(filters::body::bytes())
//         .map(|path: String, db: Database, value: Bytes| {
//             let mut state = db.write().unwrap();

//             state.insert(path, value);

//             Response::new(Body::empty())
//         })
// }
