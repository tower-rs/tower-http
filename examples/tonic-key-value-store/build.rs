fn main() {
    tonic_build::configure()
        .compile(&["key_value_store.proto"], &["proto"])
        .unwrap();
}
