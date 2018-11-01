extern crate futures;
extern crate http;
extern crate tower_add_origin;
extern crate tower_service;

pub mod add_origin {
    pub use ::tower_add_origin::{
        AddOrigin,
        Builder,
        BuilderError,
    };
}
pub mod service;
mod trailers;

pub use add_origin::AddOrigin;
pub use service::HttpService;
pub use trailers::BodyTrailers;
