use crate::common::*;
use debug_enter_leave::DebugEnterLeaveLayer;

pub(crate) mod debug_enter_leave;

pub trait LayerExt<S>: Layer<S> {
    fn debug_enter_leave(self, name: &str) -> DebugEnterLeaveLayer<Self, S>
    where
        Self: Sized,
    {
        DebugEnterLeaveLayer::new(self, name)
    }
}

impl<T, S> LayerExt<S> for T where T: Layer<S> {}
