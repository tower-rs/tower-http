use brotli::BrotliDecompress;
use crate::services::ServeFile;
use mime::Mime;
use flate2::bufread::DeflateDecoder;
use flate2::bufread::GzDecoder;
use http::header;
use http::Method;
use http::{Request, StatusCode};
use http_body::Body as _;
use hyper::Body;
use std::io::Read;
use std::str::FromStr;
use tower::ServiceExt;

#[tokio::test]
async fn basic() {
    let svc = ServeFile::new("../README.md");

    let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/markdown");

    let body = res.into_body().data().await.unwrap().unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(body.starts_with("# Tower HTTP"));
}

#[tokio::test]
async fn basic_with_mime() {
    let svc = ServeFile::new_with_mime("../README.md", &Mime::from_str("image/jpg").unwrap());

    let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

    assert_eq!(res.headers()["content-type"], "image/jpg");

    let body = res.into_body().data().await.unwrap().unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(body.starts_with("# Tower HTTP"));
}

#[tokio::test]
async fn head_request() {
    let svc = ServeFile::new("../test-files/precompressed.txt");

    let mut request = Request::new(Body::empty());
    *request.method_mut() = Method::HEAD;
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-length"], "23");

    let body = res.into_body().data().await;
    assert!(body.is_none());
}

#[tokio::test]
async fn precompresed_head_request() {
    let svc = ServeFile::new("../test-files/precompressed.txt").precompressed_gzip();

    let request = Request::builder()
        .header("Accept-Encoding", "gzip")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "gzip");
    assert_eq!(res.headers()["content-length"], "59");

    let body = res.into_body().data().await;
    assert!(body.is_none());
}

#[tokio::test]
async fn precompressed_gzip() {
    let svc = ServeFile::new("../test-files/precompressed.txt").precompressed_gzip();

    let request = Request::builder()
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "gzip");

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decoder = GzDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert!(decompressed.starts_with("\"This is a test file!\""));
}

#[tokio::test]
async fn unsupported_precompression_alogrithm_fallbacks_to_uncompressed() {
    let svc = ServeFile::new("../test-files/precompressed.txt").precompressed_gzip();

    let request = Request::builder()
        .header("Accept-Encoding", "br")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert!(res.headers().get("content-encoding").is_none());

    let body = res.into_body().data().await.unwrap().unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.starts_with("\"This is a test file!\""));
}

#[tokio::test]
async fn missing_precompressed_variant_fallbacks_to_uncompressed() {
    let svc = ServeFile::new("../test-files/missing_precompressed.txt").precompressed_gzip();

    let request = Request::builder()
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    // Uncompressed file is served because compressed version is missing
    assert!(res.headers().get("content-encoding").is_none());

    let body = res.into_body().data().await.unwrap().unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.starts_with("Test file!"));
}

#[tokio::test]
async fn missing_precompressed_variant_fallbacks_to_uncompressed_head_request() {
    let svc = ServeFile::new("../test-files/missing_precompressed.txt").precompressed_gzip();

    let request = Request::builder()
        .header("Accept-Encoding", "gzip")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-length"], "11");
    // Uncompressed file is served because compressed version is missing
    assert!(res.headers().get("content-encoding").is_none());

    let body = res.into_body().data().await;
    assert!(body.is_none());
}

#[tokio::test]
async fn only_precompressed_variant_existing() {
    let svc = ServeFile::new("../test-files/only_gzipped.txt").precompressed_gzip();

    let request = Request::builder().body(Body::empty()).unwrap();
    let res = svc.clone().oneshot(request).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Should reply with gzipped file if client supports it
    let request = Request::builder()
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "gzip");

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decoder = GzDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert!(decompressed.starts_with("\"This is a test file\""));
}

#[tokio::test]
async fn precompressed_br() {
    let svc = ServeFile::new("../test-files/precompressed.txt").precompressed_br();

    let request = Request::builder()
        .header("Accept-Encoding", "gzip,br")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "br");

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decompressed = Vec::new();
    BrotliDecompress(&mut &body[..], &mut decompressed).unwrap();
    let decompressed = String::from_utf8(decompressed.to_vec()).unwrap();
    assert!(decompressed.starts_with("\"This is a test file!\""));
}

#[tokio::test]
async fn precompressed_deflate() {
    let svc = ServeFile::new("../test-files/precompressed.txt").precompressed_deflate();
    let request = Request::builder()
        .header("Accept-Encoding", "deflate,br")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "deflate");

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decoder = DeflateDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert!(decompressed.starts_with("\"This is a test file!\""));
}

#[tokio::test]
async fn multi_precompressed() {
    let svc = ServeFile::new("../test-files/precompressed.txt")
        .precompressed_gzip()
        .precompressed_br();

    let request = Request::builder()
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.clone().oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "gzip");

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decoder = GzDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert!(decompressed.starts_with("\"This is a test file!\""));

    let request = Request::builder()
        .header("Accept-Encoding", "br")
        .body(Body::empty())
        .unwrap();
    let res = svc.clone().oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "br");

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decompressed = Vec::new();
    BrotliDecompress(&mut &body[..], &mut decompressed).unwrap();
    let decompressed = String::from_utf8(decompressed.to_vec()).unwrap();
    assert!(decompressed.starts_with("\"This is a test file!\""));
}

#[tokio::test]
async fn with_custom_chunk_size() {
    let svc = ServeFile::new("../README.md").with_buf_chunk_size(1024 * 32);

    let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/markdown");

    let body = res.into_body().data().await.unwrap().unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert!(body.starts_with("# Tower HTTP"));
}

#[tokio::test]
async fn fallbacks_to_different_precompressed_variant_if_not_found() {
    let svc = ServeFile::new("../test-files/precompressed_br.txt")
        .precompressed_gzip()
        .precompressed_deflate()
        .precompressed_br();

    let request = Request::builder()
        .header("Accept-Encoding", "gzip,deflate,br")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "br");

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decompressed = Vec::new();
    BrotliDecompress(&mut &body[..], &mut decompressed).unwrap();
    let decompressed = String::from_utf8(decompressed.to_vec()).unwrap();
    assert!(decompressed.starts_with("Test file"));
}

#[tokio::test]
async fn fallbacks_to_different_precompressed_variant_if_not_found_head_request() {
    let svc = ServeFile::new("../test-files/precompressed_br.txt")
        .precompressed_gzip()
        .precompressed_deflate()
        .precompressed_br();

    let request = Request::builder()
        .header("Accept-Encoding", "gzip,deflate,br")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-length"], "15");
    assert_eq!(res.headers()["content-encoding"], "br");

    let body = res.into_body().data().await;
    assert!(body.is_none());
}

#[tokio::test]
async fn returns_404_if_file_doesnt_exist() {
    let svc = ServeFile::new("../this-doesnt-exist.md");

    let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(res.headers().get(header::CONTENT_TYPE).is_none());
}

#[tokio::test]
async fn returns_404_if_file_doesnt_exist_when_precompression_is_used() {
    let svc = ServeFile::new("../this-doesnt-exist.md").precompressed_deflate();

    let request = Request::builder()
        .header("Accept-Encoding", "deflate")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(res.headers().get(header::CONTENT_TYPE).is_none());
}

#[tokio::test]
async fn last_modified() {
    let svc = ServeFile::new("../README.md");

    let req = Request::builder().body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let last_modified = res
        .headers()
        .get(header::LAST_MODIFIED)
        .expect("Missing last modified header!");

    // -- If-Modified-Since

    let svc = ServeFile::new("../README.md");
    let req = Request::builder()
        .header(header::IF_MODIFIED_SINCE, last_modified)
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    let body = res.into_body().data().await;
    assert!(body.is_none());

    let svc = ServeFile::new("../README.md");
    let req = Request::builder()
        .header(header::IF_MODIFIED_SINCE, "Fri, 09 Aug 1996 14:21:40 GMT")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let readme_bytes = include_bytes!("../../../../../README.md");
    let body = res.into_body().data().await.unwrap().unwrap();
    assert_eq!(body.as_ref(), readme_bytes);

    // -- If-Unmodified-Since

    let svc = ServeFile::new("../README.md");
    let req = Request::builder()
        .header(header::IF_UNMODIFIED_SINCE, last_modified)
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().data().await.unwrap().unwrap();
    assert_eq!(body.as_ref(), readme_bytes);

    let svc = ServeFile::new("../README.md");
    let req = Request::builder()
        .header(header::IF_UNMODIFIED_SINCE, "Fri, 09 Aug 1996 14:21:40 GMT")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PRECONDITION_FAILED);
    let body = res.into_body().data().await;
    assert!(body.is_none());
}
