use crate::services::{ServeDir, ServeFile};
use crate::test_helpers::{to_bytes, Body};
use brotli::BrotliDecompress;
use bytes::Bytes;
use flate2::bufread::{DeflateDecoder, GzDecoder};
use http::header::ALLOW;
use http::{header, Method, Response};
use http::{Request, StatusCode};
use http_body::Body as HttpBody;
use http_body_util::BodyExt;
use std::convert::Infallible;
use std::fs;
use std::io::Read;
use tower::{service_fn, ServiceExt};

/// Expected prefix of the decompressed content in precompressed test files.
const EXPECTED_CONTENT_PREFIX: &str = "Test file";

/// Root of the repository, relative to the working directory of the test binary.
const REPO_ROOT: &str = "..";
/// Directory containing test fixture files.
const TEST_FILES_DIR: &str = "../test-files";
/// Path to the repository README, used as a large test fixture.
const README_PATH: &str = "../README.md";

#[tokio::test]
async fn basic() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/markdown");

    let body = body_into_text(res.into_body()).await;

    let contents = std::fs::read_to_string(README_PATH).unwrap();
    assert_eq!(body, contents);
}

#[tokio::test]
async fn basic_with_index() {
    let svc = ServeDir::new(TEST_FILES_DIR);

    let req = Request::new(Body::empty());
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/html");

    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "<b>HTML!</b>\n");
}

#[tokio::test]
async fn head_request() {
    let svc = ServeDir::new(TEST_FILES_DIR);

    let req = Request::builder()
        .uri("/precompressed.txt")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-length"], "10");

    assert!(res.into_body().frame().await.is_none());
}

#[tokio::test]
async fn precompressed_head_request() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "gzip")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "gzip");
    assert_eq!(res.headers()["content-length"], "30");

    assert!(res.into_body().frame().await.is_none());
}

#[tokio::test]
async fn with_custom_chunk_size() {
    let svc = ServeDir::new(REPO_ROOT).with_buf_chunk_size(1024 * 32);

    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/markdown");

    let body = body_into_text(res.into_body()).await;

    let contents = std::fs::read_to_string(README_PATH).unwrap();
    assert_eq!(body, contents);
}

#[tokio::test]
async fn precompressed_gzip() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "gzip");

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let mut decoder = GzDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert!(decompressed.starts_with(EXPECTED_CONTENT_PREFIX));
}

#[tokio::test]
async fn precompressed_br() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_br();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "br")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "br");

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let mut decompressed = Vec::new();
    BrotliDecompress(&mut &body[..], &mut decompressed).unwrap();
    let decompressed = String::from_utf8(decompressed.to_vec()).unwrap();
    assert!(decompressed.starts_with(EXPECTED_CONTENT_PREFIX));
}

#[tokio::test]
async fn precompressed_deflate() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_deflate();
    let request = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "deflate,br")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "deflate");

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let mut decoder = DeflateDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert!(decompressed.starts_with(EXPECTED_CONTENT_PREFIX));
}

#[tokio::test]
async fn unsupported_precompression_alogrithm_fallbacks_to_uncompressed() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let request = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "br")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert!(res.headers().get("content-encoding").is_none());

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.starts_with(EXPECTED_CONTENT_PREFIX));
}

#[tokio::test]
async fn only_precompressed_variant_existing() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let request = Request::builder()
        .uri("/only_gzipped.txt")
        .body(Body::empty())
        .unwrap();
    let res = svc.clone().oneshot(request).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Should reply with gzipped file if client supports it
    let request = Request::builder()
        .uri("/only_gzipped.txt")
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "gzip");

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let mut decoder = GzDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert!(decompressed.starts_with(EXPECTED_CONTENT_PREFIX));
}

#[tokio::test]
async fn missing_precompressed_variant_fallbacks_to_uncompressed() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let request = Request::builder()
        .uri("/missing_precompressed.txt")
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    // Uncompressed file is served because compressed version is missing
    assert!(res.headers().get("content-encoding").is_none());

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.starts_with(EXPECTED_CONTENT_PREFIX));
}

#[tokio::test]
async fn missing_precompressed_variant_fallbacks_to_uncompressed_for_head_request() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let request = Request::builder()
        .uri("/missing_precompressed.txt")
        .header("Accept-Encoding", "gzip")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-length"], "10");
    // Uncompressed file is served because compressed version is missing
    assert!(res.headers().get("content-encoding").is_none());

    assert!(res.into_body().frame().await.is_none());
}

#[tokio::test]
async fn precompressed_without_extension() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let request = Request::builder()
        .uri("/extensionless_precompressed")
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(res.headers()["content-type"], "application/octet-stream");
    assert_eq!(res.headers()["content-encoding"], "gzip");

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let mut decoder = GzDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();

    let correct =
        fs::read_to_string(format!("{TEST_FILES_DIR}/extensionless_precompressed")).unwrap();
    assert_eq!(decompressed, correct);
}

#[tokio::test]
async fn missing_precompressed_without_extension_fallbacks_to_uncompressed() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let request = Request::builder()
        .uri("/extensionless_precompressed_missing")
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    assert_eq!(res.headers()["content-type"], "application/octet-stream");
    assert!(res.headers().get("content-encoding").is_none());

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();

    let correct = fs::read_to_string(format!(
        "{TEST_FILES_DIR}/extensionless_precompressed_missing"
    ))
    .unwrap();
    assert_eq!(body, correct);
}

#[tokio::test]
async fn access_to_sub_dirs() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/tower-http/Cargo.toml")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/x-toml");

    let body = body_into_text(res.into_body()).await;

    let contents = std::fs::read_to_string("Cargo.toml").unwrap();
    assert_eq!(body, contents);
}

#[tokio::test]
async fn not_found() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/not-found")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(res.headers().get(header::CONTENT_TYPE).is_none());

    let body = body_into_text(res.into_body()).await;
    assert!(body.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn not_found_when_not_a_directory() {
    let svc = ServeDir::new(TEST_FILES_DIR);

    // `index.html` is a file, and we are trying to request
    // it as a directory.
    let req = Request::builder()
        .uri("/index.html/some_file")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    // This should lead to a 404
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(res.headers().get(header::CONTENT_TYPE).is_none());

    let body = body_into_text(res.into_body()).await;
    assert!(body.is_empty());
}

#[tokio::test]
async fn not_found_precompressed() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let req = Request::builder()
        .uri("/not-found")
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(res.headers().get(header::CONTENT_TYPE).is_none());

    let body = body_into_text(res.into_body()).await;
    assert!(body.is_empty());
}

#[tokio::test]
async fn fallbacks_to_different_precompressed_variant_if_not_found_for_head_request() {
    let svc = ServeDir::new(TEST_FILES_DIR)
        .precompressed_gzip()
        .precompressed_br();

    let req = Request::builder()
        .uri("/precompressed_br.txt")
        .header("Accept-Encoding", "gzip,br,deflate")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "br");
    assert_eq!(res.headers()["content-length"], "15");

    assert!(res.into_body().frame().await.is_none());
}

#[tokio::test]
async fn fallbacks_to_different_precompressed_variant_if_not_found() {
    let svc = ServeDir::new(TEST_FILES_DIR)
        .precompressed_gzip()
        .precompressed_br();

    let req = Request::builder()
        .uri("/precompressed_br.txt")
        .header("Accept-Encoding", "gzip,br,deflate")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "br");

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let mut decompressed = Vec::new();
    BrotliDecompress(&mut &body[..], &mut decompressed).unwrap();
    let decompressed = String::from_utf8(decompressed.to_vec()).unwrap();
    assert!(decompressed.starts_with(EXPECTED_CONTENT_PREFIX));
}

#[tokio::test]
async fn redirect_to_trailing_slash_on_dir() {
    let svc = ServeDir::new(".");

    let req = Request::builder().uri("/src").body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);

    let location = &res.headers()[http::header::LOCATION];
    assert_eq!(location, "/src/");
}

#[tokio::test]
async fn redirect_to_trailing_slash_with_redirect_path_prefix() {
    let cases = [
        ("/foo", "/src", "/foo/src/"),
        ("/foo/", "/src", "/foo//src/"),
        ("", "/src", "/src/"),
        ("/foo", "/src?key=value", "/foo/src/?key=value"),
        ("/foo", "/s%72c", "/foo/s%72c/"),
    ];

    for (prefix, uri, expected_location) in cases {
        let svc = ServeDir::new(".").redirect_path_prefix(prefix);

        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);

        let location = &res.headers()[http::header::LOCATION];
        assert_eq!(location, expected_location);
    }
}

#[tokio::test]
async fn redirect_path_prefix_preserved_through_fallback() {
    async fn fallback<B>(_: Request<B>) -> Result<Response<Body>, Infallible> {
        Ok(Response::new(Body::empty()))
    }

    let svc = ServeDir::new(".")
        .redirect_path_prefix("/foo")
        .fallback(tower::service_fn(fallback));

    let req = Request::builder().uri("/src").body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);

    let location = &res.headers()[http::header::LOCATION];
    assert_eq!(location, "/foo/src/");
}

#[tokio::test]
async fn empty_directory_without_index() {
    let svc = ServeDir::new(".").append_index_html_on_directories(false);

    let req = Request::new(Body::empty());
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(res.headers().get(header::CONTENT_TYPE).is_none());

    let body = body_into_text(res.into_body()).await;
    assert!(body.is_empty());
}

#[tokio::test]
async fn empty_directory_without_index_no_information_leak() {
    let svc = ServeDir::new(REPO_ROOT).append_index_html_on_directories(false);

    let req = Request::builder()
        .uri("/test-files")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(res.headers().get(header::CONTENT_TYPE).is_none());

    let body = body_into_text(res.into_body()).await;
    assert!(body.is_empty());
}

async fn body_into_text<B>(body: B) -> String
where
    B: HttpBody<Data = bytes::Bytes> + Unpin,
    B::Error: std::fmt::Debug,
{
    let bytes = to_bytes(body).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn access_cjk_percent_encoded_uri_path() {
    // percent encoding present of 你好世界.txt
    let cjk_filename_encoded = "%E4%BD%A0%E5%A5%BD%E4%B8%96%E7%95%8C.txt";

    let svc = ServeDir::new(TEST_FILES_DIR);

    let req = Request::builder()
        .uri(format!("/{}", cjk_filename_encoded))
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/plain");
}

#[tokio::test]
async fn access_space_percent_encoded_uri_path() {
    let encoded_filename = "filename%20with%20space.txt";

    let svc = ServeDir::new(TEST_FILES_DIR);

    let req = Request::builder()
        .uri(format!("/{}", encoded_filename))
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/plain");
}

#[tokio::test]
async fn read_partial_empty() {
    let svc = ServeDir::new(TEST_FILES_DIR);

    let req = Request::builder()
        .uri("/empty.txt")
        .header("Range", "bytes=0-")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(res.headers()["content-length"], "0");
    assert_eq!(res.headers()["content-range"], "bytes 0-0/0");

    let body = to_bytes(res.into_body()).await.ok().unwrap();
    assert!(body.is_empty());
}

#[tokio::test]
async fn read_partial_in_bounds() {
    let svc = ServeDir::new(REPO_ROOT);
    let bytes_start_incl = 9;
    let bytes_end_incl = 1023;

    let req = Request::builder()
        .uri("/README.md")
        .header(
            "Range",
            format!("bytes={}-{}", bytes_start_incl, bytes_end_incl),
        )
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    let file_contents = std::fs::read(README_PATH).unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        res.headers()["content-length"],
        (bytes_end_incl - bytes_start_incl + 1).to_string()
    );
    assert!(res.headers()["content-range"]
        .to_str()
        .unwrap()
        .starts_with(&format!(
            "bytes {}-{}/{}",
            bytes_start_incl,
            bytes_end_incl,
            file_contents.len()
        )));
    assert_eq!(res.headers()["content-type"], "text/markdown");

    let body = to_bytes(res.into_body()).await.ok().unwrap();
    let source = Bytes::from(file_contents[bytes_start_incl..=bytes_end_incl].to_vec());
    assert_eq!(body, source);
}

#[tokio::test]
async fn read_partial_accepts_out_of_bounds_range() {
    let svc = ServeDir::new(REPO_ROOT);
    let bytes_start_incl = 0;
    let bytes_end_excl = 9999999;
    let requested_len = bytes_end_excl - bytes_start_incl;

    let req = Request::builder()
        .uri("/README.md")
        .header(
            "Range",
            format!("bytes={}-{}", bytes_start_incl, requested_len - 1),
        )
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    let file_contents = std::fs::read(README_PATH).unwrap();
    // Out of bounds range gives all bytes
    assert_eq!(
        res.headers()["content-range"],
        &format!(
            "bytes 0-{}/{}",
            file_contents.len() - 1,
            file_contents.len()
        )
    )
}

#[tokio::test]
async fn read_partial_errs_on_garbage_header() {
    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header("Range", "bad_format")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let file_contents = std::fs::read(README_PATH).unwrap();
    assert_eq!(
        res.headers()["content-range"],
        &format!("bytes */{}", file_contents.len())
    )
}

#[tokio::test]
async fn read_partial_errs_on_bad_range() {
    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header("Range", "bytes=-1-15")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let file_contents = std::fs::read(README_PATH).unwrap();
    assert_eq!(
        res.headers()["content-range"],
        &format!("bytes */{}", file_contents.len())
    )
}

#[tokio::test]
async fn accept_encoding_identity() {
    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header("Accept-Encoding", "identity")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    // Identity encoding should not be included in the response headers
    assert!(res.headers().get("content-encoding").is_none());
}

#[tokio::test]
async fn last_modified() {
    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let last_modified = res
        .headers()
        .get(header::LAST_MODIFIED)
        .expect("Missing last modified header!");

    // -- If-Modified-Since

    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MODIFIED_SINCE, last_modified)
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    assert!(res.into_body().frame().await.is_none());

    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MODIFIED_SINCE, "Fri, 09 Aug 1996 14:21:40 GMT")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let readme_bytes = include_bytes!("../../../../../README.md");
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), readme_bytes);

    // -- If-Unmodified-Since

    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_UNMODIFIED_SINCE, last_modified)
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), readme_bytes);

    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_UNMODIFIED_SINCE, "Fri, 09 Aug 1996 14:21:40 GMT")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PRECONDITION_FAILED);
    assert!(res.into_body().frame().await.is_none());
}

#[tokio::test]
async fn with_fallback_svc() {
    async fn fallback<B>(req: Request<B>) -> Result<Response<Body>, Infallible> {
        Ok(Response::new(Body::from(format!(
            "from fallback {}",
            req.uri().path()
        ))))
    }

    let svc = ServeDir::new(REPO_ROOT).fallback(tower::service_fn(fallback));

    let req = Request::builder()
        .uri("/doesnt-exist")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "from fallback /doesnt-exist");
}

#[tokio::test]
async fn with_fallback_serve_file() {
    let svc = ServeDir::new(REPO_ROOT).fallback(ServeFile::new(README_PATH));

    let req = Request::builder()
        .uri("/doesnt-exist")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/markdown");

    let body = body_into_text(res.into_body()).await;

    let contents = std::fs::read_to_string(README_PATH).unwrap();
    assert_eq!(body, contents);
}

#[tokio::test]
async fn method_not_allowed() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(res.headers()[ALLOW], "GET,HEAD");
}

#[tokio::test]
async fn calling_fallback_on_not_allowed() {
    async fn fallback<B>(req: Request<B>) -> Result<Response<Body>, Infallible> {
        Ok(Response::new(Body::from(format!(
            "from fallback {}",
            req.uri().path()
        ))))
    }

    let svc = ServeDir::new(REPO_ROOT)
        .call_fallback_on_method_not_allowed(true)
        .fallback(tower::service_fn(fallback));

    let req = Request::builder()
        .method(Method::POST)
        .uri("/doesnt-exist")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "from fallback /doesnt-exist");
}

#[tokio::test]
async fn method_not_allowed_without_fallback() {
    let svc = ServeDir::new(REPO_ROOT).call_fallback_on_method_not_allowed(true);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(res.headers()[ALLOW], "GET,HEAD");
}

#[tokio::test]
async fn with_fallback_svc_and_not_append_index_html_on_directories() {
    async fn fallback<B>(req: Request<B>) -> Result<Response<Body>, Infallible> {
        Ok(Response::new(Body::from(format!(
            "from fallback {}",
            req.uri().path()
        ))))
    }

    let svc = ServeDir::new(REPO_ROOT)
        .append_index_html_on_directories(false)
        .fallback(tower::service_fn(fallback));

    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "from fallback /");
}

// https://github.com/tower-rs/tower-http/issues/308
#[tokio::test]
async fn calls_fallback_on_invalid_paths() {
    async fn fallback<T>(_: T) -> Result<Response<Body>, Infallible> {
        let mut res = Response::new(Body::empty());
        res.headers_mut()
            .insert("from-fallback", "1".parse().unwrap());
        Ok(res)
    }

    let svc = ServeDir::new(REPO_ROOT).fallback(service_fn(fallback));

    let req = Request::builder()
        .uri("/weird_%c3%28_path")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["from-fallback"], "1");
}

// https://github.com/tower-rs/tower-http/issues/573
#[tokio::test]
async fn calls_fallback_on_invalid_filenames() {
    async fn fallback<T>(_: T) -> Result<Response<Body>, Infallible> {
        let mut res = Response::new(Body::empty());
        res.headers_mut()
            .insert("from-fallback", "1".parse().unwrap());
        Ok(res)
    }

    let svc = ServeDir::new(REPO_ROOT).fallback(service_fn(fallback));

    let req = Request::builder()
        .uri("/invalid|path")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["from-fallback"], "1");
}

#[tokio::test]
async fn calls_fallback_on_null() {
    async fn fallback<T>(_: T) -> Result<Response<Body>, Infallible> {
        let mut res = Response::new(Body::empty());
        res.headers_mut()
            .insert("from-fallback", "1".parse().unwrap());
        Ok(res)
    }

    let svc = ServeDir::new(REPO_ROOT).fallback(service_fn(fallback));

    let req = Request::builder()
        .uri("/invalid-path%00")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["from-fallback"], "1");
}

#[tokio::test]
async fn not_found_when_file_requested_with_trailing_slash() {
    let svc = ServeDir::new(TEST_FILES_DIR);

    let req = Request::builder()
        .uri("/index.html/")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(res.headers().get(header::CONTENT_TYPE).is_none());

    let body = body_into_text(res.into_body()).await;
    assert!(body.is_empty());
}

#[tokio::test]
async fn file_requested_with_trailing_slash_with_fallback() {
    async fn fallback<B>(req: Request<B>) -> Result<Response<Body>, Infallible> {
        Ok(Response::new(Body::from(format!(
            "from fallback {}",
            req.uri().path()
        ))))
    }

    let svc = ServeDir::new(TEST_FILES_DIR).fallback(tower::service_fn(fallback));

    let req = Request::builder()
        .uri("/index.html/")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "from fallback /index.html/");
}

#[tokio::test]
async fn directory_with_trailing_slash_appends_index_html() {
    let svc = ServeDir::new(TEST_FILES_DIR).append_index_html_on_directories(true);
    let req = Request::builder().uri("/foo/").body(Body::empty()).unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/html");
    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "<b>HTML!</b>\n");
}

#[tokio::test]
async fn root_with_trailing_slash_serves_appends_index_html() {
    let svc = ServeDir::new(TEST_FILES_DIR).append_index_html_on_directories(true);
    let req = Request::builder().uri("/").body(Body::empty()).unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/html");
    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "<b>HTML!</b>\n");
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn verify_windows_device(name: &str, is_positive: bool) {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;

    extern "system" {
        fn GetFileType(hFile: *mut std::ffi::c_void) -> u32;
    }
    const FILE_TYPE_CHAR: u32 = 0x0002;

    let file_res = OpenOptions::new().read(true).open(name);
    if let Ok(file) = file_res {
        let handle = file.as_raw_handle();
        let file_type = unsafe { GetFileType(handle as _) };
        if is_positive {
            assert_eq!(
                file_type, FILE_TYPE_CHAR,
                "Expected Windows to treat {:?} as a system character device",
                name
            );
        } else {
            assert_ne!(
                file_type, FILE_TYPE_CHAR,
                "Expected Windows NOT to treat {:?} as a system character device",
                name
            );
        }
    }
}

#[test]
fn test_is_reserved_dos_name() {
    use super::is_reserved_dos_name;

    let positives = [
        "CON",
        "con",
        "Con",
        "PRN",
        "Prn",
        "AUX",
        "aux",
        "NUL",
        "nul",
        "CONIN$",
        "conin$",
        "CONOUT$",
        "ConOut$",
        "COM0",
        "com0",
        "Com0",
        "COM1",
        "com9",
        "Com3",
        "COM¹",
        "com³",
        "LPT0",
        "lpt0",
        "Lpt0",
        "LPT1",
        "lpt9",
        "Lpt3",
        "LPT¹",
        "lpt²",
        "CON.txt",
        "con.anything",
        "AUX.tar.gz",
        "NUL.",
        "COM1:",
        "com9.ext:",
        "CON ",
        "CON  ",
        "NUL  .txt",
        "CON\t",
        "CON\n",
        "CON\r",
        "CON \t",
        "CON\x0B",
    ];

    for name in positives {
        assert!(
            is_reserved_dos_name(|| name.encode_utf16()),
            "Expected true for {:?}",
            name
        );

        #[cfg(windows)]
        verify_windows_device(name, true);
    }

    let negatives = [
        "C0N",
        "PRN1",
        "AUX42",
        "NULL",
        "CONIN",
        "CONOUT",
        "COM10",
        "LPT42",
        "COMa",
        "LPTb",
        "safe.txt",
        "index.html",
        "aux-file.js",
        "contact.html",
    ];

    for name in negatives {
        assert!(
            !is_reserved_dos_name(|| name.encode_utf16()),
            "Expected false for {:?}",
            name
        );

        #[cfg(windows)]
        verify_windows_device(name, false);
    }
}

#[test]
fn test_build_and_validate_path_reserved_dos_names() {
    use super::ServeVariant;
    use std::path::Path;

    let variant = ServeVariant::Directory {
        append_index_html_on_directories: true,
        html_as_default_extension: false,
    };
    let base = Path::new("/base");

    let reserved = ["/CON", "/CON.txt", "/com0", "/com1", "/com¹", "/CONIN$"];

    for path in reserved {
        let result = variant.build_and_validate_path(base, path);
        if cfg!(windows) {
            assert!(result.is_none(), "Expected None for path: {}", path);
        } else {
            assert!(result.is_some(), "Expected Some for path: {}", path);
        }
    }
}

// Regression test for the Windows directory-traversal fix in #204 (tracked by #251):
// a drive-letter prefix such as `C:` must not be served as an absolute path.
#[tokio::test]
async fn reject_windows_drive_prefixed_path() {
    let svc = ServeDir::new(TEST_FILES_DIR);

    let req = Request::builder()
        .uri("/C:/windows/win.ini")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "drive-prefixed path should be rejected, not served"
    );
}

// As above, but with the `:` percent-encoded (`%3A`) to confirm the drive prefix
// is still rejected *after* URL decoding.
#[tokio::test]
async fn reject_percent_encoded_windows_drive_prefixed_path() {
    let svc = ServeDir::new(TEST_FILES_DIR);

    let req = Request::builder()
        .uri("/anypath/c%3A/windows/win.ini")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "percent-encoded drive prefix should be rejected after decoding"
    );
}

// Regression test for https://github.com/tower-rs/tower-http/issues/664
// Accept-Encoding: identity should not cause extension stripping
#[tokio::test]
async fn identity_encoding_does_not_strip_extension() {
    let svc = ServeDir::new("../test-files");

    let req = Request::builder()
        .uri("/extensionless_precompressed.foobar")
        .header("Accept-Encoding", "identity")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn identity_encoding_does_not_strip_extension_head_request() {
    let svc = ServeDir::new("../test-files");

    let req = Request::builder()
        .uri("/extensionless_precompressed.foobar")
        .method(Method::HEAD)
        .header("Accept-Encoding", "identity")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn precompressed_response_includes_vary_header() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-encoding"], "gzip");
    assert_eq!(res.headers()["vary"], "accept-encoding");
}

#[tokio::test]
async fn no_vary_header_without_precompressed_serving() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().get("vary").is_none());
}

#[tokio::test]
async fn vary_header_present_when_precompressed_configured_but_fallback_to_uncompressed() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "br")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert!(res.headers().get("content-encoding").is_none());
    assert_eq!(res.headers()["vary"], "accept-encoding");
}

#[tokio::test]
async fn vary_header_present_when_precompressed_configured_but_no_accept_encoding() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert!(res.headers().get("content-encoding").is_none());
    assert_eq!(res.headers()["vary"], "accept-encoding");
}

#[tokio::test]
async fn precompressed_head_request_includes_vary_header() {
    let svc = ServeDir::new(TEST_FILES_DIR).precompressed_gzip();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .method(Method::HEAD)
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-encoding"], "gzip");
    assert_eq!(res.headers()["vary"], "accept-encoding");
}

#[tokio::test]
async fn unsync_box_body_new() {
    use crate::body::UnsyncBoxBody;
    use http_body_util::Full;

    let body: UnsyncBoxBody<Bytes, Infallible> =
        UnsyncBoxBody::new(Full::new(Bytes::from("hello")));
    let collected = body.collect().await.unwrap().to_bytes();
    assert_eq!(collected, "hello");
}

#[tokio::test]
async fn response_body_into_unsync_box_body() {
    use crate::body::UnsyncBoxBody;

    let svc = ServeDir::new("..");
    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    // Convert the ServeDir response body into UnsyncBoxBody without double-boxing
    let boxed: UnsyncBoxBody<Bytes, std::io::Error> = res.into_body().into();
    let collected = boxed.collect().await.unwrap().to_bytes();

    let expected = std::fs::read_to_string("../README.md").unwrap();
    assert_eq!(collected, expected);
}

#[tokio::test]
async fn html_as_default_extension() {
    let svc = ServeDir::new(TEST_FILES_DIR).html_as_default_extension(true);

    let req = Request::builder().uri("/page").body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/html");

    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "<b>page</b>\n");
}

#[tokio::test]
async fn html_as_default_extension_not_found() {
    let svc = ServeDir::new(TEST_FILES_DIR).html_as_default_extension(true);

    let req = Request::builder()
        .uri("/nonexistent")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn html_as_default_extension_does_not_apply_when_extension_present() {
    let svc = ServeDir::new(TEST_FILES_DIR).html_as_default_extension(true);

    // Request a file that exists with its extension; should serve normally
    let req = Request::builder()
        .uri("/precompressed.txt")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/plain");
}

#[tokio::test]
async fn etag_is_set_on_response() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let etag = res
        .headers()
        .get(header::ETAG)
        .expect("Missing ETag header");
    let etag_str = etag.to_str().unwrap();
    // Strong ETag format: "<hex>-<hex>"
    assert!(etag_str.starts_with('"'));
    assert!(etag_str.ends_with('"'));
    assert!(!etag_str.starts_with("W/"));
    assert!(etag_str.contains('-'));
}

#[tokio::test]
async fn if_none_match_returns_304() {
    let svc = ServeDir::new(REPO_ROOT);

    // First request to get the ETag
    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let etag = res.headers().get(header::ETAG).unwrap().clone();
    let last_modified = res.headers().get(header::LAST_MODIFIED).unwrap().clone();

    // Second request with If-None-Match
    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_NONE_MATCH, &etag)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    // RFC 9110 §15.4.5: 304 MUST include validator headers
    assert_eq!(res.headers().get(header::ETAG).unwrap(), &etag);
    assert_eq!(
        res.headers().get(header::LAST_MODIFIED).unwrap(),
        &last_modified
    );
    assert!(res.into_body().frame().await.is_none());
}

#[tokio::test]
async fn if_none_match_with_non_matching_etag_returns_200() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_NONE_MATCH, "\"not-a-real-etag\"")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn if_none_match_wildcard_returns_304() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_NONE_MATCH, "*")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn if_match_with_matching_etag_succeeds() {
    let svc = ServeDir::new(REPO_ROOT);

    // First request to get the ETag
    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let etag = res.headers().get(header::ETAG).unwrap().clone();

    // Second request with If-Match
    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MATCH, etag)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn if_match_with_non_matching_etag_returns_412() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MATCH, "\"not-a-real-etag\"")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PRECONDITION_FAILED);
}

#[tokio::test]
async fn if_none_match_takes_precedence_over_if_modified_since() {
    // Per RFC 9110 §13.2.2, If-None-Match takes precedence over If-Modified-Since
    let svc = ServeDir::new(REPO_ROOT);

    // First request to get the ETag
    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let etag = res.headers().get(header::ETAG).unwrap().clone();

    // Send both If-None-Match (matching) and If-Modified-Since (very old, would normally 200)
    // If-None-Match should win and return 304
    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_NONE_MATCH, etag)
        .header(header::IF_MODIFIED_SINCE, "Fri, 09 Aug 1996 14:21:40 GMT")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn if_match_takes_precedence_over_if_unmodified_since() {
    // Per RFC 9110 §13.2.2, If-Match takes precedence over If-Unmodified-Since
    let svc = ServeDir::new(REPO_ROOT);

    // Send If-Match (non-matching, should 412) and If-Unmodified-Since (far future, would pass)
    // If-Match should win and return 412
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MATCH, "\"not-a-real-etag\"")
        .header(header::IF_UNMODIFIED_SINCE, "Sun, 01 Jan 2100 00:00:00 GMT")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PRECONDITION_FAILED);
}

#[tokio::test]
async fn if_none_match_weak_comparison() {
    // Weak comparison: W/"etag" should match "etag" for If-None-Match
    let svc = ServeDir::new(REPO_ROOT);

    // First request to get the ETag
    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let etag = res
        .headers()
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // Send with W/ prefix, should still match via weak comparison
    let svc = ServeDir::new(REPO_ROOT);
    let weak_etag = format!("W/{}", etag);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_NONE_MATCH, &weak_etag)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn if_match_strong_comparison_rejects_weak_etag() {
    // Strong comparison: W/"etag" should NOT match "etag" for If-Match
    let svc = ServeDir::new(REPO_ROOT);

    // First request to get the ETag
    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let etag = res
        .headers()
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // Send with W/ prefix for If-Match, should fail (strong comparison)
    let svc = ServeDir::new(REPO_ROOT);
    let weak_etag = format!("W/{}", etag);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MATCH, &weak_etag)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PRECONDITION_FAILED);
}

#[tokio::test]
async fn if_none_match_multiple_etags() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    let etag = res
        .headers()
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    // One matching among several should still produce 304
    let svc = ServeDir::new(REPO_ROOT);
    let multi = format!("\"bogus\", {}, \"also-bogus\"", etag);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_NONE_MATCH, &multi)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn if_match_wildcard_succeeds() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MATCH, "*")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn etag_on_head_request() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers().get(header::ETAG).is_some());
}

#[tokio::test]
async fn if_modified_since_304_includes_etag() {
    let svc = ServeDir::new(REPO_ROOT);

    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    let last_modified = res.headers().get(header::LAST_MODIFIED).unwrap().clone();
    let etag = res.headers().get(header::ETAG).unwrap().clone();

    // Time-based 304 should also include ETag
    let svc = ServeDir::new(REPO_ROOT);
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MODIFIED_SINCE, &last_modified)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(res.headers().get(header::ETAG).unwrap(), &etag);
    assert_eq!(
        res.headers().get(header::LAST_MODIFIED).unwrap(),
        &last_modified
    );
}

mod memory_backend {
    use super::*;
    use crate::services::fs::serve_dir::backend::{Backend, File, Metadata};
    use std::{
        collections::HashMap, future::Future, io, path::PathBuf, pin::Pin, sync::Arc,
        time::SystemTime,
    };
    use tokio::io::{AsyncRead, AsyncSeek};

    /// In-memory file metadata.
    #[derive(Clone)]
    struct MemMetadata {
        is_dir: bool,
        len: u64,
        modified: SystemTime,
    }

    impl Metadata for MemMetadata {
        fn is_dir(&self) -> bool {
            self.is_dir
        }

        fn modified(&self) -> io::Result<SystemTime> {
            Ok(self.modified)
        }

        fn len(&self) -> u64 {
            self.len
        }
    }

    /// In-memory file backed by a Cursor.
    struct MemFile {
        cursor: std::io::Cursor<Vec<u8>>,
        meta: MemMetadata,
    }

    impl AsyncRead for MemFile {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<io::Result<()>> {
            Pin::new(&mut self.cursor).poll_read(cx, buf)
        }
    }

    impl AsyncSeek for MemFile {
        fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
            Pin::new(&mut self.cursor).start_seek(position)
        }

        fn poll_complete(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<io::Result<u64>> {
            Pin::new(&mut self.cursor).poll_complete(cx)
        }
    }

    impl File for MemFile {
        type Metadata = MemMetadata;
        type MetadataFuture<'a> = std::future::Ready<io::Result<MemMetadata>>;

        fn metadata(&self) -> Self::MetadataFuture<'_> {
            std::future::ready(Ok(self.meta.clone()))
        }
    }

    /// In-memory backend storing files in a HashMap.
    #[derive(Clone)]
    struct MemBackend {
        files: Arc<HashMap<PathBuf, Vec<u8>>>,
        dirs: Arc<Vec<PathBuf>>,
    }

    impl MemBackend {
        fn new() -> Self {
            Self {
                files: Arc::new(HashMap::new()),
                dirs: Arc::new(Vec::new()),
            }
        }

        fn with_file(mut self, path: impl Into<PathBuf>, content: impl Into<Vec<u8>>) -> Self {
            Arc::get_mut(&mut self.files)
                .unwrap()
                .insert(path.into(), content.into());
            self
        }

        fn with_dir(mut self, path: impl Into<PathBuf>) -> Self {
            Arc::get_mut(&mut self.dirs).unwrap().push(path.into());
            self
        }
    }

    impl Backend for MemBackend {
        type File = MemFile;
        type Metadata = MemMetadata;
        type OpenFuture = Pin<Box<dyn Future<Output = io::Result<MemFile>> + Send>>;
        type MetadataFuture = Pin<Box<dyn Future<Output = io::Result<MemMetadata>> + Send>>;

        fn open(&self, path: PathBuf) -> Self::OpenFuture {
            let files = self.files.clone();
            Box::pin(async move {
                match files.get(&path) {
                    Some(data) => Ok(MemFile {
                        meta: MemMetadata {
                            is_dir: false,
                            len: data.len() as u64,
                            modified: SystemTime::UNIX_EPOCH,
                        },
                        cursor: std::io::Cursor::new(data.clone()),
                    }),
                    None => Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
                }
            })
        }

        fn metadata(&self, path: PathBuf) -> Self::MetadataFuture {
            let files = self.files.clone();
            let dirs = self.dirs.clone();
            Box::pin(async move {
                if dirs.contains(&path) {
                    return Ok(MemMetadata {
                        is_dir: true,
                        len: 0,
                        modified: SystemTime::UNIX_EPOCH,
                    });
                }
                match files.get(&path) {
                    Some(data) => Ok(MemMetadata {
                        is_dir: false,
                        len: data.len() as u64,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    None => Err(io::Error::new(io::ErrorKind::NotFound, "not found")),
                }
            })
        }
    }

    #[tokio::test]
    async fn serve_file_from_memory() {
        let backend = MemBackend::new().with_file("./assets/hello.txt", "Hello, world!");

        let svc = ServeDir::with_backend("assets", backend);

        let req = Request::builder()
            .uri("/hello.txt")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "text/plain");

        let body = body_into_text(res.into_body()).await;
        assert_eq!(body, "Hello, world!");
    }

    #[tokio::test]
    async fn not_found_from_memory() {
        let backend = MemBackend::new();

        let svc = ServeDir::with_backend("assets", backend);

        let req = Request::builder()
            .uri("/missing.txt")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn head_request_from_memory() {
        let backend = MemBackend::new().with_file("./assets/hello.txt", "Hello, world!");

        let svc = ServeDir::with_backend("assets", backend);

        let req = Request::builder()
            .method(Method::HEAD)
            .uri("/hello.txt")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-length"], "13");

        // HEAD should have empty body
        let body = body_into_text(res.into_body()).await;
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn range_request_from_memory() {
        let backend = MemBackend::new().with_file("./assets/hello.txt", "Hello, world!");

        let svc = ServeDir::with_backend("assets", backend);

        let req = Request::builder()
            .uri("/hello.txt")
            .header("range", "bytes=0-4")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(res.headers()["content-range"], "bytes 0-4/13");

        let body = body_into_text(res.into_body()).await;
        assert_eq!(body, "Hello");
    }

    #[tokio::test]
    async fn directory_redirect_from_memory() {
        let backend = MemBackend::new()
            .with_dir("./assets/sub")
            .with_file("./assets/sub/index.html", "<h1>Index</h1>");

        let svc = ServeDir::with_backend("assets", backend);

        // Request without trailing slash should redirect
        let req = Request::builder().uri("/sub").body(Body::empty()).unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(res.headers()["location"], "/sub/");
    }

    #[tokio::test]
    async fn directory_serves_index_html_from_memory() {
        let backend = MemBackend::new()
            .with_dir("./assets/sub")
            .with_file("./assets/sub/index.html", "<h1>Index</h1>");

        let svc = ServeDir::with_backend("assets", backend);

        let req = Request::builder().uri("/sub/").body(Body::empty()).unwrap();
        let res = svc.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = body_into_text(res.into_body()).await;
        assert_eq!(body, "<h1>Index</h1>");
    }
}
