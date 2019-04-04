extern crate futures;
extern crate http;
extern crate tower_service;

use futures::Poll;
use http::uri::{self, Authority, Scheme, Uri};
use http::{HttpTryFrom, Request};
use tower_service::Service;

/// Wraps an HTTP service, injecting authority and scheme on every request.
#[derive(Debug, Clone)]
pub struct AddOrigin<T> {
    inner: T,
    scheme: Scheme,
    authority: Authority,
}

/// Configure an `AddOrigin` instance
#[derive(Debug, Default)]
pub struct Builder {
    uri: Option<Uri>,
}

/// Errors that can happen when building an `AddOrigin`.
#[derive(Debug)]
pub struct BuilderError {
    _p: (),
}

// ===== impl AddOrigin ======

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

impl<T, B> Service<Request<B>> for AddOrigin<T>
where
    T: Service<Request<B>>,
{
    type Response = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.inner.poll_ready()
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        // Split the request into the head and the body.
        let (mut head, body) = req.into_parts();

        // Split the request URI into parts.
        let mut uri: http::uri::Parts = head.uri.into();

        // Update the URI parts, setting hte scheme and authority
        uri.scheme = Some(self.scheme.clone());
        uri.authority = Some(self.authority.clone());

        // Update the the request URI
        head.uri = http::Uri::from_parts(uri).expect("valid uri");

        let request = Request::from_parts(head, body);

        // Call the inner service
        self.inner.call(request)
    }
}

// ===== impl Builder ======

impl Builder {
    /// Return a new, default builder
    pub fn new() -> Self {
        Builder::default()
    }

    /// Set the URI to use as the origin for all requests.
    pub fn uri<T>(&mut self, uri: T) -> &mut Self
    where
        Uri: HttpTryFrom<T>,
    {
        self.uri = Uri::try_from(uri).map(Some).unwrap_or(None);

        self
    }

    pub fn build<T>(&mut self, inner: T) -> Result<AddOrigin<T>, BuilderError> {
        // Create the error just in case. It is a zero sized type anyway right
        // now.
        let err = BuilderError { _p: () };

        let uri = match self.uri.take() {
            Some(uri) => uri,
            None => return Err(err),
        };

        let parts = uri::Parts::from(uri);

        // Get the scheme
        let scheme = match parts.scheme {
            Some(scheme) => scheme,
            None => return Err(err),
        };

        // Get the authority
        let authority = match parts.authority {
            Some(authority) => authority,
            None => return Err(err),
        };

        // Ensure that the path is unsued
        match parts.path_and_query {
            None => {}
            Some(ref path) if path == "/" => {}
            _ => return Err(err),
        }

        Ok(AddOrigin::new(inner, scheme, authority))
    }
}
