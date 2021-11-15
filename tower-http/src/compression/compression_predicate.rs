use crate::compression_utils::default_compression_predicate;

/// A filter which any response Parts needs to pass to be compressed
pub trait CompressionPredicate<B>: Clone {
    /// Predicate which takes response parts and returns true if the response should be compressed
    fn should_compress(&self, response: &http::Response<B>) -> bool;
}

impl<F, B> CompressionPredicate<B> for F where F: Fn(&http::Response<B>) -> bool + Clone {
    fn should_compress(&self, response: &http::Response<B>) -> bool {
        (self)(response)
    }
}

/// Default compression filter that proxies [`default_compression_predicate`] which looks at
/// headers to determine whether compression is suitable
#[derive(Default, Clone)]
#[non_exhaustive]
pub struct DefaultCompressionPredicate;

impl<B> CompressionPredicate<B> for DefaultCompressionPredicate {
    fn should_compress(&self, response: &http::Response<B>) -> bool {
        default_compression_predicate(&response.headers())
    }
}
