# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

# Unreleased

- `ServeDir` and `ServeFile`: Ability to serve precompressed files ([#156])
- `Trace`: Add `DefaultMakeSpan::level` to make log level of tracing spans easily configurable ([#124])
- Change the response body error type of `Compression` and `Decompression` to
  `Box<dyn std::error::Error + Send + Sync>`. This makes them usable if the body
  they're wrapping uses `Box<dyn std::error::Error + Send + Sync>` as its error
  type which they previously weren't ([#166])
- Remove `BodyOrIoError`. Its been replaced with `Box<dyn std::error::Error +
  Send + Sync>` ([#166])
- `SetRequestHeaderLayer`, `SetResponseHeaderLayer`: Remove unnecessary generic parameter ([#148])
  This removes the need (and possibility) to specify a body type for these layers.
- Remove the `compression` and `decompression` feature. They were unnecessary
  and `compression-full`/`decompression-full` can be used to get full
  compression/decompression support. For more granular control `[compression|decompression]-gzip`, 
  `[compression|decompression]-br` and `[compression|decompression]-deflate` may
  be used instead. ([#170])
- Add `ServiceBuilderExt` which adds methods to `tower::ServiceBuilder` for
  adding middleware from tower-http.
- Add `SetRequestId` and `PropagateRequestId` middleware ([#150])

[#124]: https://github.com/tower-rs/tower-http/pull/124
[#148]: https://github.com/tower-rs/tower-http/pull/148
[#150]: https://github.com/tower-rs/tower-http/pull/150
[#156]: https://github.com/tower-rs/tower-http/pull/156
[#166]: https://github.com/tower-rs/tower-http/pull/166
[#170]: https://github.com/tower-rs/tower-http/pull/170

# 0.1.2 (November 13, 2021)

- New middleware: Add `Cors` for setting [CORS] headers ([#112])
- New middleware: Add `AsyncRequireAuthorization` ([#118])
- `Compression`: Don't recompress HTTP responses ([#140])
- `Compression` and `Decompression`: Pass configuration from layer into middleware ([#132])
- `ServeDir` and `ServeFile`: Improve performance ([#137])
- `Compression`: Remove needless `ResBody::Error: Into<BoxError>` bounds ([#117])
- `ServeDir`: Percent decode path segments ([#129])
- `ServeDir`: Use correct redirection status ([#130])
- `ServeDir`: Return `404 Not Found` on requests to directories if
  `append_index_html_on_directories` is set to `false` ([#122])

[#112]: https://github.com/tower-rs/tower-http/pull/112
[#118]: https://github.com/tower-rs/tower-http/pull/118
[#140]: https://github.com/tower-rs/tower-http/pull/140
[#132]: https://github.com/tower-rs/tower-http/pull/132
[#137]: https://github.com/tower-rs/tower-http/pull/137
[#117]: https://github.com/tower-rs/tower-http/pull/117
[#129]: https://github.com/tower-rs/tower-http/pull/129
[#130]: https://github.com/tower-rs/tower-http/pull/130
[#122]: https://github.com/tower-rs/tower-http/pull/122

# 0.1.1 (July 2, 2021)

- Add example of using `SharedClassifier`.
- Add `StatusInRangeAsFailures` which is a response classifier that considers
  responses with status code in a certain range as failures. Useful for HTTP
  clients where both server errors (5xx) and client errors (4xx) are considered
  failures.
- Implement `Debug` for `NeverClassifyEos`.
- Update iri-string to 0.4.
- Add `ClassifyResponse::map_failure_class` and `ClassifyEos::map_failure_class`
  for transforming the failure classification using a function.
- Clarify exactly when each `Trace` callback is called.
- Add `AddAuthorizationLayer` for setting the `Authorization` header on
  requests.

# 0.1.0 (May 27, 2021)

- Initial release.

[CORS]: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS
