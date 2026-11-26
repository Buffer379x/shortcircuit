use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use reqwest::{
    Client, Method, Response, StatusCode,
    dns::{Name, Resolve, Resolving},
    header::{ACCEPT_ENCODING, CONTENT_TYPE, LOCATION},
};
use tokio::{net::lookup_host, sync::Semaphore, time::timeout};
use url::{Host, Url};

const DEADLINE: Duration = Duration::from_secs(8);
const MAX_HOPS: u8 = 3;
const MAX_ADDRESSES: usize = 16;
const MAX_URL: usize = 4096;
const MAX_HEADERS: usize = 16 * 1024;

pub(crate) struct Fetcher {
    permits: Semaphore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Failure {
    Blocked,
    Busy,
    Dns,
    Timeout,
    Connection,
    Protocol,
}

#[derive(serde::Serialize)]
pub(crate) struct Probe {
    pub(crate) status: u16,
    pub(crate) redirects: u8,
    pub(crate) elapsed_ms: u64,
}

impl Fetcher {
    pub(crate) fn new() -> Self {
        Self {
            permits: Semaphore::new(4),
        }
    }

    pub(crate) async fn probe(&self, target: &str) -> Result<Probe, Failure> {
        let _permit = self.permits.try_acquire().map_err(|_| Failure::Busy)?;
        let started = Instant::now();
        let url = parse_url(target)?;
        let (response, redirects) = timeout(DEADLINE, follow(url, Method::HEAD))
            .await
            .map_err(|_| Failure::Timeout)??;
        Ok(Probe {
            status: response.status().as_u16(),
            redirects,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    pub(crate) async fn deliver(&self, target: &str, json: Vec<u8>) -> Result<(), Failure> {
        let _permit = self.permits.try_acquire().map_err(|_| Failure::Busy)?;
        let url = parse_url(target)?;
        if url.scheme() != "https" {
            return Err(Failure::Blocked);
        }
        timeout(DEADLINE, async {
            let client = pinned_client(&url).await?;
            let response = client
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .body(json)
                .send()
                .await
                .map_err(request_failure)?;
            check_headers(&response)?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(Failure::Protocol)
            }
        })
        .await
        .map_err(|_| Failure::Timeout)?
    }
}

async fn follow(mut url: Url, mut method: Method) -> Result<(Response, u8), Failure> {
    for hops in 0..=MAX_HOPS {
        let client = pinned_client(&url).await?;
        let mut response = send(&client, method.clone(), url.clone()).await?;
        if method == Method::HEAD
            && matches!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
            )
        {
            method = Method::GET;
            drop(response);
            response = send(&client, method.clone(), url.clone()).await?;
        }
        if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            return Ok((response, hops));
        }
        if hops == MAX_HOPS {
            return Err(Failure::Protocol);
        }
        let mut locations = response.headers().get_all(LOCATION).iter();
        let location = locations
            .next()
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty() && value.len() <= MAX_URL)
            .ok_or(Failure::Protocol)?;
        if locations.next().is_some() {
            return Err(Failure::Protocol);
        }
        let next = url.join(location).map_err(|_| Failure::Protocol)?;
        url = validate_url(next)?;
    }
    Err(Failure::Protocol)
}

async fn send(client: &Client, method: Method, url: Url) -> Result<Response, Failure> {
    let mut request = client.request(method.clone(), url);
    if method == Method::GET {
        request = request.header("range", "bytes=0-0");
    }
    let response = request.send().await.map_err(request_failure)?;
    check_headers(&response)?;
    Ok(response)
}

fn check_headers(response: &Response) -> Result<(), Failure> {
    let size: usize = response
        .headers()
        .iter()
        .map(|(name, value)| name.as_str().len() + value.len() + 4)
        .sum();
    if size <= MAX_HEADERS {
        Ok(())
    } else {
        Err(Failure::Protocol)
    }
}

fn request_failure(error: reqwest::Error) -> Failure {
    if error.is_timeout() {
        Failure::Timeout
    } else if error.is_connect() {
        Failure::Connection
    } else {
        Failure::Protocol
    }
}

fn parse_url(value: &str) -> Result<Url, Failure> {
    if !crate::input::valid_target(value) {
        return Err(Failure::Blocked);
    }
    validate_url(Url::parse(value).map_err(|_| Failure::Blocked)?)
}

fn validate_url(mut url: Url) -> Result<Url, Failure> {
    let expected_port = match url.scheme() {
        "http" => 80,
        "https" => 443,
        _ => return Err(Failure::Blocked),
    };
    if url.as_str().len() > MAX_URL
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(expected_port)
        || url.host().is_none()
    {
        return Err(Failure::Blocked);
    }
    url.set_fragment(None);
    Ok(url)
}

struct DenyUnpinned;

impl Resolve for DenyUnpinned {
    fn resolve(&self, _: Name) -> Resolving {
        Box::pin(async { Err(io::Error::other("unpinned resolver request").into()) })
    }
}

async fn pinned_client(url: &Url) -> Result<Client, Failure> {
    let port = url.port_or_known_default().ok_or(Failure::Blocked)?;
    let mut builder = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .http1_only()
        .http1_max_headers(64)
        .pool_max_idle_per_host(0)
        .dns_resolver(Arc::new(DenyUnpinned))
        .connect_timeout(Duration::from_secs(2))
        .read_timeout(Duration::from_secs(3))
        .user_agent("ShortCircuit link monitor")
        .default_headers(
            [(ACCEPT_ENCODING, "identity".parse().expect("valid encoding"))]
                .into_iter()
                .collect(),
        );
    match url.host().ok_or(Failure::Blocked)? {
        Host::Ipv4(ip) if public_ipv4(ip) => {}
        Host::Ipv6(ip) if public_ipv6(ip) => {}
        Host::Domain(name) => {
            let canonical = name.trim_end_matches('.');
            if !canonical.contains('.')
                || canonical.ends_with(".localhost")
                || canonical.ends_with(".local")
            {
                return Err(Failure::Blocked);
            }
            let absolute = format!("{canonical}.");
            let candidates = lookup_host((absolute.as_str(), port))
                .await
                .map_err(|_| Failure::Dns)?;
            let addresses = approved_addresses(candidates)?;
            builder = builder.resolve_to_addrs(name, &addresses);
        }
        _ => return Err(Failure::Blocked),
    }
    builder.build().map_err(|_| Failure::Protocol)
}

fn approved_addresses(
    values: impl Iterator<Item = SocketAddr>,
) -> Result<Vec<SocketAddr>, Failure> {
    let mut addresses = Vec::new();
    for address in values {
        let public = match address.ip() {
            IpAddr::V4(ip) => public_ipv4(ip),
            IpAddr::V6(ip) => public_ipv6(ip),
        };
        if !public || address.port() == 0 {
            return Err(Failure::Blocked);
        }
        if !addresses.contains(&address) {
            if addresses.len() == MAX_ADDRESSES {
                return Err(Failure::Dns);
            }
            addresses.push(address);
        }
    }
    if addresses.is_empty() {
        return Err(Failure::Dns);
    }
    Ok(addresses)
}

fn public_ipv4(ip: Ipv4Addr) -> bool {
    !matches!(
        ip.octets(),
        [0 | 10 | 127 | 224..=255, ..]
            | [100, 64..=127, ..]
            | [169, 254, ..]
            | [172, 16..=31, ..]
            | [192, 0, 0 | 2, _]
            | [192, 31, 196, _]
            | [192, 52, 193, _]
            | [192, 88, 99, _]
            | [192, 168, ..]
            | [192, 175, 48, _]
            | [198, 18 | 19, ..]
            | [198, 51, 100, _]
            | [203, 0, 113, _]
    )
}

fn public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return public_ipv4(mapped);
    }
    let [first, second, third, ..] = ip.segments();
    (0x2000..=0x3fff).contains(&first)
        && !matches!(
            (first, second, third),
            (0x2001, 0..=0x01ff | 0x0db8, _)
                | (0x2002, _, _)
                | (0x3fff, 0..=0x0fff, _)
                | (0x2620, 0x004f, 0x8000)
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_mixed_answers_and_nonpublic_literal_addresses() {
        let approved: SocketAddr = "93.184.216.34:443".parse().unwrap();
        let refused: SocketAddr = "127.0.0.1:443".parse().unwrap();
        assert_eq!(
            approved_addresses([approved, refused].into_iter()),
            Err(Failure::Blocked)
        );
        assert_eq!(
            approved_addresses([approved, approved].into_iter()).unwrap(),
            [approved]
        );
        for input in [
            "::1",
            "::ffff:127.0.0.1",
            "fe80::1",
            "fc00::1",
            "2001:db8::1",
            "3fff::f",
        ] {
            assert!(!public_ipv6(input.parse().unwrap()), "{input}");
        }
        assert!(public_ipv6("2606:4700::1111".parse().unwrap()));
    }

    #[test]
    fn validates_the_outbound_scheme_ports_and_credentials() {
        for target in [
            "file:///tmp/value",
            "https://user:password@example.test/",
            "https://example.test:8443/",
            "http://example.test:443/",
        ] {
            assert_eq!(parse_url(target), Err(Failure::Blocked), "{target}");
        }
        assert_eq!(
            parse_url("https://example.test/?a=1#part")
                .unwrap()
                .as_str(),
            "https://example.test/?a=1"
        );
    }
}
