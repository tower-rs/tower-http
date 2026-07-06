# custom-middleware-either

Because a Tower `Service` requires a single, concrete `Response` type, you run into a type mismatch if your middleware returns an immediate error body in one branch, but defers to the inner service's body in the other. 

This example solves that problem by using `http_body_util::Either<B, Full<Bytes>>` to gracefully unify the happy-path body and the early-rejection body, all without requiring dynamic allocations (boxing).

## How it works

- **Missing Header:** If the `x-api-key` header is missing, the middleware short-circuits the future using `std::future::ready` and immediately bounces the request with a `400 Bad Request` and a custom error body (`Either::Right`).
- **Valid Request:** If the header is present, the request is passed through to the inner service, which returns a `200 OK` (`Either::Left`).

## Running the example

```
RUST_LOG=custom_middleware_either=trace,tower_http=trace \
    cargo run --bin custom-middleware-either
```
