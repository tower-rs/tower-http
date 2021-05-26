use super::Traffic;
use tower_layer::Layer;

/// [`Layer`] for adding high level traffic metrics to a [`Service`].
///
/// See the [module docs](crate::metrics::traffic) for more details.
///
/// [`Layer`]: tower_layer::Layer
/// [`Service`]: tower_service::Service
#[derive(Debug, Clone)]
pub struct TrafficLayer<M, MetricsSink> {
    make_classifier: M,
    sink: MetricsSink,
}

impl<M, MetricsSink> TrafficLayer<M, MetricsSink> {
    /// Create a new `TrafficLayer`.
    pub fn new(make_classifier: M, sink: MetricsSink) -> Self {
        TrafficLayer {
            make_classifier,
            sink,
        }
    }
}

impl<S, M, MetricsSink> Layer<S> for TrafficLayer<M, MetricsSink>
where
    M: Clone,
    MetricsSink: Clone,
{
    type Service = Traffic<S, M, MetricsSink>;

    fn layer(&self, inner: S) -> Self::Service {
        Traffic {
            inner,
            make_classifier: self.make_classifier.clone(),
            sink: self.sink.clone(),
        }
    }
}
