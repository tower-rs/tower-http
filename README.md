# Tower HTTP

Tower middlewares and utilities for HTTP clients and servers

[![Build status](https://github.com/tower-rs/tower-http/workflows/CI/badge.svg)](https://github.com/tower-rs/tower-http/actions)

More information about this crate can be found in the [crate documentation][dox].

[dox]: https://tower-rs.github.io/tower-http/tower_http

**This library is not production ready. Do not try to use it in a production
environment or you will regret it!** This crate is still under active
development and there has not yet been any focus on documentation (because you
shouldn't be using it yet!).

## Middlewares

These are the middlewares included in this crate:

- `AddExtension`: Stick some shareable value in [request extensions].
- `Compression`: Compression response bodies.
- `Decompression`: Decompress response bodies.
- `SetResponseHeader`: Set a header on the response.

Middlewares uses the [`http`] crate as the HTTP interface so they're compatible with any library or framework that also uses [`http`]. For example hyper and actix.

The middlewares were originally extracted from one of [@EmbarkStudios] internal projects.

All middlewares are disabled by default and can be enabled using a cargo feature. The feature `full` turns on everything.

[`http`]: https://crates.io/crates/http
[@EmbarkStudios]: https://github.com/EmbarkStudios
