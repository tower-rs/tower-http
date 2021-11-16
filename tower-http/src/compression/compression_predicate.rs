#![allow(missing_docs)]

use http::{header, Extensions, HeaderMap, StatusCode, Version};
use http_body::Body;
use std::borrow::Cow;

/// A filter which any response Parts needs to pass to be compressed
pub trait CompressionPredicate: Clone {
    /// Predicate which takes response parts and returns true if the response should be compressed
    fn should_compress<B>(&self, response: &http::Response<B>) -> bool
    where
        B: Body;

    fn and<Other>(self, other: Other) -> And<Self, Other>
    where
        Self: Sized,
        Other: CompressionPredicate,
    {
        And {
            lhs: self,
            rhs: other,
        }
    }
}

impl<F> CompressionPredicate for F
where
    F: Fn(StatusCode, Version, &HeaderMap, &Extensions) -> bool + Clone,
{
    fn should_compress<B>(&self, response: &http::Response<B>) -> bool
    where
        B: Body,
    {
        let status = response.status();
        let version = response.version();
        let headers = response.headers();
        let extensions = response.extensions();
        self(status, version, headers, extensions)
    }
}

impl<T> CompressionPredicate for Option<T>
where
    T: CompressionPredicate,
{
    fn should_compress<B>(&self, response: &http::Response<B>) -> bool
    where
        B: Body,
    {
        self.as_ref()
            .map(|inner| inner.should_compress(response))
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, Default, Copy)]
pub struct And<Lhs, Rhs> {
    lhs: Lhs,
    rhs: Rhs,
}

impl<Lhs, Rhs> CompressionPredicate for And<Lhs, Rhs>
where
    Lhs: CompressionPredicate,
    Rhs: CompressionPredicate,
{
    fn should_compress<B>(&self, response: &http::Response<B>) -> bool
    where
        B: Body,
    {
        self.lhs.should_compress(response) && self.rhs.should_compress(response)
    }
}

#[derive(Clone)]
pub struct DefaultCompressionPredicate(And<And<SizeAbove, NotForContentType>, NotForContentType>);

impl Default for DefaultCompressionPredicate {
    fn default() -> Self {
        let inner = SizeAbove::new(SizeAbove::DEFAULT_MIN_SIZE)
            .and(NotForContentType::GRPC)
            .and(NotForContentType::IMAGES);
        Self(inner)
    }
}

impl CompressionPredicate for DefaultCompressionPredicate {
    fn should_compress<B>(&self, response: &http::Response<B>) -> bool
    where
        B: Body,
    {
        self.0.should_compress(response)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SizeAbove(u16);

impl SizeAbove {
    const DEFAULT_MIN_SIZE: u16 = 32;

    pub const fn new(min_size_bytes: u16) -> Self {
        Self(min_size_bytes)
    }
}

impl Default for SizeAbove {
    fn default() -> Self {
        Self(Self::DEFAULT_MIN_SIZE)
    }
}

impl CompressionPredicate for SizeAbove {
    fn should_compress<B>(&self, response: &http::Response<B>) -> bool
    where
        B: Body,
    {
        let content_size = response.body().size_hint().exact().or_else(|| {
            response
                .headers()
                .get(header::CONTENT_LENGTH)
                .and_then(|h| h.to_str().ok())
                .and_then(|val| val.parse().ok())
        });

        match content_size {
            Some(size) => size >= (self.0 as u64),
            _ => true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NotForContentType(Cow<'static, str>);

impl NotForContentType {
    const GRPC: Self = Self::const_new("application/grpc");
    const IMAGES: Self = Self::const_new("image/");

    pub fn new(content_type: impl Into<Cow<'static, str>>) -> Self {
        Self(content_type.into())
    }

    pub const fn const_new(content_type: &'static str) -> Self {
        Self(Cow::Borrowed(content_type))
    }
}

impl CompressionPredicate for NotForContentType {
    fn should_compress<B>(&self, response: &http::Response<B>) -> bool
    where
        B: Body,
    {
        !content_type(response).starts_with(&*self.0)
    }
}

fn content_type<B>(response: &http::Response<B>) -> &str {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
}
