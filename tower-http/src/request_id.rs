use http::{
    header::{HeaderName, HeaderValue},
    request::Parts,
    Request,
};
use std::task::{Context, Poll};
use tower_layer::Layer;
use tower_service::Service;

pub trait MakeRequestId {
    fn make_request_id(&mut self, request_parts: &Parts) -> (HeaderName, Option<RequestId>);
}

impl<F> MakeRequestId for F
where
    F: FnMut(&Parts) -> (HeaderName, Option<RequestId>),
{
    fn make_request_id(&mut self, request_parts: &Parts) -> (HeaderName, Option<RequestId>) {
        self(request_parts)
    }
}

#[cfg(feature = "uuid")]
#[cfg_attr(docsrs, doc(cfg(feature = "uuid")))]
#[derive(Debug, Clone)]
pub struct UuidRequestId {
    header_name: HeaderName,
}

#[cfg(feature = "uuid")]
#[cfg_attr(docsrs, doc(cfg(feature = "uuid")))]
impl UuidRequestId {
    pub fn new(header_name: HeaderName) -> Self {
        Self { header_name }
    }
}

#[cfg(feature = "uuid")]
impl MakeRequestId for UuidRequestId {
    fn make_request_id(&mut self, request_parts: &Parts) -> (HeaderName, Option<RequestId>) {
        if request_parts.headers.contains_key(&self.header_name) {
            (self.header_name.clone(), None)
        } else {
            let id = HeaderValue::from_str(&uuid::Uuid::new_v4().to_string())
                .expect("uuid wasn't valid header value");
            let id = RequestId::new(id);
            (self.header_name.clone(), Some(id))
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestId(HeaderValue);

impl RequestId {
    pub fn new<T>(header_value: T) -> Self
    where
        T: Into<HeaderValue>,
    {
        Self(header_value.into())
    }

    pub fn header_value(&self) -> &HeaderValue {
        &self.0
    }

    pub fn into_header_value(self) -> HeaderValue {
        self.0
    }

    pub fn from_request<B>(request: &Request<B>) -> Option<Self> {
        request.extensions().get().cloned()
    }
}

#[derive(Debug, Clone)]
pub struct SetRequestIdLayer<M> {
    make_request_id: M,
}

impl<M> SetRequestIdLayer<M> {
    pub fn new(make_request_id: M) -> Self {
        SetRequestIdLayer { make_request_id }
    }
}

#[cfg(feature = "uuid")]
#[cfg_attr(docsrs, doc(cfg(feature = "uuid")))]
impl SetRequestIdLayer<UuidRequestId> {
    pub fn uuid(header_name: HeaderName) -> Self {
        Self::new(UuidRequestId::new(header_name))
    }
}

impl<S, M> Layer<S> for SetRequestIdLayer<M>
where
    M: Clone,
{
    type Service = SetRequestId<S, M>;

    fn layer(&self, inner: S) -> Self::Service {
        SetRequestId::new(inner, self.make_request_id.clone())
    }
}

#[derive(Debug, Clone)]
pub struct SetRequestId<S, M> {
    inner: S,
    make_request_id: M,
}

impl<S, M> SetRequestId<S, M> {
    pub fn new(inner: S, make_request_id: M) -> Self {
        Self {
            inner,
            make_request_id,
        }
    }

    define_inner_service_accessors!();

    pub fn layer(make_request_id: M) -> SetRequestIdLayer<M> {
        SetRequestIdLayer::new(make_request_id)
    }
}

#[cfg(feature = "uuid")]
#[cfg_attr(docsrs, doc(cfg(feature = "uuid")))]
impl<S> SetRequestId<S, UuidRequestId> {
    pub fn uuid(inner: S, header_name: HeaderName) -> Self {
        Self::new(inner, UuidRequestId::new(header_name))
    }
}

impl<S, M, ReqBody> Service<Request<ReqBody>> for SetRequestId<S, M>
where
    S: Service<Request<ReqBody>>,
    M: MakeRequestId,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let (mut parts, body) = req.into_parts();

        match self.make_request_id.make_request_id(&parts) {
            (header, Some(request_id)) => {
                parts.extensions.insert(request_id.clone());
                parts.headers.insert(header, request_id.0);
            }
            (header, None) => {
                if parts.extensions.get::<RequestId>().is_none() {
                    if let Some(request_id) = parts.headers.get(header) {
                        parts.extensions.insert(RequestId::new(request_id.clone()));
                    }
                }
            }
        }

        let req = Request::from_parts(parts, body);

        self.inner.call(req)
    }
}

// TODO(david): tests
