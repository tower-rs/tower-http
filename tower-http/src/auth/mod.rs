//! Authorization related middleware.

pub mod add_authorization;
pub mod require_authorization;
pub mod async_require_authorization;

#[doc(inline)]
pub use self::{
    add_authorization::{AddAuthorization, AddAuthorizationLayer},
    require_authorization::{AuthorizeRequest, RequireAuthorization, RequireAuthorizationLayer},
    async_require_authorization::{
        AsyncAuthorizeRequest, AsyncRequireAuthorization, AsyncRequireAuthorizationLayer,
    },
};
