#![forbid(unsafe_code)]

mod admin;
mod analytics;
mod auth;
mod domains;
mod http;
mod input;
mod qr;
mod redirect;
mod store;

pub use admin::{AdminConfig, admin_router, admin_router_with_analytics};
pub use analytics::Analytics;
pub use auth::{Auth, BootstrapKind};
pub use http::{public_router, public_router_with_analytics};
pub use input::PublicHosts;
pub use store::open_database;
