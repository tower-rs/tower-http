use std::convert::Infallible;

use crate::{cors::Vary, test_helpers::Body};
use http::{header, HeaderName, HeaderValue, Method, Request, Response};
use tower::{service_fn, util::ServiceExt, Layer};

use crate::cors::{AllowHeaders, AllowMethods, AllowOrigin, Any, Cors, CorsLayer};

const INITIAL_VARY_HEADERS: HeaderValue = HeaderValue::from_static("accept, accept-encoding");
const ADDITIONAL_VARY_HEADERS: [HeaderName; 3] = [
    header::ORIGIN,
    header::ACCESS_CONTROL_REQUEST_METHOD,
    header::ACCESS_CONTROL_REQUEST_HEADERS,
];

#[tokio::test]
async fn permissive_vary_header_is_empty() {
    let svc = CorsLayer::permissive().layer(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }));

    let req = Request::builder().body(Body::empty()).unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert!(
        res.headers().get(header::VARY).is_none(),
        "Vary header should be omitted for permissive config"
    );
}

#[tokio::test]
async fn include_custom_permissive_to_vary_set_by_inner_service() {
    const PERMISSIVE_CORS_VARY_HEADERS: HeaderValue = HeaderValue::from_static(
        "origin, access-control-request-method, access-control-request-headers",
    );

    async fn inner_svc(_: Request<Body>) -> Result<Response<Body>, Infallible> {
        Ok(Response::builder()
            .header(header::VARY, INITIAL_VARY_HEADERS)
            .body(Body::empty())
            .unwrap())
    }

    let svc = CorsLayer::permissive()
        .vary(Vary::list(ADDITIONAL_VARY_HEADERS))
        .layer(service_fn(inner_svc));

    let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();
    let mut vary_headers = res.headers().get_all(header::VARY).into_iter();
    assert_eq!(vary_headers.next(), Some(&INITIAL_VARY_HEADERS));
    assert_eq!(vary_headers.next(), Some(&PERMISSIVE_CORS_VARY_HEADERS));
    assert_eq!(vary_headers.next(), None);
}

#[tokio::test]
async fn permissive_with_custom_vary_builder() {
    let custom_vary = HeaderValue::from_static("x-foo");
    let svc = CorsLayer::permissive()
        .vary(Vary::list([header::HeaderName::from_static("x-foo")]))
        .layer(service_fn(|_: Request<Body>| async {
            Ok::<_, Infallible>(Response::new(Body::empty()))
        }));

    let req = Request::builder().body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();
    let vary = res.headers().get(header::VARY);
    assert_eq!(vary, Some(&custom_vary));
}

#[tokio::test]
async fn permissive_with_inner_and_builder_vary() {
    let custom_vary = HeaderValue::from_static("x-foo");
    let inner_vary = HeaderValue::from_static("accept-encoding");
    let svc = CorsLayer::permissive()
        .vary(Vary::list([header::HeaderName::from_static("x-foo")]))
        .layer(service_fn(|_: Request<Body>| {
            let inner_vary = inner_vary.clone();
            async move {
                Ok::<_, Infallible>(
                    Response::builder()
                        .header(header::VARY, inner_vary)
                        .body(Body::empty())
                        .unwrap(),
                )
            }
        }));

    let req = Request::builder().body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();
    let mut vary_headers = res.headers().get_all(header::VARY).iter();
    assert_eq!(vary_headers.next(), Some(&inner_vary));
    assert_eq!(vary_headers.next(), Some(&custom_vary));
    assert_eq!(vary_headers.next(), None);
}

#[tokio::test]
async fn test_allow_origin_async_predicate() {
    #[derive(Clone)]
    struct Client;

    impl Client {
        async fn fetch_allowed_origins_for_path(&self, _path: String) -> Vec<HeaderValue> {
            vec![HeaderValue::from_static("http://example.com")]
        }
    }

    let client = Client;

    let allow_origin = AllowOrigin::async_predicate(|origin, parts| {
        let path = parts.uri.path().to_owned();

        async move {
            let origins = client.fetch_allowed_origins_for_path(path).await;

            origins.contains(&origin)
        }
    });

    let valid_origin = HeaderValue::from_static("http://example.com");
    let parts = http::Request::new("hello world").into_parts().0;

    let header = allow_origin
        .to_future(Some(&valid_origin), &parts)
        .await
        .unwrap();
    assert_eq!(header.0, header::ACCESS_CONTROL_ALLOW_ORIGIN);
    assert_eq!(header.1, valid_origin);

    let invalid_origin = HeaderValue::from_static("http://example.org");
    let parts = http::Request::new("hello world").into_parts().0;

    let res = allow_origin.to_future(Some(&invalid_origin), &parts).await;
    assert!(res.is_none());
}

#[tokio::test]
async fn derived_vary_header_for_mixed_wildcard_configuration() {
    let svc = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(AllowMethods::mirror_request())
        .allow_headers(AllowHeaders::mirror_request())
        .layer(service_fn(|_: Request<Body>| async {
            Ok::<_, Infallible>(Response::new(Body::empty()))
        }));

    let req = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(
        res.headers().get(header::VARY),
        Some(&HeaderValue::from_static(
            "access-control-request-method, access-control-request-headers",
        ))
    );
}

#[tokio::test]
async fn very_permissive_emits_vary_headers() {
    let svc = CorsLayer::very_permissive().layer(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }));

    let req = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(
        res.headers().get(header::VARY),
        Some(&HeaderValue::from_static(
            "origin, access-control-request-method, access-control-request-headers",
        ))
    );
}

#[tokio::test]
async fn cors_map_layer_smoke_without_vary_header() {
    let svc = Cors::new(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }))
    .allow_origin(Any)
    .allow_methods(Any)
    .allow_headers(Any);

    let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

    assert!(res.headers().get(header::VARY).is_none());
}

#[tokio::test]
async fn cors_map_layer_smoke_with_vary_header() {
    let svc = Cors::new(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }))
    .allow_origin(Any)
    .allow_methods(AllowMethods::mirror_request())
    .allow_headers(Any);

    let req = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(
        res.headers().get(header::VARY),
        Some(&HeaderValue::from_static("access-control-request-method"))
    );
}

#[tokio::test]
async fn exact_origin_does_not_emit_origin_vary_header() {
    let svc = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(HeaderValue::from_static(
            "http://example.com",
        )))
        .allow_methods([Method::GET])
        .allow_headers([header::CONTENT_TYPE])
        .layer(service_fn(|_: Request<Body>| async {
            Ok::<_, Infallible>(Response::new(Body::empty()))
        }));

    let req = Request::builder()
        .header(header::ORIGIN, "http://example.com")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert!(res.headers().get(header::VARY).is_none());
}
