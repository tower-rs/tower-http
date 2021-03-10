// TODO(david): document somewhere that things are implemented for `tower::util::Either`

use super::{ClassifiedResponse, ClassifyEos, ClassifyResponse};
use http::{HeaderMap, Response};
use tower::util::Either;

impl<A, B, E> ClassifyResponse<E> for Either<A, B>
where
    A: ClassifyResponse<E>,
    B: ClassifyResponse<E, FailureClass = A::FailureClass>,
{
    type FailureClass = A::FailureClass;
    type ClassifyEos = Either<A::ClassifyEos, B::ClassifyEos>;

    fn classify_response<Body>(
        self,
        res: &Response<Body>,
    ) -> ClassifiedResponse<Self::FailureClass, Self::ClassifyEos> {
        match self {
            Either::A(inner) => inner.classify_response(res).map_classify_eos(Either::A),
            Either::B(inner) => inner.classify_response(res).map_classify_eos(Either::B),
        }
    }

    fn classify_error(self, error: &E) -> Self::FailureClass {
        match self {
            Either::A(inner) => inner.classify_error(error),
            Either::B(inner) => inner.classify_error(error),
        }
    }
}

impl<A, B, E> ClassifyEos<E> for Either<A, B>
where
    A: ClassifyEos<E>,
    B: ClassifyEos<E, FailureClass = A::FailureClass>,
{
    type FailureClass = A::FailureClass;

    fn classify_eos(self, trailers: Option<&HeaderMap>) -> Result<(), Self::FailureClass> {
        match self {
            Either::A(inner) => inner.classify_eos(trailers),
            Either::B(inner) => inner.classify_eos(trailers),
        }
    }

    fn classify_error(self, error: &E) -> Self::FailureClass {
        match self {
            Either::A(inner) => inner.classify_error(error),
            Either::B(inner) => inner.classify_error(error),
        }
    }
}
