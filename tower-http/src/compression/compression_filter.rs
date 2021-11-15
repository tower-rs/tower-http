use crate::compression_utils::default_compression_filter_predicate;

/// A filter which any response Parts needs to pass to be compressed
pub trait CompressionPredicate: Clone {
    /// Predicate which takes response parts and returns true if the response should be compressed
    fn should_compress(&self, parts: &http::response::Parts) -> bool;
}

impl<F> CompressionPredicate for F where F: Fn(&http::response::Parts) -> bool + Clone {
    fn should_compress(&self, parts: &http::response::Parts) -> bool {
        (self)(parts)
    }
}

/// Default compression filter that proxies `default_compression_filter_predicate` which looks at
/// headers to determine whether compression is suitable
#[derive(Default, Clone)]
#[non_exhaustive]
pub struct DefaultCompressionPredicate;

impl CompressionPredicate for DefaultCompressionPredicate {
    #[inline]
    fn should_compress(&self, parts: &http::response::Parts) -> bool {
        default_compression_filter_predicate(&parts.headers)
    }
}
