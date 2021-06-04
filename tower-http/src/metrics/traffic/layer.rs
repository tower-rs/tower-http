use super::Traffic;
use tower_layer::Layer;

/// [`Layer`] for adding high level traffic metrics to a [`Service`].
///
/// See the [module docs](crate::metrics::traffic) for more details.
///
/// [`Layer`]: tower_layer::Layer
/// [`Service`]: tower_service::Service
#[derive(Debug, Clone)]
pub struct TrafficLayer<M, Callbacks> {
    make_classifier: M,
    callbacks: Callbacks,
}

impl<M, Callbacks> TrafficLayer<M, Callbacks> {
    /// Create a new `TrafficLayer`.
    pub fn new(make_classifier: M, callbacks: Callbacks) -> Self {
        TrafficLayer {
            make_classifier,
            callbacks,
        }
    }
}

impl<S, M, Callbacks> Layer<S> for TrafficLayer<M, Callbacks>
where
    M: Clone,
    Callbacks: Clone,
{
    type Service = Traffic<S, M, Callbacks>;

    fn layer(&self, inner: S) -> Self::Service {
        Traffic {
            inner,
            make_classifier: self.make_classifier.clone(),
            callbacks: self.callbacks.clone(),
        }
    }
}
