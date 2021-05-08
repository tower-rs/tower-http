# Tower HTTP

**NOTE:** This crate is still under active development and while most of the
initial churn is over, Tower HTTP still isn't released on crates.io. We are
actively working on that and you can follow the progress towards 0.1.0
[here][milestone].

Tower middlewares and utilities for HTTP clients and servers.

[![Build status](https://github.com/tower-rs/tower-http/workflows/CI/badge.svg)](https://github.com/tower-rs/tower-http/actions)
[![Crates.io](https://img.shields.io/crates/v/tower-http)](https://crates.io/crates/tower-http)
[![Documentation](https://docs.rs/tower-http/badge.svg)](https://docs.rs/tower-http)
[![Crates.io](https://img.shields.io/crates/l/tower-http)](LICENSE)

More information about this crate can be found in the [crate documentation][docs].

## Middlewares

Tower HTTP contains lots of middlewares that are generally useful when building
HTTP servers and clients. Some of the highlights are:

- `Trace` adds high level logging of requests and responses. Supports both
  regular HTTP requests as well as gRPC.
- `Compression` and `Decompression` to compress/decompress response bodies.
- `FollowRedirect` to automatically follow redirection responses.

See the [docs] for the complete list of middlewares.

Middlewares uses the [`http`] crate as the HTTP interface so they're compatible
with any library or framework that also uses [`http`]. For example [hyper].

The middlewares were originally extracted from one of [@EmbarkStudios] internal
projects.

## Examples

The [`examples`] folder contains various examples of how to use Tower HTTP:

- [`warp-key-value-store`]: A key/value store with an HTTP API built with warp.
- [`tonic-key-value-store`]: A key/value store with a gRPC API and client built with tonic.

## Getting Help

First, see if the answer to your question can be found in the API documentation.
If the answer is not there, there is an active community in the [Tower Discord
channel][chat]. We would be happy to try to answer your question. If that
doesn't work, try opening an [issue] with the question.

## Contributing

:balloon: Thanks for your help improving the project! We are so happy to have
you! We have a [contributing guide][guide] to help you get involved in the Tower
HTTP project.

[guide]: CONTRIBUTING.md

## License

This project is licensed under the [MIT license](LICENSE).

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Tower HTTP by you, shall be licensed as MIT, without any
additional terms or conditions.

[@EmbarkStudios]: https://github.com/EmbarkStudios
[`examples`]: https://github.com/tower-rs/tower-http/tree/master/examples
[`http`]: https://crates.io/crates/http
[`tonic-key-value-store`]: https://github.com/tower-rs/tower-http/tree/master/examples/tonic-key-value-store
[`warp-key-value-store`]: https://github.com/tower-rs/tower-http/tree/master/examples/warp-key-value-store
[chat]: https://discord.gg/tokio
[docs]: https://docs.rs/tower-http
[hyper]: https://github.com/hyperium/hyper
[issue]: https://github.com/tower-rs/tower-http/issues/new
[milestone]: https://github.com/tower-rs/tower-http/milestones
