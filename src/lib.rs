#![forbid(unsafe_code)]

mod admin;
mod auth;
mod domains;
mod http;
mod input;
mod qr;
mod redirect;
mod store;

pub use admin::{AdminConfig, admin_router};
pub use auth::{Auth, BootstrapKind};
pub use http::public_router;
pub use input::PublicHosts;
pub use store::open_database;
