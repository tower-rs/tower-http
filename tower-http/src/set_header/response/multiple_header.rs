use http::{header::HeaderName, Request, Response};
use pin_project_lite::pin_project;
use std::{
    fmt,
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

use crate::set_header::{InsertHeaderMode, MakeHeaderValue};

pub trait HeaderMetadataGenerator<M> {
    fn to_metadata(self) -> HeaderMetadata<M>;
}

/// Metadata describing a response header to be set by [`SetMultipleResponseHeadersLayer`] or [`SetMultipleResponseHeader`].
#[derive(Clone, Debug)]
pub struct HeaderMetadata<M> {
    /// The name of the header to set.
    pub header_name: HeaderName,
    /// The value or value factory for the header.
    pub make: M,
}

impl<M> HeaderMetadataGenerator<M> for HeaderMetadata<M> {
    fn to_metadata(self) -> HeaderMetadata<M> {
        self
    }
}

impl<M> HeaderMetadataGenerator<M> for (HeaderName, M) {
    fn to_metadata(self) -> HeaderMetadata<M> {
        HeaderMetadata {
            header_name: self.0,
            make: self.1,
        }
    }
}

impl<M> HeaderMetadata<M> {
    /// Convert this metadata into a [`HeaderInsertionConfig`] with the given insertion mode.
    fn convert_to_header_config(self, mode: InsertHeaderMode) -> HeaderInsertionConfig<M> {
        HeaderInsertionConfig {
            header_name: self.header_name,
            make: self.make,
            mode,
        }
    }
}

#[derive(Clone, Debug)]
struct HeaderInsertionConfig<M> {
    header_name: HeaderName,
    make: M,
    mode: InsertHeaderMode,
}

/// Layer that applies [`SetMultipleResponseHeader`] which adds multiple response headers.
///
/// See [`SetMultipleResponseHeader`] for more details.
pub struct SetMultipleResponseHeadersLayer<M> {
    headers: Vec<HeaderInsertionConfig<M>>,
}

impl<M> fmt::Debug for SetMultipleResponseHeadersLayer<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for metadata in &self.headers {
            f.debug_struct("SetMultipleResponseHeadersLayer")
                .field("header_name", &metadata.header_name)
                .field("mode", &metadata.mode)
                .field("make", &std::any::type_name::<M>())
                .finish()?;
        }

        Ok(())
    }
}

impl<M> SetMultipleResponseHeadersLayer<M> {
    /// Create a new [`SetMultipleResponseHeadersLayer`] that overrides any existing values for the same header.
    ///
    /// If any previous value exists for the same header, it is removed and replaced with the new matching header value.
    pub fn overriding(metadata: Vec<impl HeaderMetadataGenerator<M>>) -> Self {
        let headers: Vec<HeaderInsertionConfig<M>> = metadata
            .into_iter()
            .map(|m| {
                m.to_metadata()
                    .convert_to_header_config(InsertHeaderMode::Override)
            })
            .collect();

        Self::new(headers)
    }

    /// Create a new [`SetMultipleResponseHeadersLayer`] that appends header values.
    ///
    /// The new header is always added, preserving any existing values. If previous values exist, the header will have multiple values.
    pub fn appending(metadata: Vec<impl HeaderMetadataGenerator<M>>) -> Self {
        let headers: Vec<HeaderInsertionConfig<M>> = metadata
            .into_iter()
            .map(|m| {
                m.to_metadata()
                    .convert_to_header_config(InsertHeaderMode::Append)
            })
            .collect();

        Self::new(headers)
    }

    /// Create a new [`SetMultipleResponseHeadersLayer`] that only inserts if the header is not already present.
    ///
    /// If a previous value exists for the header, the new value is not inserted.
    pub fn if_not_present(metadata: Vec<impl HeaderMetadataGenerator<M>>) -> Self {
        let headers: Vec<HeaderInsertionConfig<M>> = metadata
            .into_iter()
            .map(|m| {
                m.to_metadata()
                    .convert_to_header_config(InsertHeaderMode::IfNotPresent)
            })
            .collect();

        Self::new(headers)
    }

    /// Internal constructor for a new [`SetMultipleResponseHeadersLayer`] from a list of headers.
    fn new(headers: Vec<HeaderInsertionConfig<M>>) -> Self {
        Self { headers }
    }
}

impl<S, M> Layer<S> for SetMultipleResponseHeadersLayer<M>
where
    M: Clone,
{
    type Service = SetMultipleResponseHeader<S, M>;

    fn layer(&self, inner: S) -> Self::Service {
        SetMultipleResponseHeader {
            inner,
            headers: self.headers.clone(),
        }
    }
}

/// Middleware that sets multiple headers on the response.
#[derive(Clone)]
pub struct SetMultipleResponseHeader<S, M> {
    inner: S,
    headers: Vec<HeaderInsertionConfig<M>>,
}

impl<S, M> SetMultipleResponseHeader<S, M> {
    /// Create a new [`SetMultipleResponseHeader`] that overrides any existing values for the same header.
    ///
    /// If a previous value exists for the same header, it is removed and replaced with the new header value.
    pub fn overriding(inner: S, metadata: Vec<impl HeaderMetadataGenerator<M>>) -> Self {
        let headers: Vec<HeaderInsertionConfig<M>> = metadata
            .into_iter()
            .map(|m| {
                m.to_metadata()
                    .convert_to_header_config(InsertHeaderMode::Override)
            })
            .collect();

        Self::new(inner, headers)
    }

    /// Create a new [`SetMultipleResponseHeader`] that appends header values.
    ///
    /// The new header is always added, preserving any existing values. If previous values exist, the header will have multiple values.
    pub fn appending(inner: S, metadata: Vec<impl HeaderMetadataGenerator<M>>) -> Self {
        let headers: Vec<HeaderInsertionConfig<M>> = metadata
            .into_iter()
            .map(|m| {
                m.to_metadata()
                    .convert_to_header_config(InsertHeaderMode::Append)
            })
            .collect();

        Self::new(inner, headers)
    }

    /// Create a new [`SetMultipleResponseHeader`] that only inserts if the header is not already present.
    ///
    /// If a previous value exists for the header, the new value is not inserted.
    pub fn if_not_present(inner: S, metadata: Vec<impl HeaderMetadataGenerator<M>>) -> Self {
        let headers: Vec<HeaderInsertionConfig<M>> = metadata
            .into_iter()
            .map(|m| {
                m.to_metadata()
                    .convert_to_header_config(InsertHeaderMode::IfNotPresent)
            })
            .collect();

        Self::new(inner, headers)
    }

    /// Internal constructor for a new [`SetMultipleResponseHeader`] from an inner service and a list of headers.
    fn new(inner: S, headers: Vec<HeaderInsertionConfig<M>>) -> Self {
        Self { inner, headers }
    }

    define_inner_service_accessors!();
}

impl<S, M> fmt::Debug for SetMultipleResponseHeader<S, M>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for metadata in &self.headers {
            f.debug_struct("SetMultipleResponseHeader")
                .field("header_name", &metadata.header_name)
                .field("mode", &metadata.mode)
                .field("make", &std::any::type_name::<M>())
                .finish()?;
        }

        Ok(())
    }
}

impl<ReqBody, ResBody, S, M> Service<Request<ReqBody>> for SetMultipleResponseHeader<S, M>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    M: MakeHeaderValue<Response<ResBody>> + Clone,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, M>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    /// Call the inner service and apply all configured headers to the response.
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        ResponseFuture {
            future: self.inner.call(req),
            headers: self.headers.clone(),
        }
    }
}

pin_project! {
    /// Response future for [`SetMultipleResponseHeader`].
    #[derive(Debug)]
    pub struct ResponseFuture<F, M> {
        #[pin]
        future: F,
        headers: Vec<HeaderInsertionConfig<M>>,
    }
}

impl<F, ResBody, E, M> Future for ResponseFuture<F, M>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    M: MakeHeaderValue<Response<ResBody>>,
{
    type Output = F::Output;

    /// Polls the inner future and applies all configured headers to the response before returning it.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        for header in this.headers {
            header
                .mode
                .apply(&header.header_name, &mut res, &mut header.make);
        }

        Poll::Ready(Ok(res))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::Body;
    use http::{header, HeaderValue};
    use std::convert::Infallible;
    use tower::{service_fn, ServiceExt};

    #[tokio::test]
    async fn test_override_mode() {
        let svc = SetMultipleResponseHeader::overriding(
            service_fn(|_req: Request<Body>| async {
                let res = Response::builder()
                    .header(header::CONTENT_TYPE, "good-content")
                    .body(Body::empty())
                    .unwrap();
                Ok::<_, Infallible>(res)
            }),
            vec![(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))],
        );

        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        let mut values = res.headers().get_all(header::CONTENT_TYPE).iter();
        assert_eq!(values.next().unwrap(), "text/html");
        assert_eq!(values.next(), None);
    }

    #[tokio::test]
    async fn test_append_mode() {
        let svc = SetMultipleResponseHeader::appending(
            service_fn(|_req: Request<Body>| async {
                let res = Response::builder()
                    .header(header::CONTENT_TYPE, "good-content")
                    .body(Body::empty())
                    .unwrap();
                Ok::<_, Infallible>(res)
            }),
            vec![(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))],
        );

        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        let mut values = res.headers().get_all(header::CONTENT_TYPE).iter();
        assert_eq!(values.next().unwrap(), "good-content");
        assert_eq!(values.next().unwrap(), "text/html");
        assert_eq!(values.next(), None);
    }

    #[tokio::test]
    async fn test_skip_if_present_mode() {
        let svc = SetMultipleResponseHeader::if_not_present(
            service_fn(|_req: Request<Body>| async {
                let res = Response::builder()
                    .header(header::CONTENT_TYPE, "good-content")
                    .body(Body::empty())
                    .unwrap();
                Ok::<_, Infallible>(res)
            }),
            vec![(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))],
        );

        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        let mut values = res.headers().get_all(header::CONTENT_TYPE).iter();
        assert_eq!(values.next().unwrap(), "good-content");
        assert_eq!(values.next(), None);
    }

    #[tokio::test]
    async fn test_skip_if_present_mode_when_not_present() {
        let svc = SetMultipleResponseHeader::if_not_present(
            service_fn(|_req: Request<Body>| async {
                let res = Response::builder().body(Body::empty()).unwrap();
                Ok::<_, Infallible>(res)
            }),
            vec![(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))],
        );

        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        let mut values = res.headers().get_all(header::CONTENT_TYPE).iter();
        assert_eq!(values.next().unwrap(), "text/html");
        assert_eq!(values.next(), None);
    }

    #[test]
    fn test_tuple_metadata_impl() {
        use super::*;
        let tuple: (HeaderName, HeaderValue) =
            (header::CONTENT_TYPE, HeaderValue::from_static("foo"));
        let meta = tuple.to_metadata();
        assert_eq!(meta.header_name, header::CONTENT_TYPE);
        assert_eq!(meta.make, HeaderValue::from_static("foo"));
    }

    #[test]
    fn test_convert_to_header_config_struct_and_tuple() {
        use super::*;
        let meta = HeaderMetadata {
            header_name: header::CONTENT_TYPE,
            make: HeaderValue::from_static("bar"),
        };
        let rh = meta.convert_to_header_config(crate::set_header::InsertHeaderMode::Override);
        assert_eq!(rh.header_name, header::CONTENT_TYPE);
        assert_eq!(rh.make, HeaderValue::from_static("bar"));

        let tuple: (HeaderName, HeaderValue) =
            (header::CONTENT_TYPE, HeaderValue::from_static("baz"));
        let rh2 = tuple
            .to_metadata()
            .convert_to_header_config(crate::set_header::InsertHeaderMode::Override);
        assert_eq!(rh2.header_name, header::CONTENT_TYPE);
        assert_eq!(rh2.make, HeaderValue::from_static("baz"));
    }

    #[test]
    fn test_debug_impls() {
        use super::*;
        let meta = HeaderMetadata {
            header_name: header::CONTENT_TYPE,
            make: HeaderValue::from_static("bar"),
        };
        let rh = meta
            .clone()
            .convert_to_header_config(crate::set_header::InsertHeaderMode::Override);
        let layer = SetMultipleResponseHeadersLayer::overriding(vec![meta]);
        let debug_str = format!("{:?}", layer);
        assert!(debug_str.contains("SetMultipleResponseHeadersLayer"));
        let debug_rh = format!("{:?}", rh);
        assert!(debug_rh.contains("HeaderInsertionConfig"));

        let svc = SetMultipleResponseHeader::overriding(
            tower::service_fn(|_req: Request<Body>| async {
                Ok::<_, std::convert::Infallible>(Response::new(Body::empty()))
            }),
            vec![(header::CONTENT_TYPE, HeaderValue::from_static("foo"))],
        );
        let debug_svc = format!("{:?}", svc);
        assert!(debug_svc.contains("SetMultipleResponseHeader"));
    }
}
