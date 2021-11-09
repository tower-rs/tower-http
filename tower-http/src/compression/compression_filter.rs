use crate::compression_utils::default_compression_filter_predicate;

/// A filter which any response Parts needs to pass to be compressed
pub trait CompressionFilter {
    /// Predicate which takes response parts and returns true if the response should be compressed
    fn filter_response(&self, parts: &http::response::Parts) -> bool;
}

/// Default compression filter that proxies `default_compression_filter_predicate` which looks at
/// headers to determine whether compression is suitable
#[derive(Default, Copy, Clone, Debug)]
pub struct DefaultCompressionFilter {

}

impl<F> CompressionFilter for F where F: Fn(&http::response::Parts) -> bool {
    fn filter_response(&self, parts: &http::response::Parts) -> bool {
        (self)(parts)
    }
}

impl CompressionFilter for DefaultCompressionFilter {
    fn filter_response(&self, parts: &http::response::Parts) -> bool {
        default_compression_filter_predicate(&parts.headers)
    }
}
