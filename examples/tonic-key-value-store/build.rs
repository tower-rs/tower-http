fn main() {
    let fds = protox::compile(["key_value_store.proto"], ["proto"]).unwrap();
    tonic_prost_build::compile_fds(fds).unwrap();
}
