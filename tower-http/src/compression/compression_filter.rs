use crate::compression_utils::default_compression_filter_predicate;

/// A filter which any response Parts needs to pass to be compressed
pub trait CompressionFilter: Copy {
    /// Predicate which takes response parts and returns true if the response should be compressed
    fn should_compress(&self, parts: &http::response::Parts) -> bool;
}

impl<F> CompressionFilter for F where F: Fn(&http::response::Parts) -> bool + Copy {
    fn should_compress(&self, parts: &http::response::Parts) -> bool {
        (self)(parts)
    }
}

/// Default compression filter that proxies `default_compression_filter_predicate` which looks at
/// headers to determine whether compression is suitable
#[derive(Default, Copy, Clone, Debug)]
pub struct DefaultCompressionFilter {

}

impl CompressionFilter for DefaultCompressionFilter {
    fn should_compress(&self, parts: &http::response::Parts) -> bool {
        default_compression_filter_predicate(&parts.headers)
    }
}
