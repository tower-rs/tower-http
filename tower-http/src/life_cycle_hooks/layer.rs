use super::LifeCycleHooks;
use tower_layer::Layer;

/// [`Layer`] for adding high level traffic metrics to a [`Service`].
///
/// See the [module docs](crate::life_cycle_hooks) for more details.
///
/// [`Layer`]: tower_layer::Layer
/// [`Service`]: tower_service::Service
#[derive(Debug, Clone)]
pub struct LifeCycleHooksLayer<M, Callbacks> {
    make_classifier: M,
    callbacks: Callbacks,
}

impl<M, Callbacks> LifeCycleHooksLayer<M, Callbacks> {
    /// Create a new `LifeCycleHooksLayer`.
    pub fn new(make_classifier: M, callbacks: Callbacks) -> Self {
        LifeCycleHooksLayer {
            make_classifier,
            callbacks,
        }
    }
}

impl<S, M, Callbacks> Layer<S> for LifeCycleHooksLayer<M, Callbacks>
where
    M: Clone,
    Callbacks: Clone,
{
    type Service = LifeCycleHooks<S, M, Callbacks>;

    fn layer(&self, inner: S) -> Self::Service {
        LifeCycleHooks {
            inner,
            make_classifier: self.make_classifier.clone(),
            callbacks: self.callbacks.clone(),
        }
    }
}
