//! Authorization related middleware.

pub mod require_authorization;

#[doc(inline)]
pub use self::require_authorization::{
    AuthorizeRequest, RequireAuthorization, RequireAuthorizationLayer,
};
