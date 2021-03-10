use super::{ClassifiedResponse, ClassifyEos, ClassifyResponse};
use http::{HeaderMap, Response};

/// TODO(david): docs
// TODO(david): impl Debug
#[derive(Clone, Copy)]
pub struct MapFailureClass<C, F> {
    inner: C,
    f: F,
}

impl<C, F> MapFailureClass<C, F> {
    pub(crate) fn new(inner: C, f: F) -> Self {
        Self { inner, f }
    }
}

impl<E, C, F, NewFailureClass> ClassifyResponse<E> for MapFailureClass<C, F>
where
    C: ClassifyResponse<E>,
    F: FnOnce(C::FailureClass) -> NewFailureClass,
{
    type FailureClass = NewFailureClass;
    type ClassifyEos = MapEosFailureClass<C::ClassifyEos, F>;

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
                ClassifiedResponse::RequiresEos(MapEosFailureClass {
                    inner: classify_eos,
                    f: self.f,
                })
            }
        }
    }

    fn classify_error(self, error: &E) -> Self::FailureClass {
        (self.f)(self.inner.classify_error(error))
    }
}

/// TODO(david): docs
// TODO(david): impl Debug
#[derive(Clone, Copy)]
pub struct MapEosFailureClass<C, F> {
    inner: C,
    f: F,
}

impl<E, C, F, NewFailureClass> ClassifyEos<E> for MapEosFailureClass<C, F>
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
