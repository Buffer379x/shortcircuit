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

#[derive(Clone, Debug)]
pub(crate) enum Network {
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
            .map(|network| Network::parse(network.trim(), PROXY_ERROR))
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
        self.0.iter().any(|network| network.contains(candidate))
    }
}

const PROXY_ERROR: &str =
    "TRUSTED_PROXIES must contain IP addresses or CIDR networks with nonzero prefixes";

/// IPv6 clients usually control a whole /64, so they share one budget per /64 prefix.
fn budget_key(peer: IpAddr) -> IpAddr {
    match peer.to_canonical() {
        IpAddr::V6(address) => IpAddr::V6(std::net::Ipv6Addr::from(
            u128::from(address) & (u128::MAX << 64),
        )),
        address => address,
    }
}

impl Network {
    pub(crate) fn contains(&self, candidate: IpAddr) -> bool {
        match (self, candidate.to_canonical()) {
            (Self::V4 { address, mask }, IpAddr::V4(candidate)) => {
                (u32::from(candidate) & *mask) == *address
            }
            (Self::V6 { address, mask }, IpAddr::V6(candidate)) => {
                (u128::from(candidate) & *mask) == *address
            }
            _ => false,
        }
    }

    /// The first address and the prefix length in bits.
    pub(crate) fn base(&self) -> (IpAddr, u32) {
        match self {
            Self::V4 { address, mask } => (IpAddr::V4(Ipv4Addr::from(*address)), mask.count_ones()),
            Self::V6 { address, mask } => (
                IpAddr::V6(std::net::Ipv6Addr::from(*address)),
                mask.count_ones(),
            ),
        }
    }

    pub(crate) fn parse(value: &str, error: &'static str) -> Result<Self, &'static str> {
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
    /// Protected-link passwords per link: attempts per client key and across all clients. The
    /// ceiling stays below the process-wide password budget, so guessing one link's password
    /// leaves attempts for every other link.
    pub const PASSWORD_LINK_DEFAULT: Self = Self {
        per_peer: 5,
        global: 30,
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
    /// An optional budget per object, such as a protected link.
    scoped: Option<RateLimit>,
    window: Mutex<Attempts>,
}

/// Counters of one fixed window. Only counted attempts create entries, and every counted
/// attempt also counts toward `total`, so each map holds at most `limit.global` entries until
/// the window resets and clears them.
struct Attempts {
    since: Instant,
    per_peer: HashMap<IpAddr, u32>,
    total: u32,
    per_scope: HashMap<i64, u32>,
    per_scope_peer: HashMap<(i64, IpAddr), u32>,
}

impl AttemptLimiter {
    pub(crate) fn with_limit(limit: RateLimit) -> Self {
        Self {
            limit,
            scoped: None,
            window: Mutex::new(Attempts {
                since: Instant::now(),
                per_peer: HashMap::new(),
                total: 0,
                per_scope: HashMap::new(),
                per_scope_peer: HashMap::new(),
            }),
        }
    }

    /// Adds a budget per scope: `per_peer` attempts per client key and scope, and `global`
    /// attempts per scope across all clients.
    pub(crate) fn with_scope_limit(limit: RateLimit, scoped: RateLimit) -> Self {
        Self {
            scoped: Some(scoped),
            ..Self::with_limit(limit)
        }
    }

    pub(crate) fn allow(&self, peer: IpAddr) -> Result<(), StatusCode> {
        self.check(peer, None)
    }

    /// Counts one attempt against the client, process, and scope budgets. A denied attempt
    /// counts toward none of them.
    pub(crate) fn allow_scoped(&self, peer: IpAddr, scope: i64) -> Result<(), StatusCode> {
        self.check(peer, Some(scope))
    }

    fn check(&self, peer: IpAddr, scope: Option<i64>) -> Result<(), StatusCode> {
        let mut window = self
            .window
            .lock()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if window.since.elapsed() >= WINDOW {
            window.since = Instant::now();
            window.per_peer.clear();
            window.total = 0;
            window.per_scope.clear();
            window.per_scope_peer.clear();
        }
        let key = budget_key(peer);
        let used = |count: Option<&u32>| count.copied().unwrap_or(0);
        if window.total >= self.limit.global
            || used(window.per_peer.get(&key)) >= self.limit.per_peer
        {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let scope = scope.zip(self.scoped);
        if let Some((scope, limit)) = scope
            && (used(window.per_scope.get(&scope)) >= limit.global
                || used(window.per_scope_peer.get(&(scope, key))) >= limit.per_peer)
        {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        *window.per_peer.entry(key).or_default() += 1;
        window.total += 1;
        if let Some((scope, _)) = scope {
            *window.per_scope.entry(scope).or_default() += 1;
            *window.per_scope_peer.entry((scope, key)).or_default() += 1;
        }
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
    fn ipv6_clients_share_a_budget_per_64_prefix() {
        let limiter = AttemptLimiter::with_limit(RateLimit::new(2, 100).unwrap());
        let first: IpAddr = "2001:db8:1:2::10".parse().unwrap();
        let neighbor: IpAddr = "2001:db8:1:2:ffff::1".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::10".parse().unwrap();
        assert!(limiter.allow(first).is_ok());
        assert!(limiter.allow(neighbor).is_ok());
        assert_eq!(limiter.allow(first), Err(StatusCode::TOO_MANY_REQUESTS));
        assert!(limiter.allow(other).is_ok());
        let mapped: IpAddr = "::ffff:192.0.2.1".parse().unwrap();
        assert_eq!(budget_key(mapped), "192.0.2.1".parse::<IpAddr>().unwrap());
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

    #[test]
    fn scoped_budgets_bound_one_link_per_client_and_across_clients() {
        let limiter = AttemptLimiter::with_scope_limit(
            RateLimit::new(8, 60).unwrap(),
            RateLimit::new(2, 3).unwrap(),
        );
        let first: IpAddr = "192.0.2.1".parse().unwrap();
        let second: IpAddr = "2001:db8:1:2::1".parse().unwrap();
        let neighbor: IpAddr = "2001:db8:1:2::2".parse().unwrap();
        let third: IpAddr = "192.0.2.3".parse().unwrap();
        assert_eq!(limiter.allow_scoped(first, 1), Ok(()));
        assert_eq!(limiter.allow_scoped(first, 1), Ok(()));
        // The client exhausted its budget for link 1, but not for link 2.
        assert_eq!(
            limiter.allow_scoped(first, 1),
            Err(StatusCode::TOO_MANY_REQUESTS)
        );
        assert_eq!(limiter.allow_scoped(first, 2), Ok(()));
        // A second client reaches the link's ceiling, which then stops everyone else.
        assert_eq!(limiter.allow_scoped(second, 1), Ok(()));
        assert_eq!(
            limiter.allow_scoped(third, 1),
            Err(StatusCode::TOO_MANY_REQUESTS)
        );
        assert_eq!(limiter.allow_scoped(third, 2), Ok(()));
        // A /64 neighbor shares the client's budget for a link.
        assert_eq!(limiter.allow_scoped(second, 3), Ok(()));
        assert_eq!(limiter.allow_scoped(neighbor, 3), Ok(()));
        assert_eq!(
            limiter.allow_scoped(second, 3),
            Err(StatusCode::TOO_MANY_REQUESTS)
        );
        // Denied attempts count toward no budget.
        let window = limiter.window.lock().unwrap();
        assert_eq!(window.total, 7);
        assert_eq!(window.per_scope.get(&1), Some(&3));
        assert_eq!(window.per_scope.get(&2), Some(&2));
        assert_eq!(window.per_scope.get(&3), Some(&2));
        assert_eq!(window.per_peer.get(&third), Some(&1));
    }

    #[test]
    fn scoped_maps_stay_within_the_process_budget() {
        let limiter = AttemptLimiter::with_scope_limit(
            RateLimit::new(1, 50).unwrap(),
            RateLimit::new(1, 1).unwrap(),
        );
        for index in 0..500_i64 {
            let peer = IpAddr::V4(Ipv4Addr::from(0xC000_0200_u32 + index as u32));
            let _ = limiter.allow_scoped(peer, index);
        }
        let window = limiter.window.lock().unwrap();
        assert_eq!(window.total, 50);
        assert!(window.per_peer.len() <= 50);
        assert!(window.per_scope.len() <= 50);
        assert!(window.per_scope_peer.len() <= 50);
        drop(window);
        // Without a scope limit, scoped calls apply only the client and process budgets.
        let plain = AttemptLimiter::with_limit(RateLimit::new(1, 2).unwrap());
        assert_eq!(plain.allow_scoped("192.0.2.9".parse().unwrap(), 7), Ok(()));
        assert!(plain.window.lock().unwrap().per_scope.is_empty());
    }
}
