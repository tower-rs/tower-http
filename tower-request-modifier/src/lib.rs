#![doc(html_root_url = "https://docs.rs/tower-request-modifier/0.1.0")]
#![deny(missing_docs, missing_debug_implementations, unreachable_pub)]
#![cfg_attr(test, deny(warnings))]

//! A `tower::Service` middleware to modify the request.

use http::header::{HeaderName, HeaderValue};
use http::uri::{self, Uri};
use http::Request;
use std::convert::TryFrom;
use std::fmt;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower_service::Service;

/// Wraps an HTTP service, injecting authority and scheme on every request.
pub struct RequestModifier<T, B> {
    inner: T,
    modifiers: Arc<Vec<Box<dyn Fn(Request<B>) -> Request<B> + Send + Sync>>>,
}

impl<T, B> std::fmt::Debug for RequestModifier<T, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(f, "RequestModifier with {} modifiers", self.modifiers.len())
    }
}

/// Configure an `RequestModifier` instance
pub struct Builder<B> {
    modifiers: Vec<Result<Box<dyn Fn(Request<B>) -> Request<B> + Send + Sync>, BuilderError>>,
}

impl<B> Default for Builder<B> {
    fn default() -> Self {
        Builder {
            modifiers: Vec::default(),
        }
    }
}

/// Errors that can happen when building an `RequestModifier`.
#[derive(Debug)]
pub struct BuilderError {
    _p: (),
}

// ===== impl RequestModifier ======

impl<T, B> RequestModifier<T, B> {
    /// Create a new `RequestModifier`
    pub fn new(
        inner: T,
        modifiers: Arc<Vec<Box<dyn Fn(Request<B>) -> Request<B> + Send + Sync>>>,
    ) -> Self {
        RequestModifier {
            inner: inner,
            modifiers: modifiers,
        }
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

impl<T, B> Service<Request<B>> for RequestModifier<T, B>
where
    T: Service<Request<B>>,
{
    type Response = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        let mods = &self.modifiers;
        for m in mods.iter() {
            req = m(req);
        }

        // Call the inner service
        self.inner.call(req)
    }
}

impl<T, B> Clone for RequestModifier<T, B>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        RequestModifier {
            inner: self.inner.clone(),
            modifiers: self.modifiers.clone(),
        }
    }
}

// ===== impl Builder ======

impl<B> Builder<B> {
    /// Return a new, default builder
    pub fn new() -> Self {
        Builder::default()
    }

    /// Build a Fn to add desired header
    fn make_add_header(
        name: HeaderName,
        val: HeaderValue,
    ) -> Box<dyn Fn(Request<B>) -> Request<B> + Send + Sync> {
        Box::new(move |mut req: Request<B>| {
            req.headers_mut().append(name.clone(), val.clone());
            req
        })
    }

    /// Set a header on all requests.
    pub fn add_header<T: ToString, R>(mut self, name: T, val: R) -> Self
    where
        HeaderName: TryFrom<T>,
        HeaderValue: TryFrom<R>,
    {
        let name = HeaderName::try_from(name);
        let val = HeaderValue::try_from(val);

        let err = BuilderError { _p: () };

        let modification = match (name, val) {
            (Ok(name), Ok(val)) => Ok(Self::make_add_header(name, val)),
            (_, _) => Err(err),
        };

        self.modifiers.push(modification);
        self
    }

    /// Build a Fn to perform desired Request origin modification
    fn make_set_origin(
        scheme: uri::Scheme,
        authority: uri::Authority,
    ) -> Box<dyn Fn(Request<B>) -> Request<B> + Send + Sync> {
        Box::new(move |req: Request<B>| {
            // Split the request into the head and the body.
            let (mut head, body) = req.into_parts();

            // Split the request URI into parts.
            let mut uri: http::uri::Parts = head.uri.into();

            // Update the URI parts, setting the scheme and authority
            uri.authority = Some(authority.clone());
            uri.scheme = Some(scheme.clone());

            // Update the the request URI
            head.uri = http::Uri::from_parts(uri).expect("valid uri");

            Request::from_parts(head, body)
        })
    }

    /// Set the URI to use as the origin for all requests.
    pub fn set_origin<T>(mut self, uri: T) -> Self
    where
        Uri: TryFrom<T>,
    {
        let modification = Uri::try_from(uri)
            .map_err(|_| BuilderError { _p: () })
            .and_then(|u| {
                let parts = uri::Parts::from(u);

                let scheme = parts.scheme.ok_or(BuilderError { _p: () })?;
                let authority = parts.authority.ok_or(BuilderError { _p: () })?;

                let check = match parts.path_and_query {
                    None => Ok(()),
                    Some(ref path) if path == "/" => Ok(()),
                    _ => Err(BuilderError { _p: () }),
                };

                check.and_then(|_| Ok(Self::make_set_origin(scheme, authority)))
            });

        self.modifiers.push(modification);
        self
    }

    /// Run an arbitrary modifier on all requests
    pub fn add_modifier(
        mut self,
        modifier: Box<dyn Fn(Request<B>) -> Request<B> + Send + Sync>,
    ) -> Self {
        self.modifiers.push(Ok(modifier));
        self
    }

    /// Build the `RequestModifier` from the provided settings.
    pub fn build<T>(self, inner: T) -> Result<RequestModifier<T, B>, BuilderError> {
        let modifiers = self.modifiers.into_iter().collect::<Result<Vec<_>, _>>()?;
        Ok(RequestModifier::new(inner, Arc::new(modifiers)))
    }
}

impl<B> fmt::Debug for Builder<B> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RequestModifierBuilder")
    }
}
