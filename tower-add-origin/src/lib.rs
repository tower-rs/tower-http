extern crate futures;
extern crate http;
extern crate tower;

use futures::Poll;
use http::Request;
use http::uri::{Authority, Scheme};
use tower::Service;

/// Wraps an HTTP service, injecting authority and scheme on every request.
pub struct AddOrigin<T> {
    inner: T,
    scheme: Scheme,
    authority: Authority,
}

impl<T> AddOrigin<T> {
    /// Create a new `AddOrigin`
    pub fn new(inner: T, scheme: Scheme, authority: Authority) -> Self {
        AddOrigin {
            inner,
            authority,
            scheme,
        }
    }

    /// Return a reference to the HTTP scheme that is added to all requests.
    pub fn scheme(&self) -> &Scheme {
        &self.scheme
    }

    /// Return a reference to the HTTP authority that is added to all requests.
    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    /// Returns a reference to the inner service.
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Returns a mutable reference to the inner service.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Consumes `self`, returning the inner service.
    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T, B> Service for AddOrigin<T>
where T: Service<Request = Request<B>>,
{
    type Request = Request<B>;
    type Response = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.inner.poll_ready()
    }

    fn call(&mut self, req: Self::Request) -> Self::Future {
        // Split the request into the head and the body.
        let (mut head, body) = req.into_parts();

        // Split the request URI into parts.
        let mut uri: http::uri::Parts = head.uri.into();

        // Update the URI parts, setting hte scheme and authority
        uri.scheme = Some(self.scheme.clone());
        uri.authority = Some(self.authority.clone());

        // Update the the request URI
        head.uri = http::Uri::from_parts(uri)
            .expect("valid uri");

        let request = Request::from_parts(head, body);

        // Call the inner service
        self.inner.call(request)
    }
}
