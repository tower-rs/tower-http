# warp-key-value-store

This examples contains a simple key/value store with an HTTP API built using warp.

## Endpoints

- `GET /:key` - Look up a key. If the key doesn't exist it returns `400 Not Found`
- `POST /:key` - Insert a key. The value is the request body.

## Running the example

```
cargo run --bin warp-key-value-store
```