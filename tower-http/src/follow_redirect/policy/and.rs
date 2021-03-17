use super::{Action, ActionKind, Attempt, Policy};
use http::Request;

/// A redirection [`Policy`] that combines the results of two `Policy`s.
///
/// See [`Policy::and`] for more details.
#[derive(Clone, Copy, Debug, Default)]
pub struct And<A, B> {
    a: A,
    b: B,
}

impl<A, B> And<A, B> {
    pub(crate) fn new(a: A, b: B) -> Self {
        And { a, b }
    }
}

impl<Bd, A, B> Policy<Bd> for And<A, B>
where
    A: Policy<Bd>,
    B: Policy<Bd>,
{
    fn redirect(&mut self, attempt: &Attempt<'_>) -> Action {
        match self.a.redirect(attempt).kind {
            ActionKind::Follow => self.b.redirect(attempt),
            ActionKind::Stop => Action::stop(),
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

    impl<B, P> Policy<B> for Taint<P>
    where
        P: Policy<B>,
    {
        fn redirect(&mut self, attempt: &Attempt<'_>) -> Action {
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
        let mut policy = And::new(&mut a, &mut b);
        assert!(Policy::<()>::redirect(&mut policy, &attempt).follows());
        assert!(a.used);
        assert!(b.used);

        let mut a = Taint::new(Action::stop());
        let mut b = Taint::new(Action::follow());
        let mut policy = And::new(&mut a, &mut b);
        assert!(Policy::<()>::redirect(&mut policy, &attempt).stops());
        assert!(a.used);
        assert!(!b.used); // short-circuiting

        let mut a = Taint::new(Action::follow());
        let mut b = Taint::new(Action::stop());
        let mut policy = And::new(&mut a, &mut b);
        assert!(Policy::<()>::redirect(&mut policy, &attempt).stops());
        assert!(a.used);
        assert!(b.used);

        let mut a = Taint::new(Action::stop());
        let mut b = Taint::new(Action::stop());
        let mut policy = And::new(&mut a, &mut b);
        assert!(Policy::<()>::redirect(&mut policy, &attempt).stops());
        assert!(a.used);
        assert!(!b.used);
    }
}
