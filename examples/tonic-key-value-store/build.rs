fn main() {
    tonic_build::configure()
        .format(false)
        .compile(&["key_value_store.proto"], &["proto"])
        .unwrap();
}
