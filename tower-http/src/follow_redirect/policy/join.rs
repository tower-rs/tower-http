use super::{Action, Attempt, Policy};
use http::Request;

/// A redirection [`Policy`] that combines the results of two `Policy`s.
///
/// See [`join`] for more details.
#[derive(Clone, Copy, Debug, Default)]
pub struct Join<A, B> {
    a: A,
    b: B,
}

impl<Bd, E, A, B> Policy<Bd, E> for Join<A, B>
where
    A: Policy<Bd, E>,
    B: Policy<Bd, E>,
{
    fn redirect(&mut self, attempt: &Attempt<'_>) -> Action<E> {
        let a = self.a.redirect(attempt);
        if a.follows() {
            self.b.redirect(attempt)
        } else {
            a
        }
    }

    fn on_request(&mut self, request: &mut Request<Bd>) {
        self.a.on_request(request);
        self.b.on_request(request);
    }

    fn clone_body(&self, body: &Bd) -> Option<Bd> {
        self.a.clone_body(body).or_else(|| self.b.clone_body(body))
    }
}

/// Create a new `Policy` that returns [`Action::follow()`] only if `self` and `other` return
/// `Action::follow()`.
///
/// [`clone_body`][Policy::clone_body] method of the returned `Policy` tries to clone the body
/// with both policies.
///
/// # Example
///
/// ```
/// use bytes::Bytes;
/// use hyper::Body;
/// use tower_http::follow_redirect::policy::{self, clone_body_fn, Limited, Policy};
///
/// enum MyBody {
///     Bytes(Bytes),
///     Hyper(Body),
/// }
///
/// let policy = policy::join::<_, _, _, ()>(
///     Limited::default(),
///     clone_body_fn(|body| {
///         if let MyBody::Bytes(buf) = body {
///             Some(MyBody::Bytes(buf.clone()))
///         } else {
///             None
///         }
///     }),
/// );
/// ```
pub fn join<A, B, Bd, E>(a: A, b: B) -> Join<A, B>
where
    A: Policy<Bd, E>,
    B: Policy<Bd, E>,
{
    Join { a, b }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Uri;

    struct Taint<P> {
        policy: P,
        used: bool,
    }

    impl<P> Taint<P> {
        fn new(policy: P) -> Self {
            Taint {
                policy,
                used: false,
            }
        }
    }

    impl<B, E, P> Policy<B, E> for Taint<P>
    where
        P: Policy<B, E>,
    {
        fn redirect(&mut self, attempt: &Attempt<'_>) -> Action<E> {
            self.used = true;
            self.policy.redirect(attempt)
        }
    }

    #[test]
    fn redirect() {
        let attempt = Attempt {
            status: Default::default(),
            location: &Uri::from_static("*"),
            previous: &Uri::from_static("*"),
        };

        let mut a = Taint::new(Action::follow());
        let mut b = Taint::new(Action::follow());
        let mut policy = join::<_, _, (), _>(&mut a, &mut b);
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt).follows());
        assert!(a.used);
        assert!(b.used);

        let mut a = Taint::new(Action::stop());
        let mut b = Taint::new(Action::follow());
        let mut policy = join::<_, _, (), _>(&mut a, &mut b);
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt).stops());
        assert!(a.used);
        assert!(!b.used); // short-circuiting

        let mut a = Taint::new(Action::follow());
        let mut b = Taint::new(Action::stop());
        let mut policy = join::<_, _, (), _>(&mut a, &mut b);
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt).stops());
        assert!(a.used);
        assert!(b.used);

        let mut a = Taint::new(Action::stop());
        let mut b = Taint::new(Action::stop());
        let mut policy = join::<_, _, (), _>(&mut a, &mut b);
        assert!(Policy::<(), ()>::redirect(&mut policy, &attempt).stops());
        assert!(a.used);
        assert!(!b.used);
    }
}
