# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

# Unreleased

- Fix the bug that `ServeDir` can not handle percent encoded uri path correctly.
- Enhancement: avoid using permanent redirection when redirect directory path without a trailing slash to the one has
- Fix a [bug](https://github.com/tower-rs/tower-http/issues/121) which happens when `append_index_html_on_directories` is set to `false` in `ServeDir`.
- Add `Cors` for setting [CORS] headers.

## Breaking changes

None.

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

## Breaking changes

None.

# 0.1.0 (May 27, 2021)

- Initial release.

[CORS]: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS
