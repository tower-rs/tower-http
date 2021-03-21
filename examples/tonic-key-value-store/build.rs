fn main() {
    tonic_build::compile_protos("proto/key_value_store.proto").unwrap();
}
