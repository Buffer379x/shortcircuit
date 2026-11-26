use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request},
    http::StatusCode,
};

const WINDOW: Duration = Duration::from_secs(60);
const PEER_ATTEMPTS: u8 = 8;
const GLOBAL_ATTEMPTS: u8 = 60;

pub(crate) fn peer(request: &Request) -> IpAddr {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |info| info.0.ip())
}

pub(crate) struct AttemptLimiter(Mutex<Attempts>);

struct Attempts {
    since: Instant,
    per_peer: HashMap<IpAddr, u8>,
    total: u8,
}

impl AttemptLimiter {
    pub(crate) fn new() -> Self {
        Self(Mutex::new(Attempts {
            since: Instant::now(),
            per_peer: HashMap::new(),
            total: 0,
        }))
    }

    pub(crate) fn allow(&self, peer: IpAddr) -> Result<(), StatusCode> {
        let mut window = self.0.lock().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if window.since.elapsed() >= WINDOW {
            window.since = Instant::now();
            window.per_peer.clear();
            window.total = 0;
        }
        if window.total >= GLOBAL_ATTEMPTS {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let count = window.per_peer.entry(peer).or_default();
        if *count >= PEER_ATTEMPTS {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        *count += 1;
        window.total += 1;
        Ok(())
    }
}
