use super::{eq_origin, Action, Attempt, Policy};
use http::{
    header::{self, HeaderName},
    Extensions, Request,
};

/// A redirection [`Policy`] that removes credentials from requests in redirections.
///
/// In addition to request headers, this policy also filters request
/// [`Extensions`] in "blocked" redirections. Because extensions are keyed by
/// arbitrary user types, there is no built-in blocklist of sensitive extensions to mirror the
/// header blocklist. Instead, blocked redirections drop *all* extensions by default, and callers
/// opt specific types back in with [`allow_extension`][Self::allow_extension].
#[derive(Clone, Debug)]
pub struct FilterCredentials {
    block_cross_origin: bool,
    block_any: bool,
    remove_blocklisted: bool,
    remove_all: bool,
    remove_all_extensions: bool,
    extension_allowlist: Vec<fn(&mut Extensions, &mut Extensions)>,
    blocked: bool,
}

const BLOCKLIST: &[HeaderName] = &[
    header::AUTHORIZATION,
    header::COOKIE,
    header::PROXY_AUTHORIZATION,
];

impl FilterCredentials {
    /// Create a new [`FilterCredentials`] that removes blocklisted request headers in cross-origin
    /// redirections.
    pub fn new() -> Self {
        FilterCredentials {
            block_cross_origin: true,
            block_any: false,
            remove_blocklisted: true,
            remove_all: false,
            remove_all_extensions: true,
            extension_allowlist: Vec::new(),
            blocked: false,
        }
    }

    /// Configure `self` to mark cross-origin redirections as "blocked".
    pub fn block_cross_origin(mut self, enable: bool) -> Self {
        self.block_cross_origin = enable;
        self
    }

    /// Configure `self` to mark every redirection as "blocked".
    pub fn block_any(mut self) -> Self {
        self.block_any = true;
        self
    }

    /// Configure `self` to mark no redirections as "blocked".
    pub fn block_none(mut self) -> Self {
        self.block_any = false;
        self.block_cross_origin(false)
    }

    /// Configure `self` to remove blocklisted headers in "blocked" redirections.
    ///
    /// The blocklist includes the following headers:
    ///
    /// - `Authorization`
    /// - `Cookie`
    /// - `Proxy-Authorization`
    pub fn remove_blocklisted(mut self, enable: bool) -> Self {
        self.remove_blocklisted = enable;
        self
    }

    /// Configure `self` to remove all headers in "blocked" redirections.
    pub fn remove_all(mut self) -> Self {
        self.remove_all = true;
        self
    }

    /// Configure `self` to remove no headers in "blocked" redirections.
    pub fn remove_none(mut self) -> Self {
        self.remove_all = false;
        self.remove_blocklisted(false)
    }

    /// Configure `self` to remove all request extensions in "blocked" redirections, except for
    /// types added to the allowlist with [`allow_extension`][Self::allow_extension].
    ///
    /// This is the default. Because the library cannot know which extension types carry sensitive
    /// data, blocked redirections (cross-origin by default) drop every extension unless it has
    /// been explicitly allowed.
    pub fn remove_all_extensions(mut self) -> Self {
        self.remove_all_extensions = true;
        self
    }

    /// Configure `self` to keep all request extensions in "blocked" redirections.
    ///
    /// This forwards every extension across the redirection boundary, including cross-origin ones.
    /// Only use this when the extensions are known not to carry sensitive, origin-scoped data.
    pub fn keep_all_extensions(mut self) -> Self {
        self.remove_all_extensions = false;
        self
    }

    /// Add the extension type `T` to the allowlist, keeping it in "blocked" redirections even when
    /// other extensions are removed.
    ///
    /// This has no effect if extension removal has been disabled with
    /// [`keep_all_extensions`][Self::keep_all_extensions].
    pub fn allow_extension<T>(mut self) -> Self
    where
        T: Clone + Send + Sync + 'static,
    {
        self.extension_allowlist.push(|from, to| {
            if let Some(value) = from.remove::<T>() {
                to.insert(value);
            }
        });
        self
    }
}

impl Default for FilterCredentials {
    fn default() -> Self {
        Self::new()
    }
}

impl<B, E> Policy<B, E> for FilterCredentials {
    fn redirect(&mut self, attempt: &Attempt<'_>) -> Result<Action, E> {
        self.blocked = self.block_any
            || (self.block_cross_origin && !eq_origin(attempt.previous(), attempt.location()));
        Ok(Action::Follow)
    }

    fn on_request(&mut self, request: &mut Request<B>) {
        if self.blocked {
            let headers = request.headers_mut();
            if self.remove_all {
                headers.clear();
            } else if self.remove_blocklisted {
                for key in BLOCKLIST {
                    headers.remove(key);
                }
            }

            if self.remove_all_extensions {
                let extensions = request.extensions_mut();
                if self.extension_allowlist.is_empty() {
                    extensions.clear();
                } else {
                    let mut allowed = Extensions::new();
                    for transfer in &self.extension_allowlist {
                        transfer(extensions, &mut allowed);
                    }
                    *extensions = allowed;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{Method, Uri};

    #[test]
    fn works() {
        let mut policy = FilterCredentials::default();

        let initial = Uri::from_static("http://example.com/old");
        let same_origin = Uri::from_static("http://example.com/new");
        let cross_origin = Uri::from_static("https://example.com/new");

        let mut request = Request::builder()
            .uri(initial)
            .header(header::COOKIE, "42")
            .body(())
            .unwrap();
        Policy::<(), ()>::on_request(&mut policy, &mut request);
        assert!(request.headers().contains_key(header::COOKIE));

        let attempt = Attempt {
            status: Default::default(),
            method: &Method::GET,
            location: &same_origin,
            previous_method: &Method::GET,
            previous: request.uri(),
        };
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt)
            .unwrap()
            .is_follow());

        let mut request = Request::builder()
            .uri(same_origin)
            .header(header::COOKIE, "42")
            .body(())
            .unwrap();
        Policy::<(), ()>::on_request(&mut policy, &mut request);
        assert!(request.headers().contains_key(header::COOKIE));

        let attempt = Attempt {
            status: Default::default(),
            method: &Method::GET,
            location: &cross_origin,
            previous_method: &Method::GET,
            previous: request.uri(),
        };
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt)
            .unwrap()
            .is_follow());

        let mut request = Request::builder()
            .uri(cross_origin)
            .header(header::COOKIE, "42")
            .body(())
            .unwrap();
        Policy::<(), ()>::on_request(&mut policy, &mut request);
        assert!(!request.headers().contains_key(header::COOKIE));
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Kept(u32);

    #[derive(Clone, Debug, PartialEq)]
    struct Dropped(u32);

    fn cross_origin_attempt<'a>(previous: &'a Uri, location: &'a Uri) -> Attempt<'a> {
        Attempt {
            status: Default::default(),
            method: &Method::GET,
            location,
            previous_method: &Method::GET,
            previous,
        }
    }

    #[test]
    fn extensions_are_kept_same_origin_and_dropped_cross_origin() {
        let initial = Uri::from_static("http://example.com/old");
        let same_origin = Uri::from_static("http://example.com/new");
        let cross_origin = Uri::from_static("https://example.com/new");

        let mut policy = FilterCredentials::default();

        let attempt = cross_origin_attempt(&initial, &same_origin);
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt)
            .unwrap()
            .is_follow());
        let mut request = Request::builder().uri(&same_origin).body(()).unwrap();
        request.extensions_mut().insert(Kept(42));
        Policy::<(), ()>::on_request(&mut policy, &mut request);
        assert_eq!(request.extensions().get::<Kept>(), Some(&Kept(42)));

        let attempt = cross_origin_attempt(&same_origin, &cross_origin);
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt)
            .unwrap()
            .is_follow());
        let mut request = Request::builder().uri(&cross_origin).body(()).unwrap();
        request.extensions_mut().insert(Kept(42));
        Policy::<(), ()>::on_request(&mut policy, &mut request);
        assert!(request.extensions().get::<Kept>().is_none());
    }

    #[test]
    fn allowlisted_extensions_survive_cross_origin() {
        let initial = Uri::from_static("http://example.com/old");
        let cross_origin = Uri::from_static("https://example.com/new");

        let mut policy = FilterCredentials::default().allow_extension::<Kept>();
        let attempt = cross_origin_attempt(&initial, &cross_origin);
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt)
            .unwrap()
            .is_follow());

        let mut request = Request::builder().uri(&cross_origin).body(()).unwrap();
        request.extensions_mut().insert(Kept(1));
        request.extensions_mut().insert(Dropped(2));
        Policy::<(), ()>::on_request(&mut policy, &mut request);
        assert_eq!(request.extensions().get::<Kept>(), Some(&Kept(1)));
        assert!(request.extensions().get::<Dropped>().is_none());
    }

    #[test]
    fn keep_all_extensions_forwards_cross_origin() {
        let initial = Uri::from_static("http://example.com/old");
        let cross_origin = Uri::from_static("https://example.com/new");

        let mut policy = FilterCredentials::default().keep_all_extensions();
        let attempt = cross_origin_attempt(&initial, &cross_origin);
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt)
            .unwrap()
            .is_follow());

        let mut request = Request::builder().uri(&cross_origin).body(()).unwrap();
        request.extensions_mut().insert(Kept(1));
        Policy::<(), ()>::on_request(&mut policy, &mut request);
        assert_eq!(request.extensions().get::<Kept>(), Some(&Kept(1)));
    }

    #[test]
    fn allow_extension_is_ignored_when_keeping_all() {
        let initial = Uri::from_static("http://example.com/old");
        let cross_origin = Uri::from_static("https://example.com/new");

        // The allowlist only takes effect while extensions are being removed; keep_all disables
        // removal, so everything is forwarded regardless of the allowlist.
        let mut policy = FilterCredentials::default()
            .keep_all_extensions()
            .allow_extension::<Kept>();
        let attempt = cross_origin_attempt(&initial, &cross_origin);
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt)
            .unwrap()
            .is_follow());

        let mut request = Request::builder().uri(&cross_origin).body(()).unwrap();
        request.extensions_mut().insert(Kept(1));
        request.extensions_mut().insert(Dropped(2));
        Policy::<(), ()>::on_request(&mut policy, &mut request);
        assert_eq!(request.extensions().get::<Kept>(), Some(&Kept(1)));
        assert_eq!(request.extensions().get::<Dropped>(), Some(&Dropped(2)));
    }
}
