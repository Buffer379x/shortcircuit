#![forbid(unsafe_code)]

mod admin;
mod auth;
mod http;
mod input;
mod store;

pub use admin::{AdminConfig, admin_router};
pub use auth::{Auth, BootstrapKind};
pub use http::public_router;
pub use input::PublicHosts;
pub use store::open_database;
