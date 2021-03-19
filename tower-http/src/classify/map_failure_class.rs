use super::{ClassifiedResponse, ClassifyEos, ClassifyResponse};
use http::{HeaderMap, Response};
use std::fmt;
use std::marker::PhantomData;

/// [`ClassifyResponse`] and [`ClassifyEos`] combinator that transforms failure classifications.
///
/// See [`ClassifyResponse::map_failure_class`] or [`ClassifyEos::map_failure_class`] for more
/// details.
pub struct MapFailureClass<C, F, E> {
    inner: C,
    f: F,
    _marker: PhantomData<E>,
}

impl<C, F, E> MapFailureClass<C, F, E> {
    pub(crate) fn new(inner: C, f: F) -> Self {
        Self {
            inner,
            f,
            _marker: PhantomData,
        }
    }
}

impl<E, C, F, NewFailureClass> ClassifyResponse<E> for MapFailureClass<C, F, E>
where
    C: ClassifyResponse<E>,
    F: FnOnce(C::FailureClass) -> NewFailureClass,
{
    type FailureClass = NewFailureClass;
    type ClassifyEos = MapFailureClass<C::ClassifyEos, F, E>;

    fn classify_response<B>(
        self,
        res: &Response<B>,
    ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos> {
        match self.inner.classify_response(res) {
            ClassifiedResponse::Ready(Ok(())) => ClassifiedResponse::Ready(Ok(())),
            ClassifiedResponse::Ready(Err(class)) => {
                ClassifiedResponse::Ready(Err((self.f)(class)))
            }
            ClassifiedResponse::RequiresEos(classify_eos) => {
                ClassifiedResponse::RequiresEos(classify_eos.map_failure_class(self.f))
            }
        }
    }

    fn classify_error(self, error: &E) -> Self::FailureClass {
        (self.f)(self.inner.classify_error(error))
    }
}

impl<C, F, E> Clone for MapFailureClass<C, F, E>
where
    C: Clone,
    F: Clone,
{
    fn clone(&self) -> Self {
        Self::new(self.inner.clone(), self.f.clone())
    }
}

impl<C, F, E> fmt::Debug for MapFailureClass<C, F, E>
where
    C: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MapFailureClass")
            .field("inner", &self.inner)
            .field("f", &format_args!("{}", std::any::type_name::<F>()))
            .finish()
    }
}

impl<E, C, F, NewFailureClass> ClassifyEos<E> for MapFailureClass<C, F, E>
where
    C: ClassifyEos<E>,
    F: FnOnce(C::FailureClass) -> NewFailureClass,
{
    type FailureClass = NewFailureClass;

    fn classify_eos(self, trailers: Option<&HeaderMap>) -> Result<(), Self::FailureClass> {
        self.inner.classify_eos(trailers).map_err(self.f)
    }

    fn classify_error(self, error: &E) -> Self::FailureClass {
        (self.f)(self.inner.classify_error(error))
    }
}
