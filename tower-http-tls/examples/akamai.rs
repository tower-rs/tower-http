extern crate futures;
extern crate hyper;
extern crate tokio_buf;
extern crate tower;
extern crate tower_http;
extern crate tower_http_tls;
extern crate tower_hyper;

use futures::Future;
use hyper::Request;
use tokio_buf::util::BufStreamExt;
use tower::{MakeService, Service};
use tower_http::BodyExt;
use tower_http_tls::TlsConnector;
use tower_hyper::Connect;

fn main() {
    hyper::rt::run(connect());
}

fn connect() -> impl Future<Item = (), Error = ()> {
    let destination = "https://http2.akamai.com";

    let connector = TlsConnector::with_root(true);
    let mut client = Connect::new(connector);

    client
        .make_service(destination)
        .map_err(|err| eprintln!("Connect Error {:?}", err))
        .and_then(|mut conn| {
            let request = Request::get("https://http2.akamai.com/")
                .body(Vec::new())
                .unwrap();

            conn.call(request)
                .map_err(|e| eprintln!("Call Error: {}", e))
        })
        .and_then(|response| {
            println!("Response Status: {:?}", response.status());
            response
                .into_body()
                .into_buf_stream()
                .collect::<Vec<u8>>()
                .map(|v| String::from_utf8(v).unwrap())
                .map_err(|e| eprintln!("Body Error: {:?}", e))
        })
        .and_then(|body| {
            println!("Response Body: {}", body);
            Ok(())
        })
}
