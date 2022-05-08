use crate::services::{ServeDir, ServeFile};
use brotli::BrotliDecompress;
use bytes::Bytes;
use flate2::bufread::{DeflateDecoder, GzDecoder};
use http::header::ALLOW;
use http::{header, Method, Response};
use http::{Request, StatusCode};
use http_body::Body as HttpBody;
use hyper::Body;
use std::io::{self, Read};
use tower::ServiceExt;

#[tokio::test]
async fn basic() {
    let svc = ServeDir::new("..");

    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/markdown");

    let body = body_into_text(res.into_body()).await;

    let contents = std::fs::read_to_string("../README.md").unwrap();
    assert_eq!(body, contents);
}

#[tokio::test]
async fn basic_with_index() {
    let svc = ServeDir::new("../test-files");

    let req = Request::new(Body::empty());
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/html");

    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "<b>HTML!</b>\n");
}

#[tokio::test]
async fn head_request() {
    let svc = ServeDir::new("../test-files");

    let req = Request::builder()
        .uri("/precompressed.txt")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-length"], "23");

    let body = res.into_body().data().await;
    assert!(body.is_none());
}

#[tokio::test]
async fn precompresed_head_request() {
    let svc = ServeDir::new("../test-files").precompressed_gzip();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "gzip")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "gzip");
    assert_eq!(res.headers()["content-length"], "59");

    let body = res.into_body().data().await;
    assert!(body.is_none());
}

#[tokio::test]
async fn with_custom_chunk_size() {
    let svc = ServeDir::new("..").with_buf_chunk_size(1024 * 32);

    let req = Request::builder()
        .uri("/README.md")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/markdown");

    let body = body_into_text(res.into_body()).await;

    let contents = std::fs::read_to_string("../README.md").unwrap();
    assert_eq!(body, contents);
}

#[tokio::test]
async fn precompressed_gzip() {
    let svc = ServeDir::new("../test-files").precompressed_gzip();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-encoding"], "gzip");

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decoder = GzDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert!(decompressed.starts_with("\"This is a test file!\""));
}

#[tokio::test]
async fn precompressed_br() {
    let svc = ServeDir::new("../test-files").precompressed_br();

    let req = Request::builder()
        .uri("/precompressed.txt")
        .header("Accept-Encoding", "br")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

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
    let svc = ServeDir::new("../test-files").precompressed_deflate();
    let request = Request::builder()
        .uri("/precompressed.txt")
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
async fn unsupported_precompression_alogrithm_fallbacks_to_uncompressed() {
    let svc = ServeDir::new("../test-files").precompressed_gzip();

    let request = Request::builder()
        .uri("/precompressed.txt")
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
async fn only_precompressed_variant_existing() {
    let svc = ServeDir::new("../test-files").precompressed_gzip();

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

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decoder = GzDecoder::new(&body[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert!(decompressed.starts_with("\"This is a test file\""));
}

#[tokio::test]
async fn missing_precompressed_variant_fallbacks_to_uncompressed() {
    let svc = ServeDir::new("../test-files").precompressed_gzip();

    let request = Request::builder()
        .uri("/missing_precompressed.txt")
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
async fn missing_precompressed_variant_fallbacks_to_uncompressed_for_head_request() {
    let svc = ServeDir::new("../test-files").precompressed_gzip();

    let request = Request::builder()
        .uri("/missing_precompressed.txt")
        .header("Accept-Encoding", "gzip")
        .method(Method::HEAD)
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();

    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.headers()["content-length"], "11");
    // Uncompressed file is served because compressed version is missing
    assert!(res.headers().get("content-encoding").is_none());

    assert!(res.into_body().data().await.is_none());
}

#[tokio::test]
async fn access_to_sub_dirs() {
    let svc = ServeDir::new("..");

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
    let svc = ServeDir::new("..");

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

#[tokio::test]
async fn not_found_precompressed() {
    let svc = ServeDir::new("../test-files").precompressed_gzip();

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
    let svc = ServeDir::new("../test-files")
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

    assert!(res.into_body().data().await.is_none());
}

#[tokio::test]
async fn fallbacks_to_different_precompressed_variant_if_not_found() {
    let svc = ServeDir::new("../test-files")
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

    let body = res.into_body().data().await.unwrap().unwrap();
    let mut decompressed = Vec::new();
    BrotliDecompress(&mut &body[..], &mut decompressed).unwrap();
    let decompressed = String::from_utf8(decompressed.to_vec()).unwrap();
    assert!(decompressed.starts_with("Test file"));
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
async fn empty_directory_without_index() {
    let svc = ServeDir::new(".").append_index_html_on_directories(false);

    let req = Request::new(Body::empty());
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
    let bytes = hyper::body::to_bytes(body).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn access_cjk_percent_encoded_uri_path() {
    // percent encoding present of 你好世界.txt
    let cjk_filename_encoded = "%E4%BD%A0%E5%A5%BD%E4%B8%96%E7%95%8C.txt";

    let svc = ServeDir::new("../test-files");

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

    let svc = ServeDir::new("../test-files");

    let req = Request::builder()
        .uri(format!("/{}", encoded_filename))
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/plain");
}

#[tokio::test]
async fn read_partial_in_bounds() {
    let svc = ServeDir::new("..");
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

    let file_contents = std::fs::read("../README.md").unwrap();
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

    let body = hyper::body::to_bytes(res.into_body()).await.ok().unwrap();
    let source = Bytes::from(file_contents[bytes_start_incl..=bytes_end_incl].to_vec());
    assert_eq!(body, source);
}

#[tokio::test]
async fn read_partial_rejects_out_of_bounds_range() {
    let svc = ServeDir::new("..");
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

    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let file_contents = std::fs::read("../README.md").unwrap();
    assert_eq!(
        res.headers()["content-range"],
        &format!("bytes */{}", file_contents.len())
    )
}

#[tokio::test]
async fn read_partial_errs_on_garbage_header() {
    let svc = ServeDir::new("..");
    let req = Request::builder()
        .uri("/README.md")
        .header("Range", "bad_format")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let file_contents = std::fs::read("../README.md").unwrap();
    assert_eq!(
        res.headers()["content-range"],
        &format!("bytes */{}", file_contents.len())
    )
}

#[tokio::test]
async fn read_partial_errs_on_bad_range() {
    let svc = ServeDir::new("..");
    let req = Request::builder()
        .uri("/README.md")
        .header("Range", "bytes=-1-15")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let file_contents = std::fs::read("../README.md").unwrap();
    assert_eq!(
        res.headers()["content-range"],
        &format!("bytes */{}", file_contents.len())
    )
}
#[tokio::test]
async fn last_modified() {
    let svc = ServeDir::new("..");
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

    let svc = ServeDir::new("..");
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MODIFIED_SINCE, last_modified)
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    let body = res.into_body().data().await;
    assert!(body.is_none());

    let svc = ServeDir::new("..");
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_MODIFIED_SINCE, "Fri, 09 Aug 1996 14:21:40 GMT")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let readme_bytes = include_bytes!("../../../../../README.md");
    let body = res.into_body().data().await.unwrap().unwrap();
    assert_eq!(body.as_ref(), readme_bytes);

    // -- If-Unmodified-Since

    let svc = ServeDir::new("..");
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_UNMODIFIED_SINCE, last_modified)
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().data().await.unwrap().unwrap();
    assert_eq!(body.as_ref(), readme_bytes);

    let svc = ServeDir::new("..");
    let req = Request::builder()
        .uri("/README.md")
        .header(header::IF_UNMODIFIED_SINCE, "Fri, 09 Aug 1996 14:21:40 GMT")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PRECONDITION_FAILED);
    let body = res.into_body().data().await;
    assert!(body.is_none());
}

#[tokio::test]
async fn with_fallback_svc() {
    async fn fallback<B>(req: Request<B>) -> io::Result<Response<Body>> {
        Ok(Response::new(Body::from(format!(
            "from fallback {}",
            req.uri().path()
        ))))
    }

    let svc = ServeDir::new("..").fallback(tower::service_fn(fallback));

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
    let svc = ServeDir::new("..").fallback(ServeFile::new("../README.md"));

    let req = Request::builder()
        .uri("/doesnt-exist")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/markdown");

    let body = body_into_text(res.into_body()).await;

    let contents = std::fs::read_to_string("../README.md").unwrap();
    assert_eq!(body, contents);
}

#[tokio::test]
async fn method_not_allowed() {
    let svc = ServeDir::new("..");

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
    async fn fallback<B>(req: Request<B>) -> io::Result<Response<Body>> {
        Ok(Response::new(Body::from(format!(
            "from fallback {}",
            req.uri().path()
        ))))
    }

    let svc = ServeDir::new("..")
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
async fn with_fallback_svc_and_not_append_index_html_on_directories() {
    async fn fallback<B>(req: Request<B>) -> io::Result<Response<Body>> {
        Ok(Response::new(Body::from(format!(
            "from fallback {}",
            req.uri().path()
        ))))
    }

    let svc = ServeDir::new("..")
        .append_index_html_on_directories(false)
        .fallback(tower::service_fn(fallback));

    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let body = body_into_text(res.into_body()).await;
    assert_eq!(body, "from fallback /");
}
