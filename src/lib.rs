#![forbid(unsafe_code)]

mod http;
mod input;
mod store;

pub use http::{AdminKey, admin_router, public_router};
pub use input::PublicHosts;
pub use store::open_database;
