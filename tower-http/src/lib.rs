extern crate futures;
extern crate http;
extern crate tower_http_service;
extern crate tower_util;

use futures::{Future, Poll};
use http::{Request, Response};
use http::uri::Uri;
use tower_http_service::HttpService;
use tower_http_service::util::{IntoService};
use tower_util::Oneshot;

pub struct Client<T> {
    service: T,
}

pub struct ResponseFuture<T: HttpService<RequestBody>, RequestBody> {
    inner: Oneshot<IntoService<T>, Request<RequestBody>>,
}

impl<T> Client<T> {
    pub fn get<RequestBody>(&self, uri: Uri) -> ResponseFuture<T, RequestBody>
    where
        T: HttpService<RequestBody> + Clone,
        RequestBody: Default
    {
        let request = Request::get(uri).body(Default::default()).unwrap();
        let inner = Oneshot::new(self.service.clone().into_service(), request);

        ResponseFuture { inner }
    }
}

impl<T, RequestBody> Future for ResponseFuture<T, RequestBody>
where
    T: HttpService<RequestBody>,
{
    type Item = Response<T::ResponseBody>;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll()
    }
}
