# Custom future with multiple bodies

This example serves to demonstrate how to build a middleware service that
returns a concrete response in case of an event (in this instance, a missing
HTTP header) while leaving the inner service's response untouched.

This requires wrapping the response body if the user wishes to leave the inner
service's body untouched.

This solution is geared towards application code, library author's should
consider studying [tower_http/limit's implementation instead](https://github.com/tower-rs/tower-http/tree/main/tower-http/src/limit), since `Either` erases the error type.

## Running the example

```
cargo run -p custom-future-multiple-bodies
```
