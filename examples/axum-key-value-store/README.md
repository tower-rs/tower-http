# axum-key-value-store

This examples contains a simple key/value store with an HTTP API built using axum.

## Endpoints

- `GET /:key` - Look up a key. If the key doesn't exist it returns `404 Not Found`
- `POST /:key` - Insert a key. The value is the request body.

## Running the example

```
RUST_LOG=axum_key_value_store=trace,tower_http=trace \
    cargo run --bin axum-key-value-store
```

## Enabling OpenTelemetry

Run Jaeger, or whatever trace collector you want to use:

```
docker run -d -p6831:6831/udp -p6832:6832/udp -p16686:16686 jaegertracing/all-in-one:latest
```

Go to `localhost:16686`.

Run the application with OpenTelemetry enabled:

```
RUST_LOG=axum_key_value_store=trace,tower_http=trace \
    cargo run --bin axum-key-value-store -- --otel
```

