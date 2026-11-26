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
const MAX_ATTEMPTS: u32 = 100_000;
const MAX_TRUSTED_PROXIES: usize = 64;
const MAX_FORWARDED_ADDRESSES: usize = 32;
const MAX_FORWARDED_BYTES: usize = 2048;

#[derive(Clone, Default)]
pub struct TrustedProxies(Vec<Network>);

#[derive(Clone)]
enum Network {
    V4 { address: u32, mask: u32 },
    V6 { address: u128, mask: u128 },
}

impl TrustedProxies {
    /// Accepts a comma-separated list of IP addresses and CIDR networks.
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        if value.trim().is_empty() {
            return Ok(Self::default());
        }
        let networks = value
            .split(',')
            .take(MAX_TRUSTED_PROXIES + 1)
            .map(|network| Network::parse(network.trim()))
            .collect::<Result<Vec<_>, _>>()?;
        if networks.len() > MAX_TRUSTED_PROXIES {
            return Err("TRUSTED_PROXIES allows at most 64 entries");
        }
        Ok(Self(networks))
    }

    pub fn client(&self, request: &Request) -> IpAddr {
        let Some(ConnectInfo(peer)) = request.extensions().get::<ConnectInfo<SocketAddr>>() else {
            return IpAddr::V4(Ipv4Addr::LOCALHOST);
        };
        let address = peer.ip().to_canonical();
        if !self.contains(address) {
            return address;
        }

        // A trusted ingress must overwrite the field or append the observed sender.
        // Walk back from the connection until the chain crosses the trust boundary.
        let fields = request.headers().get_all("x-forwarded-for");
        let mut values = fields.iter();
        let Some(value) = values.next().and_then(|field| field.to_str().ok()) else {
            return address;
        };
        if values.next().is_some() || value.len() > MAX_FORWARDED_BYTES {
            return address;
        }
        for (index, value) in value.rsplit(',').enumerate() {
            if index == MAX_FORWARDED_ADDRESSES {
                return address;
            }
            let Ok(candidate) = value.trim().parse::<IpAddr>() else {
                return address;
            };
            let candidate = candidate.to_canonical();
            if !self.contains(candidate) {
                return candidate;
            }
        }
        address
    }

    fn contains(&self, candidate: IpAddr) -> bool {
        self.0.iter().any(|network| match (network, candidate) {
            (Network::V4 { address, mask }, IpAddr::V4(candidate)) => {
                (u32::from(candidate) & *mask) == *address
            }
            (Network::V6 { address, mask }, IpAddr::V6(candidate)) => {
                (u128::from(candidate) & *mask) == *address
            }
            _ => false,
        })
    }
}

impl Network {
    fn parse(value: &str) -> Result<Self, &'static str> {
        let error =
            "TRUSTED_PROXIES must contain IP addresses or CIDR networks with nonzero prefixes";
        let (address, prefix) = match value.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix.parse::<u32>().map_err(|_| error)?)),
            None => (value, None),
        };
        let address = address.parse::<IpAddr>().map_err(|_| error)?;
        match (address, address.to_canonical()) {
            (IpAddr::V6(_), IpAddr::V4(address)) => {
                let prefix = prefix.unwrap_or(128);
                if !(97..=128).contains(&prefix) {
                    return Err(error);
                }
                Ok(Self::v4(address, prefix - 96))
            }
            (IpAddr::V4(address), _) => {
                let prefix = prefix.unwrap_or(32);
                if !(1..=32).contains(&prefix) {
                    return Err(error);
                }
                Ok(Self::v4(address, prefix))
            }
            (IpAddr::V6(address), _) => {
                let prefix = prefix.unwrap_or(128);
                if !(1..=128).contains(&prefix) {
                    return Err(error);
                }
                let mask = u128::MAX << (128 - prefix);
                Ok(Self::V6 {
                    address: u128::from(address) & mask,
                    mask,
                })
            }
        }
    }

    fn v4(address: Ipv4Addr, prefix: u32) -> Self {
        let mask = u32::MAX << (32 - prefix);
        Self::V4 {
            address: u32::from(address) & mask,
            mask,
        }
    }
}

#[derive(Clone, Copy)]
pub struct RateLimit {
    per_peer: u32,
    global: u32,
}

impl RateLimit {
    pub const PASSWORD_DEFAULT: Self = Self {
        per_peer: 8,
        global: 60,
    };
    pub const SENSITIVE_DEFAULT: Self = Self::PASSWORD_DEFAULT;
    pub const ADMIN_DEFAULT: Self = Self {
        per_peer: 240,
        global: 4_000,
    };

    pub fn new(per_peer: u32, global: u32) -> Result<Self, &'static str> {
        if per_peer == 0 || global < per_peer || global > MAX_ATTEMPTS {
            return Err("rate limits require 1 <= per-peer <= global <= 100000");
        }
        Ok(Self { per_peer, global })
    }

    pub fn per_peer(self) -> u32 {
        self.per_peer
    }

    pub fn global(self) -> u32 {
        self.global
    }
}

pub(crate) struct AttemptLimiter {
    limit: RateLimit,
    window: Mutex<Attempts>,
}

struct Attempts {
    since: Instant,
    per_peer: HashMap<IpAddr, u32>,
    total: u32,
}

impl AttemptLimiter {
    pub(crate) fn with_limit(limit: RateLimit) -> Self {
        Self {
            limit,
            window: Mutex::new(Attempts {
                since: Instant::now(),
                per_peer: HashMap::new(),
                total: 0,
            }),
        }
    }

    pub(crate) fn allow(&self, peer: IpAddr) -> Result<(), StatusCode> {
        let mut window = self
            .window
            .lock()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if window.since.elapsed() >= WINDOW {
            window.since = Instant::now();
            window.per_peer.clear();
            window.total = 0;
        }
        if window.total >= self.limit.global {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let count = window.per_peer.entry(peer).or_default();
        if *count >= self.limit.per_peer {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        *count += 1;
        window.total += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn request(peer: &str, forwarded: &str) -> Request {
        let mut request = Request::builder()
            .header("x-forwarded-for", forwarded)
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        request
    }

    #[test]
    fn uses_only_forwarding_from_a_trusted_immediate_peer() {
        let proxies = TrustedProxies::parse("192.0.2.0/24, 2001:db8::5").unwrap();
        let direct = request("198.51.100.1:8080", "203.0.113.10");
        assert_eq!(
            proxies.client(&direct),
            "198.51.100.1".parse::<IpAddr>().unwrap()
        );

        let chain = request(
            "192.0.2.4:8080",
            "ignored-prefix, 198.51.100.1, 2001:db8::5",
        );
        assert_eq!(
            proxies.client(&chain),
            "198.51.100.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn malformed_and_fully_trusted_chains_fall_back_to_the_peer() {
        let proxies = TrustedProxies::parse("192.0.2.0/24").unwrap();
        let peer = "192.0.2.4".parse::<IpAddr>().unwrap();
        assert_eq!(
            proxies.client(&request("[::ffff:192.0.2.4]:8080", "198.51.100.1, unknown")),
            peer
        );
        assert_eq!(
            proxies.client(&request("192.0.2.4:8080", "192.0.2.7")),
            peer
        );
        let mut duplicate = request("192.0.2.4:8080", "198.51.100.1");
        duplicate
            .headers_mut()
            .append("x-forwarded-for", "203.0.113.10".parse().unwrap());
        assert_eq!(proxies.client(&duplicate), peer);
        assert!(TrustedProxies::parse("0.0.0.0/0").is_err());
        assert!(TrustedProxies::parse("192.0.2.4,").is_err());
    }

    #[test]
    fn limits_requests_per_peer_and_globally_without_counting_denials() {
        let limiter = AttemptLimiter::with_limit(RateLimit::new(1, 2).unwrap());
        let first = "192.0.2.1".parse().unwrap();
        let second = "192.0.2.2".parse().unwrap();
        let third = "192.0.2.3".parse().unwrap();
        assert_eq!(limiter.allow(first), Ok(()));
        assert_eq!(limiter.allow(first), Err(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(limiter.allow(second), Ok(()));
        assert_eq!(limiter.allow(third), Err(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(limiter.window.lock().unwrap().per_peer.len(), 2);
    }
}
