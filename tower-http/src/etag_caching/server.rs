//! ETag caching middleware for use in servers.
//!
//! See [MDN] for more details on ETag.
//!
//! Note that this [`ETagCaching`] will set ETags on requests with other methods than HEAD or GET.
//! [`ComputeEtag::compute`] should return `None` if the request should never be cached.
//!
//! TODO(david): Example
//!
//! [MDN]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/ETag

use futures_core::ready;
use http::{
    header::{ETAG, IF_NONE_MATCH, RANGE},
    HeaderValue, Request, Response, StatusCode,
};
use http_body::{combinators::BoxBody, Body, Empty};
use pin_project::pin_project;
use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// [`Layer`] for computing ETags and checking if resources have changed since they were last
/// requested.
///
/// See the [module docs](crate::etag_caching::server) for more details.
#[derive(Debug, Clone)]
pub struct ETagCachingLayer<T> {
    compute_etag: T,
    comparison_context: ComparisonContext,
}

impl<T> ETagCachingLayer<T> {
    /// Create a new [`ETagCachingLayer`].
    ///
    /// `compute_etag` is expected to implement [`ComputeEtag`].
    pub fn new(compute_etag: T, comparison_context: ComparisonContext) -> Self {
        ETagCachingLayer {
            compute_etag,
            comparison_context,
        }
    }
}

impl<T, S> Layer<S> for ETagCachingLayer<T>
where
    T: Clone,
{
    type Service = ETagCaching<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        ETagCaching::new(inner, self.compute_etag.clone(), self.comparison_context)
    }
}

/// Middleware for computing ETags and checking if resources have changed since they were last
/// requested.
///
/// See the [module docs](crate::etag_caching::server) for more details.
#[derive(Debug, Clone)]
pub struct ETagCaching<S, T> {
    inner: S,
    compute_etag: T,
    comparison_context: ComparisonContext,
}

impl<S, T> ETagCaching<S, T> {
    /// Create a new [`ETagCaching`].
    ///
    /// `compute_etag` is expected to implement [`ComputeEtag`].
    pub fn new(inner: S, compute_etag: T, comparison_context: ComparisonContext) -> Self {
        Self {
            inner,
            compute_etag,
            comparison_context,
        }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with an `ETagCachingLayer` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(compute_etag: T, comparison_context: ComparisonContext) -> ETagCachingLayer<T> {
        ETagCachingLayer::new(compute_etag, comparison_context)
    }
}

impl<S, T, ReqBody, ResBody> Service<Request<ReqBody>> for ETagCaching<S, T>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ResBody: Body + Send + Sync + 'static,
    T: ComputeEtag<ReqBody>,
{
    type Response = Response<BoxBody<ResBody::Data, ResBody::Error>>;
    type Error = S::Error;
    type Future = ResponseFuture<S, Request<ReqBody>, T::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let clone = self.inner.clone();
        let inner = std::mem::replace(&mut self.inner, clone);

        ResponseFuture {
            state: State::ComputingETag {
                future: self.compute_etag.compute(&req),
                svc: Some(inner),
                req: Some(req),
                comparison_context: self.comparison_context,
            },
        }
    }
}

/// Response future for [`ETagCaching`].
#[pin_project]
pub struct ResponseFuture<S, R, ETagF>
where
    S: Service<R>,
{
    #[pin]
    state: State<S, R, ETagF>,
}

#[pin_project(project = StateProj)]
enum State<S, R, ETagF>
where
    S: Service<R>,
{
    ComputingETag {
        #[pin]
        future: ETagF,
        svc: Option<S>,
        req: Option<R>,
        comparison_context: ComparisonContext,
    },
    InnerCall {
        #[pin]
        future: S::Future,
        etag: Option<ETag>,
    },
}

impl<S, ReqBody, ETagF, ResBody> Future for ResponseFuture<S, Request<ReqBody>, ETagF>
where
    ETagF: Future<Output = Option<ETag>>,
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Body + Send + Sync + 'static,
{
    type Output = Result<Response<BoxBody<ResBody::Data, ResBody::Error>>, S::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let mut this = self.as_mut().project();

            let new_state = match this.state.as_mut().project() {
                StateProj::ComputingETag {
                    future,
                    svc,
                    req,
                    comparison_context,
                } => {
                    let etag = ready!(future.poll(cx));

                    let req = req.take().unwrap();

                    if precondition_matches(etag, &req, *comparison_context) {
                        let res = Response::builder()
                            .status(StatusCode::NOT_MODIFIED)
                            .body(Empty::new().map_err(|_: Infallible| unreachable!()).boxed())
                            .unwrap();
                        return Poll::Ready(Ok(res));
                    }

                    let mut svc = svc.take().unwrap();
                    State::InnerCall {
                        future: svc.call(req),
                        etag,
                    }
                }

                StateProj::InnerCall { future, etag } => {
                    let result = ready!(future.poll(cx)).map(|mut res| {
                        if let Some(etag) = etag {
                            res.headers_mut().insert(ETAG, HeaderValue::from(*etag));
                        }
                        res.map(Body::boxed)
                    });
                    return Poll::Ready(result);
                }
            };

            this.state.set(new_state);
        }
    }
}

// TODO(david): tests for this
fn precondition_matches<B>(
    etag: Option<ETag>,
    req: &Request<B>,
    comparison_context: ComparisonContext,
) -> bool {
    use std::iter::once;

    // weak validators cannot be used with range requests
    // RFC 7232 top of page 6
    if let (Some(ETag::Weak(_)), true) = (etag, req.headers().contains_key(RANGE)) {
        return false;
    }

    let if_none_match = if let Some(if_none_match) = req
        .headers()
        .get(IF_NONE_MATCH)
        .and_then(IfNoneMatch::from_header)
    {
        if_none_match
    } else {
        return false;
    };

    match (comparison_context, etag, if_none_match) {
        (_, Some(_), IfNoneMatch::Any) => false,
        (_, None, IfNoneMatch::Any) => true,

        (_, None, _) => false,

        (ComparisonContext::Strong, Some(ETag::Weak(_)), _) => false,

        (ComparisonContext::Strong, Some(ETag::Strong(etag)), IfNoneMatch::Single(single)) => {
            compare_etag_fields(etag, once(single))
        }
        (ComparisonContext::Strong, Some(ETag::Strong(etag)), IfNoneMatch::List(list)) => {
            compare_etag_fields(etag, list.into_iter())
        }

        (ComparisonContext::Weak, Some(etag), IfNoneMatch::Single(single)) => {
            compare_etag_fields(etag.value(), once(single))
        }
        (ComparisonContext::Weak, Some(etag), IfNoneMatch::List(list)) => {
            compare_etag_fields(etag.value(), list.into_iter())
        }
    }
}

enum IfNoneMatch {
    Single(IfNoneMatchField),
    List(Vec<IfNoneMatchField>),
    Any,
}

enum IfNoneMatchField {
    Strong(u64),
    Weak(u64),
}

impl IfNoneMatch {
    fn from_header(value: &HeaderValue) -> Option<Self> {
        let s = value.to_str().ok()?;

        if s == "*" {
            return Some(IfNoneMatch::Any);
        }

        let mut split = s.split(',').map(|part| part.trim()).peekable();

        let mut first_field = Some(split.next()?);
        let mut fields = Vec::new();

        if split.peek().is_some() {
            if let Some(first_field) = first_field.take() {
                fields.push(parse_if_none_match_field(first_field)?);
            }

            for field in split {
                fields.push(parse_if_none_match_field(field)?);
            }

            Some(IfNoneMatch::List(fields))
        } else {
            let first_field = parse_if_none_match_field(first_field.take().unwrap())?;
            Some(IfNoneMatch::Single(first_field))
        }
    }
}

fn parse_if_none_match_field(s: &str) -> Option<IfNoneMatchField> {
    if let Some(weak) = s.strip_prefix("W\"") {
        let weak = weak.strip_suffix("\"")?;
        let weak = weak.parse().ok()?;
        return Some(IfNoneMatchField::Weak(weak));
    }

    if let Some(strong) = s.strip_prefix("\"") {
        let strong = strong.strip_suffix("\"")?;
        let weak = strong.parse().ok()?;
        return Some(IfNoneMatchField::Strong(weak));
    }

    None
}

fn compare_etag_fields<I>(etag: u64, mut if_none_match: I) -> bool
where
    I: Iterator<Item = IfNoneMatchField>,
{
    if_none_match.any(|if_none_match| match if_none_match {
        IfNoneMatchField::Weak(value) => etag == value,
        IfNoneMatchField::Strong(value) => etag == value,
    })
}

/// The context used for controlling whether weak validators are allowed when comparing ETags.
///
/// See [RFC 7232 section 2.3.2][rfc] for more details.
///
/// [rfc]: https://tools.ietf.org/html/rfc7232#section-2.3.2
#[derive(Clone, Copy, Debug)]
pub enum ComparisonContext {
    /// Only strong validators will be accepted.
    Strong,
    /// Both weak and strong validators are accepted.
    Weak,
}

/// An entity-tag that acts as the fingerprint of a resource.
///
/// When a resource at some URL changes so does the ETag. That allows it to be used for caching
/// resources on the client and saving server bandwidth. See [MDN] for more details.
///
/// [MDN]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/ETag
#[derive(Debug, Clone, Copy)]
pub enum ETag {
    /// Indicates that a strong validator was used and that the resource is byte-for-byte identical
    /// if the ETag matches.
    Strong(u64),

    /// Indicates that a weak validator was used and that the resource will be semantically
    /// equivalent but might not be byte-for-byte identical.
    Weak(u64),
}

impl ETag {
    fn value(self) -> u64 {
        match self {
            ETag::Strong(inner) => inner,
            ETag::Weak(inner) => inner,
        }
    }
}

impl From<ETag> for HeaderValue {
    fn from(etag: ETag) -> Self {
        match etag {
            ETag::Strong(hash) => HeaderValue::from_str(&format!("\"{}\"", hash)).unwrap(),
            ETag::Weak(hash) => HeaderValue::from_str(&format!("W\"{}\"", hash)).unwrap(),
        }
    }
}

/// Trait for computing ETags from requests.
pub trait ComputeEtag<B> {
    /// The future type that fields the ETag.
    type Future: Future<Output = Option<ETag>>;

    /// Compute the ETag.
    ///
    /// Note that this [`ETagCaching`] will set ETags on requests with other methods than HEAD or
    /// GET. This method should return `None` if the request should never be cached.
    fn compute(&mut self, request: &Request<B>) -> Self::Future;
}

impl<B, F, Fut> ComputeEtag<B> for F
where
    F: FnMut(&Request<B>) -> Fut,
    Fut: Future<Output = Option<ETag>>,
{
    type Future = Fut;

    fn compute(&mut self, request: &Request<B>) -> Self::Future {
        self(request)
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use futures::future::BoxFuture;
    use hyper::Body;
    use std::hash::{BuildHasher, Hash};
    use std::{collections::hash_map::RandomState, hash::Hasher};
    use tokio::fs;
    use tower::{BoxError, Service, ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn works() {
        let mut svc = ServiceBuilder::new()
            .layer(ETagCachingLayer::new(
                ETagFromFileModificationDate {
                    state: RandomState::new(),
                },
                ComparisonContext::Weak,
            ))
            .service_fn(handle);

        let mut res = svc
            .ready()
            .await
            .unwrap()
            .call(Request::new(Body::empty()))
            .await
            .unwrap();

        let etag = res.headers_mut().remove(ETAG).unwrap();

        let res = svc
            .ready()
            .await
            .unwrap()
            .call(
                Request::builder()
                    .header(IF_NONE_MATCH, etag.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);

        let contents = fs::read("Cargo.toml").await.unwrap();
        fs::write("Cargo.toml", contents).await.unwrap();

        let res = svc
            .ready()
            .await
            .unwrap()
            .call(
                Request::builder()
                    .header(IF_NONE_MATCH, etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
    }

    async fn handle(_req: Request<Body>) -> Result<Response<Body>, BoxError> {
        let contents = fs::read_to_string("Cargo.toml").await?;
        Ok(Response::new(Body::from(contents)))
    }

    #[derive(Clone)]
    struct ETagFromFileModificationDate {
        state: RandomState,
    }

    impl<B> ComputeEtag<B> for ETagFromFileModificationDate {
        type Future = BoxFuture<'static, Option<ETag>>;

        fn compute(&mut self, _request: &Request<B>) -> Self::Future {
            let state = self.state.clone();

            Box::pin(async move {
                let metadata = fs::metadata("Cargo.toml").await.ok()?;

                let mut hasher = state.build_hasher();
                metadata.modified().ok()?.hash(&mut hasher);
                let hash = hasher.finish();

                Some(ETag::Strong(hash))
            })
        }
    }
}
