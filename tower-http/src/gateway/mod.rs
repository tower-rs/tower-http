//! Middleware that implements an HTTP gateway / reverse proxy.
//!
//! [`Gateway`] implements the logic of an HTTP gateway / reverse proxy, modifying incoming request
//! URLs and headers and outgoing response headers. It defers to an underlying service (such as
//! [`hyper::Client`]) for the actual transport to the proxied server.
//!
//! # Example
//!
//! This can be mounted on [`axum::Router`] using [`hyper::Client`] as its transport:
//!
//! ```rust,no_run
//! use axum::{error_handling::HandleError, http, Router};
//! use tower_http::gateway::Gateway;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = hyper::Client::new();
//! let gateway = Gateway::new(client, http::Uri::from_static("http://example.com:1234/api/v1"))?;
//! let app = Router::new().nest(
//!     "/api",
//!     HandleError::new(gateway, |_| async { http::StatusCode::BAD_GATEWAY }),
//! );
//! # Ok(()) }
//! ```
//!
//! # URL Handling
//!
//! Every request sent to this service will have its URI interpreted relative to the configured
//! remote URI. Any scheme or authority on the request URI will be discarded, its path will be
//! joined to the remote URI's path, and its query (if any) will be used. The remote URI's complete
//! path will be a prefix of the resulting path (even if it doesn't end in `/`) and its query will
//! be ignored.
//!
//! # Request Headers
//!
//! This service strips most hop-by-hop headers from the request before forwarding it along,
//! including arbitrary headers listed in [`Connection`][]. The exceptions to this are [`TE`][],
//! [`Transfer-Encoding`][], and [`Trailer`][]. This is because these headers do not affect any
//! processing done by this service. In particular, if the request body is passed straight through
//! to the backend server without processing then details on how its encoded (e.g.
//! [`Transfer-Encoding`][]) must be sent to the backend server, if the response body similarly
//! avoids processing then the [`TE`][] header also needs to be sent to the backend server, and
//! lastly if the request or response body has any trailer headers then the [`Trailer`][] header
//! needs to be preserved as well. Similarly the [`Connection`][] header will retain any of these
//! headers even as it's processed to remove any other hop-by-hop headers. If the request or
//! response body is modified by some other layer it is that layer's responsibility to update these
//! headers accordingly.
//!
//! Besides [`Connection`][], other standard hop-by-hop headers (except those listed above) are
//! removed even if they're not listed in [`Connection`][].
//!
//! This service will add a [`Forwarded`][] header to the request it forwards that identifies the
//! source of the incoming request. If the connection info is unknown the request will specify
//! `Forwarded: for=unknown`. See [`ConnectionInfo`], [`Gateway::with_connection_info`], and
//! [`Gateway::with_connection_info_fn`] for details.
//!
//! This service may optionally add `X-Forwarded-*` headers that mirror the [`Forwarded`][] header.
//! See [`Gateway::use_x_forwarded`] for details.
//!
//! ## `Via` header
//!
//! This service will add a [`Via`][] header if configured with a [`ConnectionInfo`] that has
//! either [`local_ip`][] or [`via_received_by`][] set. The [`Via`][] header's documentation
//! declares that an HTTP-to-HTTP gateway _MUST_ send an appropriate Via header field in each
//! inbound request. For this reason you may want to ensure that either [`local_ip`][] or
//! [`via_received_by`][] is set for each request.
//!
//! The constructed [`Via`][] header will use the incoming request's [`Request::version`] to derive
//! the [`received-protocol`][] portion. For this reason the version should not be modified prior
//! to handing the request to the gateway. The gateway will reset the version back to the default
//! prior to forwarding it, and a layer can be used after the gateway to set the request version if
//! something other than the default is desired.
//!
//! The [`Via`][] header can be used to detect infinite forwarding loops. Although this service
//! does not implement such detection automatically, by ensuring that the either [`local_ip`][] or
//! [`via_received_by`][] is set for each request, you may implement such detection in a wrapping
//! layer.
//!
//! [`local_ip`]: ConnectionInfo::local_ip
//! [`via_received_by`]: ConnectionInfo::via_received_by
//!
//! # Response Headers
//!
//! This service does not currently rewrite any response headers, besides filtering out hop-by-hop
//! headers as needed. In particular, this does not rewrite URLs in `Location` or
//! `Content-Location` headers. This may be added in the future. It also does not rewrite the
//! domain or path in any `Set-Cookie` headers, and it does not set or modify the [`Via`][] header.
//!
//! [`hyper::Client`]: https://docs.rs/hyper/0.14/hyper/client/struct.Client.html
//! [`axum::Router`]: https://docs.rs/axum/0.5/axum/struct.Router.html
//! [`Connection`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Connection
//! [`TE`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/TE
//! [`Transfer-Encoding`]:
//!     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Transfer-Encoding
//! [`Trailer`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Trailer
//! [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
//! [`Via`]: https://httpwg.org/specs/rfc7230.html#header.via "RFC 7230 §5.7.1 Via"
//! [`Request::version`]: fn@http::Request::version
//! [`received-protocol`]: https://httpwg.org/specs/rfc7230.html#header.via
//! [features]: https://doc.rust-lang.org/cargo/reference/features.html#the-features-section

use std::{
    fmt::{self, Write},
    future::Future,
    net::IpAddr,
    task::Poll,
};

use bytes::{BufMut, BytesMut};
use http::{header::HeaderName, HeaderValue};
use pin_project_lite::pin_project;
use tower_layer::Layer;
use tower_service::Service;

mod connection_info;
pub use connection_info::*;
mod util;

/// Layer that applies [`Gateway`] which implements an HTTP gateway / reverse proxy.
///
/// See the [module docs](self) for more details.
#[derive(Clone)]
pub struct GatewayLayer<F = for<'a> fn(&'a http::Extensions) -> ConnectionInfo<'a>> {
    remote: http::Uri,
    connection_info: F,
    use_x_forwarded: bool,
}

impl GatewayLayer {
    /// Forwards requests to a given remote URI after modifying headers appropriately.
    ///
    /// Returns [`Err`] if the remote URI cannot have a path joined onto it (e.g. because it has an
    /// authority and no scheme).
    pub fn new(remote: http::Uri) -> Result<Self, http::Error> {
        Ok(Self {
            remote: validate_remote_uri(remote)?,
            connection_info: |_| ConnectionInfo::new(),
            use_x_forwarded: false,
        })
    }

    /// Sets the type to use to get info about the connection from the request.
    ///
    /// The type will be looked up in the request's extensions map.
    ///
    /// This is a convenience for `self.with_connection_info_fn(|ext| ext.get::<C>().into())`.
    pub fn with_connection_info<C>(
        self,
    ) -> GatewayLayer<impl for<'a> FnMut(&'a http::Extensions) -> ConnectionInfo<'a>>
    where
        C: Send + Sync + 'static,
        for<'a> &'a C: Into<ConnectionInfo<'a>>,
    {
        self.with_connection_info_fn(|ext| ext.get::<C>().into())
    }

    /// Sets a function to use to get info about the connection from the request.
    ///
    /// This can be used to adapt connection info from other crates without having to add a layer
    /// that modifies the request extensions.
    ///
    /// # Example
    ///
    /// Adapting [`axum::extract::connect_info::ConnectInfo`][]:
    ///
    /// ```rust
    /// use axum::extract::connect_info::ConnectInfo;
    /// use std::net::SocketAddr;
    /// use tower::ServiceBuilder;
    /// use tower_http::gateway::Gateway;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let gateway = ServiceBuilder::new()
    ///     .layer(
    ///         Gateway::layer(http::Uri::from_static("http://example.com:1234/api"))?
    ///             .with_connection_info_fn(|ext| ext.get::<ConnectInfo<SocketAddr>>().map(|x| &x.0).into()),
    ///     )
    ///     .service(hyper::Client::new());
    /// # Ok(()) }
    /// ```
    ///
    /// [`axum::extract::connect_info::ConnectInfo`]:
    ///     https://docs.rs/axum/0.5/axum/extract/connect_info/struct.ConnectInfo.html
    pub fn with_connection_info_fn<F>(self, f: F) -> GatewayLayer<F>
    where
        F: for<'a> FnMut(&'a http::Extensions) -> ConnectionInfo<'a>,
    {
        GatewayLayer {
            remote: self.remote,
            connection_info: f,
            use_x_forwarded: self.use_x_forwarded,
        }
    }
}

impl<F> GatewayLayer<F> {
    /// Enables or disables the `X-Forwarded-*` headers on forwarded requests.
    ///
    /// If set to `true`, the forwarded request will have [`X-Forwarded-For`][],
    /// [`X-Forwarded-Host`][], and [`X-Forwarded-Proto`][] headers set accordingly, depending on
    /// the associated [`ConnectionInfo`] (see [`with_connection_info`] and
    /// [`with_connection_info_fn`]). If the corresponding fields on the [`ConnectionInfo`] are
    /// [`None`] (or obfuscated) the `X-Forwarded-*` header will be left unmodified.
    ///
    /// If set to `false` (the default), those headers will be ignored and any such headers already
    /// on the request will be retained as-is.
    ///
    /// Note: When the incoming request has any such headers and the [`ConnectionInfo`] does not
    /// specify unobfuscated values for all 3 headers, this may result in the forwarded request
    /// only updating some headers and not others, leading to the `X-Forwarded-*` headers having
    /// differing numbers of components.
    ///
    /// This setting should only be enabled when required. Whenever possible, the [`Forwarded`][]
    /// header should be preferred.
    ///
    /// [`X-Forwarded-For`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-For
    /// [`X-Forwarded-Host`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-Host
    /// [`X-Forwarded-Proto`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-Proto
    /// [`with_connection_info`]: fn@Self::with_connection_info
    /// [`with_connection_info_fn`]: fn@Self::with_connection_info_fn
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn use_x_forwarded(self, use_x_forwarded: bool) -> Self {
        Self {
            use_x_forwarded,
            ..self
        }
    }
}

impl<S, F: Clone> Layer<S> for GatewayLayer<F> {
    type Service = Gateway<S, F>;

    fn layer(&self, inner: S) -> Self::Service {
        Gateway {
            inner,
            remote: self.remote.clone(),
            connection_info: self.connection_info.clone(),
            use_x_forwarded: self.use_x_forwarded,
        }
    }
}

/// Middleware that implements an HTTP gateway / reverse proxy.
///
/// See the [module docs](self) for more details.
#[derive(Clone)]
pub struct Gateway<S, F = for<'a> fn(&'a http::Extensions) -> ConnectionInfo<'a>> {
    inner: S,
    /// The URI for the backend server.
    ///
    /// Invariant: This URI is guaranteed to have a path already and therefore is safe to join a
    /// path to. It also has no query.
    remote: http::Uri,
    connection_info: F,
    use_x_forwarded: bool,
}

impl<S> Gateway<S> {
    /// Creates a new `Gateway` that forwards requests to a given remote URI.
    ///
    /// Returns [`Err`] if the remote URI cannot have a path joined onto it (e.g. because it has an
    /// authority and no scheme).
    pub fn new(inner: S, remote: http::Uri) -> Result<Self, http::Error> {
        Ok(Self {
            inner,
            remote: validate_remote_uri(remote)?,
            connection_info: |_| ConnectionInfo::new(),
            use_x_forwarded: false,
        })
    }

    /// Sets the type to use to get info about the connection from the request.
    ///
    /// The type will be looked up in the request's extensions map.
    ///
    /// This is a convenience for `self.with_connection_info_fn(|ext| ext.get::<C>().into())`.
    ///
    /// # Example
    ///
    /// ```rust
    ///use tower_http::gateway::Gateway;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = hyper::Client::new();
    /// let gateway = Gateway::new(client, http::Uri::from_static("http://example.com:1234/api"))?
    ///     .with_connection_info::<std::net::SocketAddr>();
    /// # Ok(()) }
    /// ```
    pub fn with_connection_info<C>(
        self,
    ) -> Gateway<S, impl for<'a> FnMut(&'a http::Extensions) -> ConnectionInfo<'a>>
    where
        C: Send + Sync + 'static,
        for<'a> &'a C: Into<ConnectionInfo<'a>>,
    {
        self.with_connection_info_fn(|ext| ext.get::<C>().into())
    }

    /// Sets a function to use to get info about the connection from the request.
    ///
    /// This can be used to adapt connection info from other crates without having to add a layer
    /// that modifies the request extensions.
    ///
    /// # Example
    ///
    /// Adapting [`axum::extract::connect_info::ConnectInfo`][]:
    ///
    /// ```rust
    /// use axum::extract::connect_info::ConnectInfo;
    /// use std::net::SocketAddr;
    /// use tower_http::gateway::Gateway;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = hyper::Client::new();
    /// let gateway = Gateway::new(client, http::Uri::from_static("http://example.com:1234/api"))?
    ///     .with_connection_info_fn(|ext| ext.get::<ConnectInfo<SocketAddr>>().map(|x| &x.0).into());
    /// # Ok(()) }
    /// ```
    ///
    /// [`axum::extract::connect_info::ConnectInfo`]:
    ///     https://docs.rs/axum/0.5/axum/extract/connect_info/struct.ConnectInfo.html
    pub fn with_connection_info_fn<F>(self, f: F) -> Gateway<S, F>
    where
        F: for<'a> FnMut(&'a http::Extensions) -> ConnectionInfo<'a>,
    {
        Gateway {
            inner: self.inner,
            remote: self.remote,
            connection_info: f,
            use_x_forwarded: self.use_x_forwarded,
        }
    }
}

impl Gateway<()> {
    /// Returns a new [`Layer`] that wraps services with a [`GatewayLayer`] middleware.
    pub fn layer(remote: http::Uri) -> Result<GatewayLayer, http::Error> {
        GatewayLayer::new(remote)
    }
}

impl<S, F> Gateway<S, F> {
    /// Enables or disables the `X-Forwarded-*` headers on forwarded requests.
    ///
    /// If set to `true`, the forwarded request will have [`X-Forwarded-For`][],
    /// [`X-Forwarded-Host`][], and [`X-Forwarded-Proto`][] headers set accordingly, depending on
    /// the associated [`ConnectionInfo`] (see [`with_connection_info`] and
    /// [`with_connection_info_fn`]). If the corresponding fields on the [`ConnectionInfo`] are
    /// [`None`] (or obfuscated) the `X-Forwarded-*` header will be left unmodified.
    ///
    /// If set to `false` (the default), those headers will be ignored and any such headers already
    /// on the request will be retained as-is.
    ///
    /// Note: When the incoming request has any such headers and the [`ConnectionInfo`] does not
    /// specify unobfuscated values for all 3 headers, this may result in the forwarded request
    /// only updating some headers and not others, leading to the `X-Forwarded-*` headers having
    /// differing numbers of components.
    ///
    /// This setting should only be enabled when required. Whenever possible, the [`Forwarded`][]
    /// header should be preferred.
    ///
    /// [`X-Forwarded-For`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-For
    /// [`X-Forwarded-Host`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-Host
    /// [`X-Forwarded-Proto`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-Proto
    /// [`with_connection_info`]: fn@Self::with_connection_info
    /// [`with_connection_info_fn`]: fn@Self::with_connection_info_fn
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn use_x_forwarded(self, use_x_forwarded: bool) -> Self {
        Self {
            use_x_forwarded,
            ..self
        }
    }

    /// Gets a reference to the underlying service.
    pub fn get_ref(&self) -> &S {
        &self.inner
    }

    /// Gets a mutable reference to the underlying service.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Consumes `self`, returning the underlying service.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S, F, ReqBody, ResBody> Service<http::Request<ReqBody>> for Gateway<S, F>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
    F: for<'a> FnMut(&'a http::Extensions) -> ConnectionInfo<'a>,
    ResBody: Default,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<ReqBody>) -> Self::Future {
        // Disallow the CONNECT request type. It's the only one whose request-target is an absolute
        // URI, and joining that on a remote makes no sense.
        if req.method() == http::Method::CONNECT {
            // 405 Method Not Allowed sounds nice but we can't populate the `Allow` header as
            // required since we don't know what the backend allows. CONNECT isn't supposed to be
            // sent to origin servers anyway, so we'll just do a 400 Bad Request.
            let mut res = http::Response::default();
            *res.status_mut() = http::StatusCode::BAD_REQUEST;
            update_response_headers(&mut res); // just in case this is useful later
            return ResponseFuture {
                kind: Kind::Error {
                    response: Some(res),
                },
            };
        }

        let uri = uri_join(&self.remote, req.uri());
        self.update_request_headers(&mut req);
        *req.uri_mut() = uri;
        // Reset request version back to the default (HTTP/1.1), so the version used for the
        // incoming request doesn't affect our forwarded request.
        *req.version_mut() = Default::default();
        ResponseFuture {
            kind: Kind::Future {
                future: self.inner.call(req),
            },
        }
    }
}

impl<S, F> Gateway<S, F>
where
    F: for<'a> FnMut(&'a http::Extensions) -> ConnectionInfo<'a>,
{
    /// Updates the headers in the request as needed for forwarding.
    fn update_request_headers<ReqBody>(&mut self, req: &mut http::Request<ReqBody>) {
        // Calculate the Forwarded (and X-Forwarded-* if requested) headers
        let host = req.headers_mut().remove(http::header::HOST);
        let connection_info = (self.connection_info)(req.extensions());
        let forwarded = make_forwarded_header(&connection_info, host.as_ref());
        let x_forwarded = self
            .use_x_forwarded
            .then(|| make_x_forwarded_headers(&connection_info, host.as_ref()));

        // Calculate the Via header, if requested
        let via = make_via_header(req.version(), &connection_info);

        // Remove hop-by-hop headers
        let headers = req.headers_mut();
        remove_hop_by_hop_headers(headers);

        // Now append our new headers. Doing it in this order prevents the `Connection` header from
        // removing these headers.
        headers.append(http::header::FORWARDED, forwarded);
        for (name, value) in x_forwarded.into_iter().flatten() {
            headers.append(name, value);
        }
        if let Some(via) = via {
            headers.append(http::header::VIA, via);
        }
    }
}

fn validate_remote_uri(uri: http::Uri) -> Result<http::Uri, http::Error> {
    // Validate that the Uri has a path, or give it one if not. Ultimately this requires the
    // Uri to either be path-only or to have scheme/authority/path. We don't actually want
    // path-only but our wrapped service gets to deal with that.
    //
    // We're going to always split the Uri into parts first and then re-join it, even if it
    // says it has a path. This tests the specific path we care about (converting to Parts,
    // setting a path, and converting back), which we need to ensure the safety of our unwraps
    // later on. It also lets us trim off the query here so we don't need to do it later.
    let mut parts = uri.into_parts();
    parts.path_and_query = Some(match parts.path_and_query.take() {
        Some(path_and_query) if path_and_query.query().is_none() => path_and_query,
        Some(path_and_query) => path_and_query.path().parse()?,
        None => http::uri::PathAndQuery::from_static("/"),
    });
    Ok(http::Uri::from_parts(parts)?)
}

fn uri_join(base: &http::Uri, uri: &http::Uri) -> http::Uri {
    match uri.path_and_query() {
        Some(path_and_query) if path_and_query != "" && path_and_query != "/" => {
            // We need to join the remote with the request.
            let mut parts = base.clone().into_parts();
            let base_path_and_query = parts.path_and_query.take();
            let base_path = base_path_and_query.as_ref().map(|p| p.path().as_bytes());
            parts.path_and_query = Some(match base_path {
                None | Some(b"" | b"/") => {
                    // Our remote has no meaningful path, we can use the request path as-is
                    path_and_query.clone()
                }
                Some(base_path) => {
                    // http doesn't have any built-in way of joining paths or URIs so we must. If
                    // we do this with Bytes we can avoid the reallocation when converting to
                    // PathAndQuery as the latter uses Bytes internally. This isn't exposed in the
                    // API, but if it ever changes (or uses an incompatible version of bytes) then
                    // the failure mode is just an extra allocation.
                    let mut path_and_query = path_and_query.as_str().as_bytes();
                    let mut buf =
                        BytesMut::with_capacity(base_path.len() + 1 + path_and_query.len());
                    // Start with the base path
                    buf.extend_from_slice(base_path);
                    // Join the path_and_query, adding `/` if needed
                    match (buf.ends_with(b"/"), path_and_query.first().copied()) {
                        (true, Some(b'/')) => path_and_query = &path_and_query[1..], // don't double the slash
                        (true, _) | (false, Some(b'/' | b'?') | None) => {}
                        (false, _) => buf.extend_from_slice(b"/"),
                    }
                    buf.extend_from_slice(path_and_query);
                    http::uri::PathAndQuery::from_maybe_shared(buf.freeze())
                        .expect("buffer should only have path-safe characters")
                }
            });
            http::Uri::from_parts(parts).expect("base URI should be safe to join a path to")
        }
        _ => {
            // We have no path or query, we can use the base as-is
            base.clone()
        }
    }
}

/// Updates the headers in the response as needed for forwarding.
fn update_response_headers<ResBody>(response: &mut http::Response<ResBody>) {
    remove_hop_by_hop_headers(response.headers_mut());
}

fn remove_hop_by_hop_headers(headers: &mut http::HeaderMap) {
    use http::header::{
        Entry, CONNECTION, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE, TRAILER, TRANSFER_ENCODING,
        UPGRADE,
    };
    #[allow(clippy::declare_interior_mutable_const)] // it's the atomic refcount
    const KEEP_ALIVE: HeaderName = HeaderName::from_static("keep-alive"); // this one isn't in http::header

    // There are a group of hop-by-hop headers that should be removed by proxies as they're
    // intended to control this particular connection. RFC7230 mandates that every gateway
    // processes Connection and removes it and the listed names, so we'll do that with a few
    // exceptions. We're also going to strip some hop-by-hop headers even if they aren't listed
    // in Connection as that seems to be recommended behavior, especially if we're sending the
    // request over HTTP/2.

    // Strip Connection and the headers it references
    if let Entry::Occupied(mut entry) = headers.entry(CONNECTION) {
        // I'd love to use `headers::Connection` here but that doesn't offer any way to iterate its
        // values, and similarly `HeaderMap` doesn't offer a `retain()` method. We are optimizing
        // here for a single value.

        // http::header::ValueDrain currently (as of http v0.2.6) holds a hidden inner Vec
        // instead of reading data out of the map on demand. We want to avoid an unnecessary
        // vec allocation just to read this data, so we're going to instead move data out
        // without draining and then just remove the entry. This way our own Vec that we
        // allocate to let us drop the borrow on the map is the only allocation we need.

        let mut iter = entry
            .iter_mut()
            .map(|slot| {
                // We're going to swap each value with an empty one. This gives us owned values
                // without extra allocation or atomic operations (a static HeaderValue is backed by
                // a static Bytes which skips the reference counting).
                std::mem::replace(slot, HeaderValue::from_static(""))
            })
            .fuse();
        // Don't allocate a Vec if we only have one value
        let value = iter.next();
        let rest = iter.collect::<Vec<_>>();
        // We've pulled out all the header values, now remove the Connection header
        entry.remove();
        // The borrow on HeaderMap has now been dropped and we can mutate it.

        // Each header value is a comma-separated list of header names with optional whitespace
        // (defined as a space or tab).
        value
            .iter()
            .chain(&rest)
            // split on commas
            .flat_map(|value| value.as_bytes().split(|&b| b == b','))
            // convert to &str, trim whitespace
            .filter_map(|s| {
                // we need to go to &str now because AsHeaderName isn't implemented for &[u8]
                // even though HeaderName can be compared to &[u8]. This also makes it easier
                // to trim whitespace.
                Some(
                    std::str::from_utf8(s)
                        .ok()?
                        .trim_matches(&[' ', '\t'] as &[_]),
                )
            })
            .for_each(|s| {
                // skip the headers affecting body processing, we want to allow those
                if let Some(header) =
                    IntoIterator::into_iter([TE, TRANSFER_ENCODING, TRAILER]).find(|x| x == s)
                {
                    // Put the header back into Connection, we still need it there
                    headers.append(CONNECTION, header.into());
                } else {
                    // Remove all other headers
                    headers.remove(s);
                }
            })
    }

    // Strip most hop-by-hop headers even if they aren't in Connection. These headers aren't
    // supposed to do anything if they aren't in Connection but it's better to be safe,
    // especially if HTTP/2 is being used.
    for value in [KEEP_ALIVE, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, UPGRADE] {
        headers.remove(value);
    }
}

/// Constructs and returns a [`Forwarded`][] header.
///
/// This will always include a `for=` parameter, using `for=unknown` if the client IP is unknown.
///
/// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
fn make_forwarded_header(
    connection_info: &ConnectionInfo,
    host: Option<&HeaderValue>,
) -> HeaderValue {
    struct AddrPort<'a>(
        &'a Option<NodeIdentifier<'a, IpAddr>>,
        &'a Option<NodeIdentifier<'a, u16>>,
    );
    impl fmt::Display for AddrPort<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            // IpAddr, u16, and ObfuscatedIdentifier don't need any escaping, but some combinations
            // of these do need quoting.
            let quote = matches!(
                (self.0, self.1),
                (Some(NodeIdentifier::Value(IpAddr::V6(_))), _) | (_, Some(_))
            );
            if quote {
                f.write_char('"')?;
            }
            match (self.0, self.1) {
                (Some(NodeIdentifier::Value(IpAddr::V6(ip))), _) => write!(f, "[{}]", ip)?,
                (Some(ip), _) => write!(f, "{}", ip)?,
                (None, Some(_)) => f.write_str("unknown")?,
                (None, None) => {}
            }
            if let Some(port) = self.1 {
                write!(f, ":{}", port)?;
            }
            if quote {
                f.write_char('"')?;
            }
            Ok(())
        }
    }

    let mut bytes = BytesMut::new();
    match (&connection_info.local_ip, &connection_info.local_port) {
        (None, None) => {}
        (ip, port) => {
            let _ = write!(&mut bytes, "by={};", AddrPort(ip, port));
        }
    }
    bytes.extend_from_slice(b"for=");
    match (&connection_info.peer_ip, &connection_info.peer_port) {
        (None, None) => bytes.extend_from_slice(b"unknown"),
        (ip, port) => {
            let _ = write!(&mut bytes, "{}", AddrPort(ip, port));
        }
    }
    if let Some(host) = host.filter(|x| !x.is_empty()) {
        bytes.put_slice(b";host=");
        util::put_token_or_quoted(&mut bytes, &host);
    }
    if let Some(scheme) = &connection_info.scheme {
        bytes.put_slice(b";proto=");
        // Note: All valid schemes match the `token` rule and therefore don't need quoting
        bytes.put_slice(scheme.as_str().as_bytes());
    }
    // HeaderValue uses a Bytes internally, so if we pass that we will avoid a reallocation. It
    // doesn't publicly expose this but that's the current implementation. If that ever changes,
    // this code will still work, it may just start creating a new allocation.
    HeaderValue::from_maybe_shared(bytes.freeze()).expect("buffer should not contain control chars")
}

/// Constructs and returns [`X-Forwarded-For`][], [`X-Forwarded-Host`][], and
/// [`X-Forwarded-Proto`][] headers.
///
/// [`X-Forwarded-For`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-For
/// [`X-Forwarded-Host`]:
///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-Host
/// [`X-Forwarded-Proto`]:
///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-Proto
fn make_x_forwarded_headers(
    connection_info: &ConnectionInfo,
    host: Option<&HeaderValue>,
) -> impl Iterator<Item = (HeaderName, HeaderValue)> {
    IntoIterator::into_iter([
        // X-Forwarded-For
        connection_info
            .peer_ip
            .as_ref()
            .and_then(|ip| ip.as_ref().exposed())
            .map(|ip| {
                // HeaderValue uses Bytes internally, so we'll use that to avoid an allocation.
                let mut bytes = BytesMut::new();
                // Note: this header doesn't require square brackets around IPv6 addrs
                let _ = write!(bytes, "{}", ip);
                // Invariant: Our buffer does not contain any control chars
                (
                    HeaderName::from_static("x-forwarded-for"),
                    HeaderValue::from_maybe_shared(bytes.freeze())
                        .expect("buffer should not contain control chars"),
                )
            }),
        // X-Forwarded-Host
        host.cloned()
            .map(|host| (HeaderName::from_static("x-forwarded-host"), host)),
        // X-Forwarded-Proto
        connection_info.scheme.as_deref().map(|scheme| {
            (
                HeaderName::from_static("x-forwarded-proto"),
                HeaderValue::from_str(scheme.as_str())
                    .expect("Scheme should be ascii with no control chars"),
            )
        }),
    ])
    .flatten()
}

/// Constructs and returns a `Via` header.
fn make_via_header(
    version: http::Version,
    connection_info: &ConnectionInfo,
) -> Option<HeaderValue> {
    struct Addr(IpAddr);
    impl fmt::Display for Addr {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match &self.0 {
                IpAddr::V4(v4) => fmt::Display::fmt(v4, f),
                IpAddr::V6(v6) => write!(f, "[{}]", v6),
            }
        }
    }
    enum ReceivedBy<'a> {
        Host(NodeIdentifier<'a, Addr>, Option<u16>),
        Value(&'a HeaderValue),
    }
    let received_by = match (&connection_info.via_received_by, &connection_info.local_ip) {
        (Some(via_received_by), _) => ReceivedBy::Value(via_received_by.as_ref()),
        (None, Some(local_ip @ NodeIdentifier::Value(_))) => ReceivedBy::Host(
            local_ip.as_ref().map_exposed(|&ip| Addr(ip)),
            connection_info
                .local_port
                .as_ref()
                .and_then(|p| p.as_ref().exposed())
                .copied(),
        ),
        (None, Some(NodeIdentifier::Obfuscated(token))) => ReceivedBy::Host(token.into(), None),
        (None, None) => return None,
    };

    // HeaderValue uses a Bytes internally, so if we construct it that way we avoid a reallocation.
    let mut bytes = BytesMut::new();
    // The Via header states that for brevity, the protocol-name is omitted if it is "HTTP".
    const PREFIX: &[u8] = b"HTTP/";
    match connection_info.via_protocol.as_deref() {
        Some(via_protocol) => {
            let proto = via_protocol.as_bytes();
            bytes.extend_from_slice(proto.strip_prefix(PREFIX).unwrap_or(proto));
        }
        None => {
            // Version prints strings like "HTTP/1.1" from its Debug impl. I'm mildly nervous about
            // relying on this format as it's Debug, but there's no other way to get the version
            // string back out, and our only alternative is switching over all known versions and
            // failing if a version we don't know about is introduced.
            let _ = write!(&mut bytes, "{:?}", version);
            if bytes.starts_with(PREFIX) {
                bytes.copy_within(PREFIX.len().., 0);
                bytes.truncate(bytes.len() - PREFIX.len());
            }
        }
    }
    bytes.extend_from_slice(b" ");
    #[allow(clippy::unit_arg)]
    let _ = match received_by {
        ReceivedBy::Host(host, None) => write!(&mut bytes, "{}", host),
        ReceivedBy::Host(host, Some(port)) => write!(&mut bytes, "{}:{}", host, port),
        ReceivedBy::Value(value) => Ok(bytes.extend_from_slice(value.as_bytes())),
    };
    Some(
        HeaderValue::from_maybe_shared(bytes.freeze())
            .expect("buffer should not contain control chars"),
    )
}

// Note: The response future here is modeled after tower_http::auth::RequireAuthorization
pin_project! {
    /// Response future for [`Gateway`].
    pub struct ResponseFuture<F, B> {
        #[pin]
        kind: Kind<F, B>,
    }
}

pin_project! {
    #[project = KindProj]
    enum Kind<F, B> {
        Future {
            #[pin]
            future: F
        },
        Error {
            response: Option<http::Response<B>>,
        }
    }
}

impl<F, B, E> Future for ResponseFuture<F, B>
where
    F: Future<Output = Result<http::Response<B>, E>>,
{
    type Output = F::Output;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        match self.project().kind.project() {
            KindProj::Future { future } => future.poll(cx).map_ok(|mut res| {
                update_response_headers(&mut res);
                res
            }),
            KindProj::Error { response } => {
                let response = response
                    .take()
                    .expect("ResponseFuture should not be polled after returning Poll::Ready");
                Poll::Ready(Ok(response))
            }
        }
    }
}

impl<F> fmt::Debug for GatewayLayer<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewayLayer")
            .field("remote", &self.remote)
            .field("use_x_forwarded", &self.use_x_forwarded)
            .field(
                "connection_info",
                &format_args!("{}", std::any::type_name::<F>()),
            )
            .finish()
    }
}

impl<S: fmt::Debug, F> fmt::Debug for Gateway<S, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gateway")
            .field("remote", &self.remote)
            .field("use_x_forwarded", &self.use_x_forwarded)
            .field(
                "connection_info",
                &format_args!("{}", std::any::type_name::<F>()),
            )
            .field("inner", &self.inner)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::TryInto,
        net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    };

    use http::uri::Scheme;

    use super::*;

    #[test]
    fn forwarded_header() {
        macro_rules! assert_forwarded {
            ($conn:expr => $expected:expr) => { assert_forwarded!(@ $conn, None, $expected) };
            ($conn:expr, host: $host:expr => $expected:expr) => { assert_forwarded!(@ $conn, Some(&$host), $expected) };
            (@ $conn:expr, $host:expr, $expected:expr) => {
                assert_eq!(make_forwarded_header(&$conn.into(), $host), $expected);
            };
        }
        assert_forwarded!(() => "for=unknown");
        assert_forwarded!(IpAddr::from(Ipv4Addr::LOCALHOST) => "for=127.0.0.1");
        assert_forwarded!(IpAddr::from(Ipv6Addr::LOCALHOST) => "for=\"[::1]\"");
        assert_forwarded!(SocketAddr::from((Ipv4Addr::LOCALHOST, 1234)) => "for=\"127.0.0.1:1234\"");
        assert_forwarded!(ConnectionInfo::new().peer_port(Some(1234)) => "for=\"unknown:1234\"");
        assert_forwarded!(ConnectionInfo::new().peer_addr(Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 1234)))) => "for=\"127.0.0.1:1234\"");
        assert_forwarded!(ConnectionInfo::new().local_addr(Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 1234)))) => "by=\"127.0.0.1:1234\";for=unknown");

        assert_forwarded!(ConnectionInfo::new().scheme(Some(Scheme::HTTPS)) => "for=unknown;proto=https");
        assert_forwarded!((), host: HeaderValue::from_static("example.com") => "for=unknown;host=example.com");
        assert_forwarded!((), host: HeaderValue::from_static("invalid\"host\\") => r#"for=unknown;host="invalid\"host\\""#);

        assert_forwarded!(ConnectionInfo::new().obfuscated_peer_ip(Some("_hidden".try_into().unwrap())) => "for=_hidden");
        assert_forwarded!(ConnectionInfo::new()
            .obfuscated_peer_ip(Some("_hidden".try_into().unwrap()))
            .obfuscated_peer_port(Some("_private".try_into().unwrap()))
            => "for=\"_hidden:_private\"");
        assert_forwarded!(ConnectionInfo::new()
            .obfuscated_peer_ip(Some("_a.b_c-9".try_into().unwrap()))
            .peer_port(Some(6789))
            => "for=\"_a.b_c-9:6789\"");

        assert_forwarded!(ConnectionInfo::new()
            .local_ip(Some(Ipv4Addr::LOCALHOST))
            .local_port(Some(1234))
            => "by=\"127.0.0.1:1234\";for=unknown");
        assert_forwarded!(ConnectionInfo::new()
            .obfuscated_local_ip(Some("_hidden".try_into().unwrap()))
            .obfuscated_local_port(Some("_private".try_into().unwrap()))
            => "by=\"_hidden:_private\";for=unknown");

        assert_forwarded!(ConnectionInfo::new()
            .peer_ip(Some(Ipv6Addr::LOCALHOST))
            .obfuscated_peer_port(Some("_yes".try_into().unwrap()))
            .obfuscated_local_ip(Some("_borkbork".try_into().unwrap()))
            .local_port(Some(4321))
            .scheme(Some(Scheme::HTTPS)),
            host: HeaderValue::from_static("example.com")
            => "by=\"_borkbork:4321\";for=\"[::1]:_yes\";host=example.com;proto=https");
    }

    #[test]
    fn via_header() {
        const HTTP_09: http::Version = http::Version::HTTP_09;
        const HTTP_10: http::Version = http::Version::HTTP_10;
        const HTTP_11: http::Version = http::Version::HTTP_11;
        #[track_caller]
        fn assert_via<'a>(
            version: http::Version,
            connection_info: impl Into<ConnectionInfo<'a>>,
            expected: impl Into<Option<&'a str>>,
        ) {
            // Option<T> is only PartialEq with Option<T>
            match (
                make_via_header(version, &connection_info.into()),
                expected.into(),
            ) {
                (Some(header), Some(expected)) => assert_eq!(header, expected),
                (header @ Some(_), None) => assert_eq!(header, None),
                (None, expected @ Some(_)) => assert_eq!(None, expected),
                (None, None) => {}
            }
        }
        assert_via(HTTP_11, (), None);
        assert_via(HTTP_11, IpAddr::from(Ipv4Addr::LOCALHOST), None);
        assert_via(
            HTTP_11,
            ConnectionInfo::new().local_ip(Some(Ipv4Addr::LOCALHOST)),
            "1.1 127.0.0.1",
        );
        assert_via(
            HTTP_11,
            ConnectionInfo::new().local_ip(Some(Ipv6Addr::LOCALHOST)),
            "1.1 [::1]",
        );
        assert_via(
            HTTP_11,
            ConnectionInfo::new()
                .local_ip(Some(Ipv4Addr::LOCALHOST))
                .local_port(Some(42)),
            "1.1 127.0.0.1:42",
        );
        assert_via(
            HTTP_11,
            ConnectionInfo::new()
                .local_ip(Some(Ipv6Addr::LOCALHOST))
                .local_port(Some(42)),
            "1.1 [::1]:42",
        );
        assert_via(
            HTTP_11,
            ConnectionInfo::new()
                .local_ip(Some(Ipv4Addr::LOCALHOST))
                .obfuscated_local_port(Some("_port".try_into().unwrap())),
            "1.1 127.0.0.1",
        );
        assert_via(HTTP_11, ConnectionInfo::new().local_port(Some(42)), None);
        assert_via(
            HTTP_10,
            ConnectionInfo::new().obfuscated_local_ip(Some("_spork".try_into().unwrap())),
            "1.0 _spork",
        );
        assert_via(
            HTTP_10,
            ConnectionInfo::new()
                .obfuscated_local_ip(Some("_bork".try_into().unwrap()))
                .local_port(Some(42)),
            "1.0 _bork",
        );
        assert_via(
            HTTP_10,
            ConnectionInfo::new()
                .obfuscated_local_ip(Some("_bork".try_into().unwrap()))
                .obfuscated_local_port(Some("_port".try_into().unwrap())),
            "1.0 _bork",
        );

        assert_via(
            HTTP_11,
            ConnectionInfo::new().via_received_by(Some(HeaderValue::from_static("bork"))),
            "1.1 bork",
        );
        assert_via(
            HTTP_11,
            ConnectionInfo::new()
                .local_ip(Some(Ipv4Addr::LOCALHOST))
                .local_port(Some(42))
                .via_received_by(Some(HeaderValue::from_static("bork"))),
            "1.1 bork",
        );
        assert_via(
            HTTP_09,
            ConnectionInfo::new().via_received_by(Some(HeaderValue::from_static("foo (comment)"))),
            "0.9 foo (comment)",
        );
        assert_via(
            HTTP_11,
            ConnectionInfo::new()
                .local_ip(Some(Ipv4Addr::LOCALHOST))
                .via_protocol(Some(HeaderValue::from_static("WAT/2.0"))),
            "WAT/2.0 127.0.0.1",
        );
        // Ensure we strip the HTTP name even when it's custom
        assert_via(
            HTTP_11,
            ConnectionInfo::new()
                .via_received_by(Some(HeaderValue::from_static("bork")))
                .via_protocol(Some(HeaderValue::from_static("HTTP/2.0"))),
            "2.0 bork",
        );
    }
}
