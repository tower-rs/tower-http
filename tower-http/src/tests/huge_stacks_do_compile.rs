use super::EchoService;
use crate::common::*;
use crate::{
    add_extension::AddExtensionLayer, compression::CompressionLayer, metrics::MetricsLayer,
    propagate_header::PropagateHeaderLayer, sensitive_header::SensitiveHeaderLayer,
    set_response_header::SetResponseHeaderLayer, trace::TraceLayer, wrap_in_span::WrapInSpanLayer,
};
use hyper::Body;
use tower::{BoxError, ServiceBuilder};

// We've seen some issues with huge service stacks not compiling so lets make sure we don't get
// those issues.
#[allow(dead_code)]
fn huge_stacks_do_compile() -> impl Service<
    Request<Body>,
    Response = Response<Body>,
    Error = impl Into<BoxError> + Send + Sync,
    Future = impl Future<Output = Result<Response<Body>, impl Into<BoxError> + Send + Sync>> + Send,
> + Clone {
    let state: i32 = 1337;
    let span = tracing::debug_span!("huge_stacks_do_compile");
    let request_id_header = HeaderName::from_static("x-request-id");

    ServiceBuilder::new()
        .layer(AddExtensionLayer::new(state))
        .layer(AddExtensionLayer::new(state))
        .layer(AddExtensionLayer::new(state))
        .layer(AddExtensionLayer::new(state))
        .layer(AddExtensionLayer::new(state))
        .layer(AddExtensionLayer::new(state))
        .layer(AddExtensionLayer::new(state))
        .layer(AddExtensionLayer::new(state))
        .layer(AddExtensionLayer::new(state))
        .layer(AddExtensionLayer::new(state))
        .layer(CompressionLayer::new())
        .layer(CompressionLayer::new())
        .layer(CompressionLayer::new())
        .layer(CompressionLayer::new())
        .layer(CompressionLayer::new())
        .layer(CompressionLayer::new())
        .layer(CompressionLayer::new())
        .layer(CompressionLayer::new())
        .layer(CompressionLayer::new())
        .layer(CompressionLayer::new())
        .layer(MetricsLayer::new())
        .layer(MetricsLayer::new())
        .layer(MetricsLayer::new())
        .layer(MetricsLayer::new())
        .layer(MetricsLayer::new())
        .layer(MetricsLayer::new())
        .layer(MetricsLayer::new())
        .layer(MetricsLayer::new())
        .layer(MetricsLayer::new())
        .layer(MetricsLayer::new())
        .layer(PropagateHeaderLayer::new(request_id_header.clone()))
        .layer(PropagateHeaderLayer::new(request_id_header.clone()))
        .layer(PropagateHeaderLayer::new(request_id_header.clone()))
        .layer(PropagateHeaderLayer::new(request_id_header.clone()))
        .layer(PropagateHeaderLayer::new(request_id_header.clone()))
        .layer(PropagateHeaderLayer::new(request_id_header.clone()))
        .layer(PropagateHeaderLayer::new(request_id_header.clone()))
        .layer(PropagateHeaderLayer::new(request_id_header.clone()))
        .layer(PropagateHeaderLayer::new(request_id_header.clone()))
        .layer(PropagateHeaderLayer::new(request_id_header))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SensitiveHeaderLayer::new(header::AUTHORIZATION))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(SetResponseHeaderLayer::new(make_header_pair))
        .layer(TraceLayer::new())
        .layer(TraceLayer::new())
        .layer(TraceLayer::new())
        .layer(TraceLayer::new())
        .layer(TraceLayer::new())
        .layer(TraceLayer::new())
        .layer(TraceLayer::new())
        .layer(TraceLayer::new())
        .layer(TraceLayer::new())
        .layer(TraceLayer::new())
        .layer(WrapInSpanLayer::new(span.clone()))
        .layer(WrapInSpanLayer::new(span.clone()))
        .layer(WrapInSpanLayer::new(span.clone()))
        .layer(WrapInSpanLayer::new(span.clone()))
        .layer(WrapInSpanLayer::new(span.clone()))
        .layer(WrapInSpanLayer::new(span.clone()))
        .layer(WrapInSpanLayer::new(span.clone()))
        .layer(WrapInSpanLayer::new(span.clone()))
        .layer(WrapInSpanLayer::new(span.clone()))
        .layer(WrapInSpanLayer::new(span))
        .service(EchoService)
}

fn make_header_pair(_res: &Response<Body>) -> (HeaderName, HeaderValue) {
    let header = HeaderName::from_static("x-request-id");
    let value = HeaderValue::from_static("123");
    (header, value)
}
