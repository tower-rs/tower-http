# tonic-key-value-store

This examples contains a simple key/value store with a gRPC API and client built with tonic.

## Running the example

Running a server:

```
RUST_LOG=tonic_key_value_store=trace,tower_http=trace \
    cargo run --bin tonic-key-value-store -- -p 3000 server
```

Setting values:

```
echo "Hello, World" | RUST_LOG=tower_http=trace cargo run --bin tonic-key-value-store -- -p 3000 set -k foo
```

Getting values:

```
RUST_LOG=tower_http=trace cargo run --bin tonic-key-value-store -- -p 3000 get -k foo
```

Create a stream of new keys:

```
RUST_LOG=tower_http=trace cargo run --bin tonic-key-value-store -- -p 3000 subscribe
```
