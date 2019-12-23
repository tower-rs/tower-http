extern crate http;
extern crate tower_request_modifier;
extern crate tower_service;
extern crate tower_test;

use http::uri::{Authority, Scheme};
use http::{Request, Response};
use tokio_test::{assert_ready_ok, task};
use tower_request_modifier::BuilderError;
use tower_request_modifier::{Builder, RequestModifier};
use tower_service::Service;
use tower_test::mock;

#[tokio::test]
async fn adds_origin_to_requests() {
    let scheme = Scheme::HTTP;
    let authority: Authority = "www.example.com".parse().unwrap();

    let (service, mut handle) = mock::pair();
    let mut task = task::spawn(());

    let mut add_origin = Builder::new()
        .set_origin("http://www.example.com")
        .build(service)
        .unwrap();

    let request = Request::get("/").body(()).unwrap();

    assert_ready_ok!(task.enter(|cx, _| add_origin.poll_ready(cx)));
    let _response = add_origin.call(request);

    // Get the request
    let request = handle.next_request().await.unwrap();
    let (request, send_response) = request;

    // Assert that the origin is set
    assert_eq!(request.uri().scheme().unwrap(), &scheme);
    assert_eq!(request.uri().authority().unwrap(), &authority);

    // Make everything happy:
    let response = Response::builder().status(204).body(());

    send_response.send_response(response);
}

#[tokio::test]
async fn adds_header_to_requests() {
    let header = "authorization";
    let token = "Bearer ee2c2e06-0254-441d-b885-5bade6d7f3b2";

    let (service, mut handle) = mock::pair();
    let mut task = task::spawn(());

    let mut add_token = Builder::new()
        .add_header(header, token)
        .build(service)
        .unwrap();

    let request = Request::get("/").body(()).unwrap();

    assert_ready_ok!(task.enter(|cx, _| add_token.poll_ready(cx)));
    let _response = add_token.call(request);

    // Get the request
    let (request, send_response) = handle.next_request().await.unwrap();

    // Assert that the token header is set
    assert!(request.headers().contains_key(header.to_owned()));

    // Make everything happy:
    let response = Response::builder().status(204).body(());

    send_response.send_response(response);
}

#[tokio::test]
async fn run_arbitrary_modifier() {
    let (service, mut handle) = mock::pair();
    let mut task = task::spawn(());
    let new_val = "new value";
    let new_uri = "http://www.example.com/";

    let mut replace_body = Builder::new()
        .add_modifier(Box::new(move |req| {
            let (mut req, _) = req.into_parts();

            // Replace request URI
            req.uri = new_uri.parse().unwrap();

            // Build new request with different body
            Request::from_parts(req, new_val.to_owned())
        }))
        .build(service)
        .unwrap();

    let request = Request::get("http://example.org/")
        .body("initial value".to_owned())
        .unwrap();

    assert_ready_ok!(task.enter(|cx, _| replace_body.poll_ready(cx)));
    let _response = replace_body.call(request);

    // Get the request
    let (request, send_response) = handle.next_request().await.unwrap();

    // Assert that the body is set
    assert_eq!(request.body(), &new_val);

    // Assert that the uri is set
    assert_eq!(request.uri(), new_uri);

    // Make everything happy:
    let response = Response::builder().status(204).body(());
    send_response.send_response(response);
}

#[test]
fn does_not_build_with_relative_uri() {
    let _ = (Builder::new().set_origin("/").build(())
        as Result<RequestModifier<(), ()>, BuilderError>)
        .unwrap_err();
}

#[test]
fn does_not_build_with_path() {
    let _ = (Builder::new()
        .set_origin("http://www.example.com/foo")
        .build(()) as Result<RequestModifier<(), ()>, BuilderError>)
        .unwrap_err();
}

#[test]
fn can_build() {
    let _ = (Builder::new()
        .add_header(
            "authorization",
            "Bearer ee2c2e06-0254-441d-b885-5bade6d7f3b2",
        )
        .set_origin("http://www.example.com")
        .add_modifier(Box::new(|req| req))
        .build(()) as Result<RequestModifier<(), ()>, BuilderError>)
        .unwrap();
}
