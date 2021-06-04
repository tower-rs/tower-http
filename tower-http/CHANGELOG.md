# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

# Unreleased

None.

## Breaking changes

- Change the response body error type of `Compression` and `Decompression` to
  `Box<dyn std::error::Error + Send + Sync>`. This makes them usable if the body
  they're wrapping uses `Box<dyn std::error::Error + Send + Sync>` as its error
  type which they previously weren't.
- Remove `BodyOrIoError`. Its been replaced with `Box<dyn std::error::Error +
  Send + Sync>`.

# 0.1.0 (May 27, 2021)

- Initial release.
