//! Tools for customizing the behavior of a [`FollowRedirect`][super::FollowRedirect] middleware.

mod and;
mod clone_body_fn;
mod limited;
mod redirect_fn;

pub use self::{
    and::And,
    clone_body_fn::{clone_body_fn, CloneBodyFn},
    limited::Limited,
    redirect_fn::{redirect_fn, RedirectFn},
};

use http::{StatusCode, Uri};
use std::fmt;

/// Trait for the policy on handling redirection responses.
///
/// # Example
///
/// Detecting a cyclic redirection:
///
/// ```
/// use http::{Request, Uri};
/// use std::collections::HashSet;
/// use tower_http::follow_redirect::policy::{Action, Attempt, Policy};
///
/// #[derive(Clone)]
/// pub struct DetectCycle {
///     uris: HashSet<Uri>,
/// }
///
/// impl<B> Policy<B> for DetectCycle {
///     fn redirect(&mut self, attempt: &Attempt<'_>) -> Action {
///         if self.uris.contains(attempt.location()) {
///             Action::stop()
///         } else {
///             self.uris.insert(attempt.previous().clone());
///             Action::follow()
///         }
///     }
/// }
/// ```
pub trait Policy<B> {
    /// Invoked when the service received a response with a redirection status code (`3xx`).
    ///
    /// This method returns an [`Action`] which indicates whether the service should follow
    /// the redirection.
    fn redirect(&mut self, attempt: &Attempt<'_>) -> Action;

    /// Try to clone a request body before the service makes a redirected request.
    ///
    /// If the request body cannot be cloned, return `None`.
    ///
    /// This is not invoked when [`B::size_hint`][http_body::Body::size_hint] returns zero,
    /// in which case `B::default()` will be used to create a new request body.
    ///
    /// The default implementation returns `None`.
    fn clone_body(&self, _body: &B) -> Option<B> {
        None
    }

    /// Create a new `Policy` that returns [`Action::follow()`] only if `self` and `other` returns
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
    /// use tower_http::follow_redirect::policy::{clone_body_fn, Limited, Policy};
    ///
    /// enum MyBody {
    ///     Bytes(Bytes),
    ///     Hyper(Body),
    /// }
    ///
    /// let policy = Limited::default().and(clone_body_fn(|body| {
    ///     if let MyBody::Bytes(buf) = body {
    ///         Some(MyBody::Bytes(buf.clone()))
    ///     } else {
    ///         None
    ///     }
    /// }));
    /// ```
    fn and<P>(self, other: P) -> And<Self, P>
    where
        Self: Sized,
        P: Policy<B>,
    {
        And::new(self, other)
    }
}

impl<B, P> Policy<B> for &mut P
where
    P: Policy<B> + ?Sized,
{
    fn redirect(&mut self, attempt: &Attempt<'_>) -> Action {
        (**self).redirect(attempt)
    }

    fn clone_body(&self, body: &B) -> Option<B> {
        (**self).clone_body(body)
    }
}

impl<B, P> Policy<B> for Box<P>
where
    P: Policy<B> + ?Sized,
{
    fn redirect(&mut self, attempt: &Attempt<'_>) -> Action {
        (**self).redirect(attempt)
    }

    fn clone_body(&self, body: &B) -> Option<B> {
        (**self).clone_body(body)
    }
}

/// A type that holds information on a redirection attempt.
pub struct Attempt<'a> {
    pub(crate) status: StatusCode,
    pub(crate) location: &'a Uri,
    pub(crate) previous: &'a Uri,
}

impl<'a> Attempt<'a> {
    /// Returns the redirection response.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns the destination URI of the redirection.
    pub fn location(&self) -> &'a Uri {
        self.location
    }

    /// Returns the URI of the original request.
    pub fn previous(&self) -> &'a Uri {
        self.previous
    }
}

/// A value returned by [`Policy::redirect`] which indicates the action
/// [`FollowRedirect`][super::FollowRedirect] should take for a redirection response.
#[derive(Clone)]
pub struct Action {
    pub(crate) kind: ActionKind,
}

#[derive(Clone)]
pub(crate) enum ActionKind {
    Follow,
    Stop,
}

impl Action {
    /// Create a new [`Action`] which indicates that a redirection should be followed.
    pub fn follow() -> Self {
        Action {
            kind: ActionKind::Follow,
        }
    }

    /// Create a new [`Action`] which indicates that a redirection should not be followed.
    pub fn stop() -> Self {
        Action {
            kind: ActionKind::Stop,
        }
    }

    /// Returns whether the `Action` instructs to follow a redirection.
    pub fn follows(&self) -> bool {
        match self.kind {
            ActionKind::Follow => true,
            ActionKind::Stop => false,
        }
    }

    /// Returns whether the `Action` instructs not to follow a redirection.
    pub fn stops(&self) -> bool {
        !self.follows()
    }
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        struct Follow;
        #[derive(Debug)]
        struct Stop;

        let mut debug = f.debug_tuple("Action");
        match self.kind {
            ActionKind::Follow => debug.field(&Follow),
            ActionKind::Stop => debug.field(&Stop),
        };
        debug.finish()
    }
}

impl<B> Policy<B> for Action {
    fn redirect(&mut self, _: &Attempt<'_>) -> Action {
        self.clone()
    }
}
