#![doc(html_root_url = "https://docs.rs/tower-request-modifier/0.1.0")]
#![deny(missing_docs, missing_debug_implementations, unreachable_pub)]
#![cfg_attr(test, deny(warnings))]

//! A `tower::Service` middleware to modify the request.

use std::convert::TryFrom;
use std::fmt;
use std::marker::PhantomData;

use http::header::{HeaderName, HeaderValue};
use http::uri::{self, Uri};
use http::Request;
use tower::util::With;
use tower::{Service, ServiceExt};

/// Adaptor provides a way to modify requests.
pub struct Adaptor<B, S> {
    inner: S,
    _p: std::marker::PhantomData<B>,
}

impl<B, S> fmt::Debug for Adaptor<B, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Adaptor")
    }
}

/// Errors associated with using the [`Adaptor`].
#[derive(Debug)]
pub struct AdaptorError;

impl<B, S> Adaptor<B, S> {
    /// Return a new, default Adaptor
    pub fn new(inner: S) -> Self {
        Adaptor {
            inner,
            _p: PhantomData,
        }
    }
}

/// Build a Fn to add desired header.
fn make_add_header<B>(
    name: HeaderName,
    val: HeaderValue,
) -> impl FnOnce(Request<B>) -> Request<B> + Clone {
    |mut req: Request<B>| {
        req.headers_mut().append(name, val);
        req
    }
}

/// Build a Fn to perform desired Request origin modification.
fn make_set_origin<B>(
    scheme: uri::Scheme,
    authority: uri::Authority,
) -> impl FnOnce(Request<B>) -> Request<B> + Clone {
    move |req: Request<B>| {
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
    }
}

impl<B, S> Adaptor<B, S>
where
    S: Service<Request<B>>,
{
    /// Set a header on all requests.
    pub fn add_header<T: ToString, R>(
        self,
        name: T,
        val: R,
    ) -> Result<Adaptor<B, With<S, impl FnOnce(Request<B>) -> Request<B> + Clone>>, AdaptorError>
    where
        HeaderName: TryFrom<T>,
        HeaderValue: TryFrom<R>,
    {
        let name = HeaderName::try_from(name).map_err(|_| AdaptorError)?;
        let val = HeaderValue::try_from(val).map_err(|_| AdaptorError)?;

        let header_append = make_add_header(name, val);
        let new_service = self.inner.with(header_append);

        Ok(Adaptor::new(new_service))
    }

    /// Set the URI to use as the origin for all requests.
    pub fn set_origin<T>(
        self,
        uri: T,
    ) -> Result<Adaptor<B, With<S, impl FnOnce(Request<B>) -> Request<B> + Clone>>, AdaptorError>
    where
        Uri: TryFrom<T>,
    {
        let u = Uri::try_from(uri).map_err(|_| AdaptorError)?;

        let parts = uri::Parts::from(u);

        let scheme = parts.scheme.ok_or(AdaptorError)?;
        let authority = parts.authority.ok_or(AdaptorError)?;

        if let Some(path) = parts.path_and_query {
            if path != "/" {
                return Err(AdaptorError);
            }
        }

        let set_origin = make_set_origin(scheme, authority);
        let new_service = self.inner.with(set_origin);

        Ok(Adaptor::new(new_service))
    }

    /// Run an arbitrary modifier on all requests.
    pub fn add_modifier<F>(self, f: F) -> Adaptor<B, With<S, F>>
    where
        F: FnOnce(Request<B>) -> Request<B> + Clone,
    {
        Adaptor::new(self.inner.with(f))
    }

    /// Build the service subject to the adaptor.
    pub fn apply(self) -> S {
        self.inner
    }
}
