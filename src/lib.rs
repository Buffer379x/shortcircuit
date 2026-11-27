#![forbid(unsafe_code)]

mod admin;
mod analytics;
mod auth;
mod backup;
mod backup_inventory;
mod backup_schedule;
mod confirmation;
mod destinations;
mod domains;
mod history;
mod http;
mod input;
mod monitor;
mod notifications;
mod outbound;
mod protection;
mod qr;
mod rate_limit;
mod redirect;
mod resources;
mod restore;
mod store;
mod titles;

pub use admin::{
    AdminConfig, AdminServices, Operations, admin_router, admin_router_with,
    admin_router_with_analytics,
};
pub use analytics::{Analytics, DroppedCounter};
pub use auth::{Auth, AuthError, BootstrapKind};
pub use backup::create_snapshot;
pub use backup_schedule::BackupSchedule;
pub use http::{
    PublicControls, public_router, public_router_on_port, public_router_with_analytics,
    public_router_with_controls,
};
pub use input::PublicHosts;
pub use monitor::Monitor;
pub use notifications::Notifier;
pub use rate_limit::{RateLimit, TrustedProxies};
pub use restore::{apply_pending_restore, data_lock, restore_snapshot};
pub use store::{open_database, open_database_with_timeout};
pub use titles::TitleLookup;

/// The source revision embedded at build time, if one was supplied.
pub fn build_revision() -> Option<&'static str> {
    [
        option_env!("SHORTCIRCUIT_BUILD_REVISION"),
        option_env!("GITHUB_SHA"),
    ]
    .into_iter()
    .flatten()
    .find(|revision| {
        (7..=64).contains(&revision.len()) && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}
