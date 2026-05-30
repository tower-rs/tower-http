use std::convert::TryFrom;
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use std::task::{Context, Poll};

use http::{Method, Request, Response, Uri};
use tower_service::Service;

use super::future::ResponseFuture;
use super::{
    BypassFn, DebugFn, DefaultResponseForProtectionError, Origins, ProtectionError,
    ProtectionErrorKind, ResponseForProtectionError,
};

/// Middleware that enforces cross-origin request forgery (CSRF) protection.
///
/// See the [module docs](crate::csrf) for an example.
#[derive(Clone)]
#[must_use]
pub struct Csrf<S, T = DefaultResponseForProtectionError> {
    inner: S,
    insecure_bypass: Option<Arc<BypassFn>>,
    rejection_response: T,
    trusted_origins: Origins,
}

impl<S, T> Csrf<S, T> {
    pub(super) fn new(
        inner: S,
        insecure_bypass: Option<Arc<BypassFn>>,
        rejection_response: T,
        trusted_origins: Origins,
    ) -> Self {
        Self {
            inner,
            insecure_bypass,
            rejection_response,
            trusted_origins,
        }
    }

    pub(super) fn verify<Body>(&self, req: &Request<Body>) -> Result<(), ProtectionError> {
        // Deliberately not Method::is_safe: it also treats TRACE as safe, but the
        // reference implementation only exempts GET/HEAD/OPTIONS, so we match it here.
        if matches!(
            req.method(),
            &Method::GET | &Method::HEAD | &Method::OPTIONS
        ) {
            #[cfg(feature = "tracing")]
            tracing::trace!(uri = %req.uri().path(), "request passed: safe method");
            return Ok(());
        }

        let origin = req.headers().get("origin").map(|h| h.as_bytes());

        let origin_uri = origin
            .filter(|b| !b.is_empty())
            .and_then(|b| Uri::try_from(b).ok())
            .filter(|u| matches!(u.scheme_str(), Some("http" | "https")));

        let sec_fetch_site = req.headers().get("sec-fetch-site").map(|h| h.as_bytes());

        let is_exempt = || -> bool {
            let bypass = self
                .insecure_bypass
                .as_ref()
                .map_or(false, |bypass| bypass(req.method(), req.uri()));

            if bypass {
                #[cfg(feature = "tracing")]
                tracing::trace!(uri = %req.uri().path(), "request passed: bypassed");
                return true;
            }

            // Strict byte match of the raw Origin header against the registered
            // set, mirroring the Go reference's `trustedOrigins[Origin]`.
            let trusted = origin.map_or(false, |b| self.trusted_origins.contains(b));

            if trusted {
                #[cfg(feature = "tracing")]
                tracing::trace!(uri = %req.uri().path(), "request passed: trusted origin");
                return true;
            }

            false
        };

        // Fetch spec mandates lowercase here; exact byte match is intentional.
        match sec_fetch_site {
            Some(b"same-origin" | b"none") => {
                #[cfg(feature = "tracing")]
                tracing::trace!(uri = %req.uri().path(), "request passed: sec-fetch-site is same-origin or none");
                return Ok(());
            }
            None | Some(b"") => {} // fall through to Origin check
            Some(_) if is_exempt() => return Ok(()),
            Some(_) => {
                return Err(ProtectionError::new(
                    ProtectionErrorKind::CrossOriginRequest,
                ))
            }
        }

        if matches!(origin, None | Some(b"")) {
            #[cfg(feature = "tracing")]
            tracing::trace!(uri = %req.uri().path(), "request passed: neither sec-fetch-site nor origin header (same-origin or not a browser request)");
            return Ok(());
        }

        let host = req.headers().get("host").map(|h| h.as_bytes());

        // Mirrors the reference's `url.Parse(origin).Host == req.Host`. Per RFC 7230
        // §5.3, req.Host is the request-target authority (absolute-form URI / HTTP/2
        // `:authority`) if present, else the Host header. Byte-exact and scheme-blind,
        // so an http→https mismatch can't be caught here — we fail open (HSTS helps).
        let effective_host = req
            .uri()
            .authority()
            .map(|a| a.as_str().as_bytes())
            .or(host);

        if let (Some(uri), Some(effective_host)) = (&origin_uri, effective_host) {
            if uri.authority().map(|a| a.as_str().as_bytes()) == Some(effective_host) {
                #[cfg(feature = "tracing")]
                tracing::trace!(uri = %req.uri().path(), "request passed: origin is same as host");
                return Ok(());
            }
        }

        if is_exempt() {
            return Ok(());
        }

        Err(ProtectionError::new(
            ProtectionErrorKind::CrossOriginRequestFromOldBrowser,
        ))
    }
}

impl<S, T> Default for Csrf<S, T>
where
    S: Default,
    T: Default,
{
    fn default() -> Self {
        Self {
            inner: S::default(),
            insecure_bypass: None,
            rejection_response: T::default(),
            trusted_origins: Origins::default(),
        }
    }
}

impl<S: Debug, T> Debug for Csrf<S, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Csrf")
            .field("inner", &self.inner)
            .field(
                "insecure_bypass",
                &self.insecure_bypass.as_ref().map(|_| DebugFn),
            )
            .field("trusted_origins", &self.trusted_origins)
            .field("rejection_response", &DebugFn)
            .finish()
    }
}

impl<S, T, ReqBody, ResBody> Service<Request<ReqBody>> for Csrf<S, T>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    T: ResponseForProtectionError<ResBody>,
{
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;
    type Response = Response<ResBody>;

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        match self.verify(&req) {
            Ok(_) => ResponseFuture::future(self.inner.call(req)),
            Err(err) => {
                #[cfg(feature = "tracing")]
                tracing::trace!(uri = %req.uri().path(), error = %err, "request rejected");

                let mut response = self
                    .rejection_response
                    .response_for_protection_error(err.clone());

                response.extensions_mut().insert(err);

                ResponseFuture::rejected(Ok(response))
            }
        }
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Guards the comment in `verify`: `Method::is_safe` exempts more than the
    // GET/HEAD/OPTIONS set the reference implementation uses, so we can't rely on it.
    #[test]
    fn method_is_safe_covers_more_than_get_head_options() {
        for method in [&Method::GET, &Method::HEAD, &Method::OPTIONS] {
            assert!(method.is_safe());
        }

        // TRACE is "safe" per RFC 7231 but is not in the reference implementation's set.
        assert!(Method::TRACE.is_safe());
    }
}
