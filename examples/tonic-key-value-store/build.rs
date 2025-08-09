fn main() {
    tonic_prost_build::configure()
        .compile_protos(&["key_value_store.proto"], &["proto"])
        .unwrap();
}
