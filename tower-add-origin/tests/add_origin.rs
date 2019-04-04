extern crate http;
extern crate tower_add_origin;
extern crate tower_mock;
extern crate tower_service;

use http::uri::{Authority, Scheme};
use http::{Request, Response};
use tower_add_origin::{AddOrigin, Builder};
use tower_mock::*;
use tower_service::Service;

#[test]
fn adds_origin_to_requests() {
    let scheme = Scheme::HTTP;
    let authority: Authority = "www.example.com".parse().unwrap();

    let (mock, mut handle) = Mock::new();
    let mut add_origin = AddOrigin::new(mock, scheme.clone(), authority.clone());

    let request = Request::get("/").body(()).unwrap();

    assert!(add_origin.poll_ready().is_ok());
    let _response = add_origin.call(request);

    // Get the request
    let request = handle.next_request().unwrap();
    let (request, send_response) = request.into_parts();

    // Assert that the origin is set
    assert_eq!(request.uri().scheme_part().unwrap(), &scheme);
    assert_eq!(request.uri().authority_part().unwrap(), &authority);

    // Make everything happy:
    let response = Response::builder().status(204).body(());

    send_response.respond(response);
}

#[test]
fn does_not_build_with_relative_uri() {
    let _ = Builder::new().uri("/").build(()).unwrap_err();
}

#[test]
fn does_not_build_with_path() {
    let _ = Builder::new()
        .uri("http://www.example.com/foo")
        .build(())
        .unwrap_err();
}
