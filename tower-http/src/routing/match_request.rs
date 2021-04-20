use http::Request;

pub trait MatchRequest<B> {
    type Output;

    fn match_request(&mut self, request: &Request<B>) -> Option<Self::Output>;
}

impl<F, B, T> MatchRequest<B> for F
where
    F: FnMut(&Request<B>) -> Option<T>,
{
    type Output = T;

    fn match_request(&mut self, request: &Request<B>) -> Option<Self::Output> {
        self(request)
    }
}

pub fn accept_all<B>() -> impl MatchRequest<B, Output = ()> + Copy {
    |_: &Request<B>| Some(())
}

pub fn exact_path<B, S>(path: S) -> impl MatchRequest<B, Output = ()> + Clone
where
    S: Into<String>,
{
    let path = path.into();
    move |req: &Request<B>| {
        if req.uri().path() == path {
            Some(())
        } else {
            None
        }
    }
}
