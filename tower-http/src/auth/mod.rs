//! Authorization related middleware.

pub mod add_authorization;
pub mod require_authorization;

#[doc(inline)]
pub use self::{
    add_authorization::{AddAuthorization, AddAuthorizationLayer},
    require_authorization::{AuthorizeRequest, RequireAuthorization, RequireAuthorizationLayer},
};
