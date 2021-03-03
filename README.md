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
- `MapRequestBody`: Apply a transformation to the request body.
- `MapResponseBody`: Apply a transformation to the response body.
- `PropagateHeader`: Propagate a header from the request to the response.
- `SensitiveHeader`: Marks a given header as [sensitive] so it wont show up in logs.
- `SetRequestHeader`: Set a header on the request.
- `SetResponseHeader`: Set a header on the response.
- `SetSensitiveRequestHeader`: Marks a given request header as [sensitive].
- `SetSensitiveResponseHeader`: Marks a given response header as [sensitive].

Middlewares uses the [`http`] crate as the HTTP interface so they're compatible with any library or framework that also uses [`http`]. For example hyper and actix.

The middlewares were originally extracted from one of [@EmbarkStudios] internal projects.

All middlewares are disabled by default and can be enabled using a cargo feature. The feature `full` turns on everything.

[`http`]: https://crates.io/crates/http
[@EmbarkStudios]: https://github.com/EmbarkStudios
[sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
[request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
