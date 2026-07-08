# Custom future with multiple bodies

This example serves to demonstrate how to build a service that returns a
response in case of an event (in this instance, a missing HTTP header) while
leaving the inner service's response untouched.

This requires wrapping the response body if the user wishes to leave the inner
service's body untouched.

## Running the example

```
cargo run -p custom-future-multiple-bodies
```
