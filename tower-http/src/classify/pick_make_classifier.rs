use super::{ClassifyResponse, MakeClassifier};
use http::{HeaderMap, Method, Request, Uri};

/// TODO(david): docs
pub fn make_classifier_fn<F>(f: F) -> MakeClassifierFn<F> {
    MakeClassifierFn { f }
}

/// TODO(david): docs
// TODO(david): impl Debug
#[derive(Clone, Copy)]
pub struct MakeClassifierFn<F> {
    f: F,
}

impl<E, F, C> MakeClassifier<E> for MakeClassifierFn<F>
where
    F: Fn(&Method, &Uri, &HeaderMap) -> C,
    C: ClassifyResponse<E>,
{
    type Classifier = C;
    type FailureClass = C::FailureClass;
    type ClassifyEos = C::ClassifyEos;

    fn make_classifier<B>(&self, req: &Request<B>) -> Self::Classifier {
        let method = req.method();
        let uri = req.uri();
        let headers = req.headers();
        (self.f)(method, uri, headers)
    }
}
