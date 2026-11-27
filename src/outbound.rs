use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use reqwest::{
    Certificate, Client, ClientBuilder, Method, Response, StatusCode,
    dns::{Name, Resolve, Resolving},
    header::{
        ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE, HeaderValue,
        LOCATION,
    },
};
use tokio::{net::lookup_host, sync::Semaphore, time::timeout};
use url::{Host, Url};

use crate::rate_limit::Network;

const DEADLINE: Duration = Duration::from_secs(8);
const MAX_HOPS: u8 = 3;
const MAX_ADDRESSES: usize = 16;
const MAX_URL: usize = 4096;
const MAX_HEADERS: usize = 16 * 1024;
/// Response bytes read when looking for a page title.
pub(crate) const MAX_TITLE_BYTES: usize = 128 * 1024;
const MAX_TITLE_CHARS: usize = 160;
const MAX_CONNECTOR_NETWORKS: usize = 16;

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
    /// The response was not an HTML document.
    NotHtml,
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
        let (response, redirects) = timeout(DEADLINE, follow(url, Mode::Probe))
            .await
            .map_err(|_| Failure::Timeout)??;
        Ok(Probe {
            status: response.status().as_u16(),
            redirects,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// Fetches the start of an HTML page and extracts its title as plain text.
    /// Each redirect hop is revalidated, and at most `MAX_TITLE_BYTES` are read.
    pub(crate) async fn fetch_title(&self, target: &str) -> Result<Option<String>, Failure> {
        let _permit = self.permits.try_acquire().map_err(|_| Failure::Busy)?;
        let url = parse_url(target)?;
        timeout(DEADLINE, async {
            let (mut response, _) = follow(url, Mode::Page).await?;
            if !response.status().is_success() {
                return Err(Failure::Protocol);
            }
            if response
                .headers()
                .get(CONTENT_ENCODING)
                .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
            {
                return Err(Failure::Protocol);
            }
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let media = content_type.split(';').next().unwrap_or_default().trim();
            if !matches!(media, "text/html" | "application/xhtml+xml") {
                return Err(Failure::NotHtml);
            }
            let latin1 = content_type
                .split(';')
                .filter_map(|parameter| parameter.trim().strip_prefix("charset="))
                .any(|charset| {
                    matches!(
                        charset.trim_matches('"'),
                        "iso-8859-1" | "latin1" | "windows-1252"
                    )
                });
            let mut body = Vec::new();
            while body.len() < MAX_TITLE_BYTES {
                let Some(chunk) = response.chunk().await.map_err(request_failure)? else {
                    break;
                };
                let room = MAX_TITLE_BYTES - body.len();
                body.extend_from_slice(&chunk[..chunk.len().min(room)]);
                if find_ignore_case(&body, b"</title").is_some() {
                    break;
                }
            }
            Ok(extract_title(&body, latin1))
        })
        .await
        .map_err(|_| Failure::Timeout)?
    }

    /// Posts JSON to a configured webhook. Redirects are never followed.
    pub(crate) async fn deliver(&self, webhook: &Webhook, json: Vec<u8>) -> Result<(), Failure> {
        let _permit = self.permits.try_acquire().map_err(|_| Failure::Busy)?;
        timeout(DEADLINE, async {
            let client = match &webhook.connector {
                Some(connector) => connector_client(&webhook.url, connector).await?,
                None => pinned_client(&webhook.url).await?,
            };
            let mut request = client
                .post(webhook.url.clone())
                .header(CONTENT_TYPE, "application/json")
                .body(json);
            if let Some(authorization) = &webhook.authorization {
                request = request.header(AUTHORIZATION, authorization.clone());
            }
            let response = request.send().await.map_err(request_failure)?;
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// HEAD with a one-byte GET fallback for health checks.
    Probe,
    /// A GET request for an HTML page.
    Page,
}

async fn follow(mut url: Url, mode: Mode) -> Result<(Response, u8), Failure> {
    let mut method = match mode {
        Mode::Probe => Method::HEAD,
        Mode::Page => Method::GET,
    };
    for hops in 0..=MAX_HOPS {
        let client = pinned_client(&url).await?;
        let mut response = send(&client, method.clone(), url.clone(), mode).await?;
        if method == Method::HEAD
            && matches!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
            )
        {
            method = Method::GET;
            drop(response);
            response = send(&client, method.clone(), url.clone(), mode).await?;
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

async fn send(client: &Client, method: Method, url: Url, mode: Mode) -> Result<Response, Failure> {
    let mut request = client.request(method.clone(), url);
    match mode {
        Mode::Probe if method == Method::GET => request = request.header("range", "bytes=0-0"),
        Mode::Probe => {}
        Mode::Page => request = request.header(ACCEPT, "text/html, application/xhtml+xml"),
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

fn base_builder() -> ClientBuilder {
    Client::builder()
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
        .user_agent("ShortCircuit")
        .default_headers(
            [(ACCEPT_ENCODING, "identity".parse().expect("valid encoding"))]
                .into_iter()
                .collect(),
        )
}

/// A client restricted to validated public addresses on port 80 or 443.
async fn pinned_client(url: &Url) -> Result<Client, Failure> {
    let port = url.port_or_known_default().ok_or(Failure::Blocked)?;
    let mut builder = base_builder();
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
            let addresses = approved_addresses(candidates, |ip| match ip {
                IpAddr::V4(ip) => public_ipv4(ip),
                IpAddr::V6(ip) => public_ipv6(ip),
            })?;
            builder = builder.resolve_to_addrs(name, &addresses);
        }
        _ => return Err(Failure::Blocked),
    }
    builder.build().map_err(|_| Failure::Protocol)
}

/// A client restricted to the connector's pinned origin and address ranges.
async fn connector_client(url: &Url, connector: &Connector) -> Result<Client, Failure> {
    if !connector.matches(url) {
        return Err(Failure::Blocked);
    }
    let mut builder = base_builder();
    for root in &connector.roots {
        builder = builder.add_root_certificate(root.clone());
    }
    match url.host().ok_or(Failure::Blocked)? {
        Host::Ipv4(ip) if connector.permits(IpAddr::V4(ip)) => {}
        Host::Ipv6(ip) if connector.permits(IpAddr::V6(ip)) => {}
        Host::Domain(name) => {
            let absolute = format!("{}.", name.trim_end_matches('.'));
            let candidates = lookup_host((absolute.as_str(), connector.port))
                .await
                .map_err(|_| Failure::Dns)?;
            let addresses = approved_addresses(candidates, |ip| connector.permits(ip))?;
            builder = builder.resolve_to_addrs(name, &addresses);
        }
        _ => return Err(Failure::Blocked),
    }
    builder.build().map_err(|_| Failure::Protocol)
}

/// Accepts a DNS answer only if every address passes `allowed`.
fn approved_addresses(
    values: impl Iterator<Item = SocketAddr>,
    allowed: impl Fn(IpAddr) -> bool,
) -> Result<Vec<SocketAddr>, Failure> {
    let mut addresses = Vec::new();
    for address in values {
        if !allowed(address.ip()) || address.port() == 0 {
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

/// A webhook destination. Without a connector it follows the public address policy.
pub(crate) struct Webhook {
    url: Url,
    authorization: Option<HeaderValue>,
    connector: Option<Connector>,
}

impl std::fmt::Debug for Webhook {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Webhook")
            .field("origin", &self.origin())
            .field("connector", &self.connector.is_some())
            .finish_non_exhaustive()
    }
}

impl Webhook {
    pub(crate) fn new(
        url: &str,
        authorization: Option<&str>,
        connector: Option<Connector>,
    ) -> Result<Self, &'static str> {
        let invalid = "the notification webhook requires a valid HTTPS URL";
        if !crate::input::valid_target(url) {
            return Err(invalid);
        }
        let mut url = Url::parse(url).map_err(|_| invalid)?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            return Err(invalid);
        }
        url.set_fragment(None);
        match &connector {
            Some(connector) if !connector.matches(&url) => {
                return Err("the notification webhook must use the connector origin");
            }
            Some(_) => {}
            None => {
                url = validate_url(url).map_err(|_| {
                    "the notification webhook must use port 443 unless a connector is configured"
                })?;
            }
        }
        let authorization = authorization
            .map(|value| {
                let mut header = HeaderValue::from_str(value)
                    .map_err(|_| "the webhook authorization contains invalid characters")?;
                header.set_sensitive(true);
                Ok::<_, &'static str>(header)
            })
            .transpose()?;
        Ok(Self {
            url,
            authorization,
            connector,
        })
    }

    /// The scheme, host, and port only, for display.
    pub(crate) fn origin(&self) -> String {
        self.url.origin().ascii_serialization()
    }

    pub(crate) fn uses_connector(&self) -> bool {
        self.connector.is_some()
    }

    /// Whether a webhook without a connector names a literal address that delivery would refuse.
    /// Names are resolved and checked when each delivery connects.
    pub(crate) fn targets_private_literal(&self) -> bool {
        self.connector.is_none()
            && match self.url.host() {
                Some(Host::Ipv4(ip)) => !public_ipv4(ip),
                Some(Host::Ipv6(ip)) => !public_ipv6(ip),
                Some(Host::Domain(name)) => {
                    let name = name.trim_end_matches('.');
                    !name.contains('.') || name.ends_with(".localhost") || name.ends_with(".local")
                }
                None => true,
            }
    }
}

/// Whether a value can be sent as an `Authorization` header.
pub(crate) fn valid_authorization(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && HeaderValue::from_str(value).is_ok()
}

/// One explicitly trusted internal HTTPS origin and the address ranges it may resolve to.
/// The connector applies only to webhook delivery. Probes and title lookups never use it.
#[derive(Clone)]
pub(crate) struct Connector {
    host: String,
    port: u16,
    networks: Vec<Network>,
    roots: Vec<Certificate>,
}

impl Connector {
    pub(crate) fn new(
        origin: &str,
        networks: &str,
        certificates: Option<&[u8]>,
    ) -> Result<Self, &'static str> {
        let invalid_origin = "NOTIFICATION_CONNECTOR_ORIGIN must be a bare HTTPS origin";
        let url = Url::parse(origin).map_err(|_| invalid_origin)?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_origin);
        }
        let host = url.host_str().ok_or(invalid_origin)?.to_owned();
        let port = url.port_or_known_default().ok_or(invalid_origin)?;
        let invalid_networks =
            "NOTIFICATION_CONNECTOR_NETWORKS must list 1 to 16 private CIDR networks";
        let networks = networks
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| Network::parse(value, invalid_networks))
            .collect::<Result<Vec<_>, _>>()?;
        if networks.is_empty() || networks.len() > MAX_CONNECTOR_NETWORKS {
            return Err(invalid_networks);
        }
        for network in &networks {
            let (base, prefix) = network.base();
            let broad = match base {
                IpAddr::V4(_) => prefix < 8,
                IpAddr::V6(_) => prefix < 16,
            };
            if broad || never_connector(base) {
                return Err(invalid_networks);
            }
        }
        let invalid_roots = "NOTIFICATION_CONNECTOR_CA_FILE must contain PEM certificates";
        let roots = match certificates {
            Some(pem) => {
                let roots = Certificate::from_pem_bundle(pem).map_err(|_| invalid_roots)?;
                if roots.is_empty() {
                    return Err(invalid_roots);
                }
                roots
            }
            None => Vec::new(),
        };
        let connector = Self {
            host,
            port,
            networks,
            roots,
        };
        // A literal address must already fall inside the pinned ranges.
        let literal = match url.host() {
            Some(Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
            _ => None,
        };
        if literal.is_some_and(|ip| !connector.permits(ip)) {
            return Err(
                "NOTIFICATION_CONNECTOR_ORIGIN must resolve inside NOTIFICATION_CONNECTOR_NETWORKS",
            );
        }
        Ok(connector)
    }

    fn matches(&self, url: &Url) -> bool {
        url.scheme() == "https"
            && url.host_str() == Some(self.host.as_str())
            && url.port_or_known_default() == Some(self.port)
    }

    fn permits(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        !never_connector(ip) && self.networks.iter().any(|network| network.contains(ip))
    }
}

/// Loopback, link-local, unspecified, and multicast addresses are never connector targets.
fn never_connector(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_multicast()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn find_ignore_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

/// Extracts the first `<title>` element as normalized plain text of at most 160 characters.
pub(crate) fn extract_title(body: &[u8], latin1: bool) -> Option<String> {
    let mut start = 0;
    let open = loop {
        let found = start + find_ignore_case(&body[start..], b"<title")?;
        let next = body.get(found + 6).copied();
        if matches!(next, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'/')) {
            break found;
        }
        start = found + 6;
    };
    let content = open + body[open..].iter().position(|byte| *byte == b'>')? + 1;
    let end = content + find_ignore_case(&body[content..], b"</title")?;
    let raw = &body[content..end];
    let text: String = if latin1 {
        raw.iter().map(|byte| char::from(*byte)).collect()
    } else {
        String::from_utf8_lossy(raw).into_owned()
    };
    let decoded = decode_entities(&text);
    let mut normalized = String::with_capacity(decoded.len().min(4 * MAX_TITLE_CHARS));
    let mut count = 0;
    for word in decoded.split_whitespace() {
        let word: String = word.chars().filter(|ch| !ch.is_control()).collect();
        if word.is_empty() {
            continue;
        }
        if !normalized.is_empty() {
            if count == MAX_TITLE_CHARS {
                break;
            }
            normalized.push(' ');
            count += 1;
        }
        for ch in word.chars() {
            if count == MAX_TITLE_CHARS {
                break;
            }
            normalized.push(ch);
            count += 1;
        }
    }
    let normalized = normalized.trim().to_owned();
    (!normalized.is_empty()).then_some(normalized)
}

fn decode_entities(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find('&') {
        output.push_str(&rest[..position]);
        rest = &rest[position..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            output.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .map(|hex| u32::from_str_radix(hex, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse::<u32>))
                .and_then(Result::ok)
                .and_then(char::from_u32)
                .filter(|ch| !ch.is_control()),
        };
        match decoded {
            Some(ch) => {
                output.push(ch);
                rest = &rest[end + 1..];
            }
            None => {
                output.push('&');
                rest = &rest[1..];
            }
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(ip) => public_ipv4(ip),
            IpAddr::V6(ip) => public_ipv6(ip),
        }
    }

    #[test]
    fn titles_are_bounded_plain_text_from_the_first_title_element() {
        for (body, expected) in [
            (
                &b"<html><head><title>Docs &amp; Guides</title>"[..],
                Some("Docs & Guides"),
            ),
            (
                b"<TITLE lang=en>\n  Line\n\tbreaks  </TITLE>",
                Some("Line breaks"),
            ),
            (
                b"<titlebar>x</titlebar><title>&lt;b&gt;Real&lt;/b&gt; &#x1F600; &#65;</title>",
                Some("<b>Real</b> \u{1F600} A"),
            ),
            (
                b"<title>Unknown &bogus; &#0; &</title>",
                Some("Unknown &bogus; &#0; &"),
            ),
            (b"<title></title>", None),
            (b"<title>   </title>", None),
            (b"<title>never closed", None),
            (b"<p>no title</p>", None),
            (
                b"<title><script>alert(1)</script></title>",
                Some("<script>alert(1)</script>"),
            ),
        ] {
            assert_eq!(extract_title(body, false).as_deref(), expected, "{body:?}");
        }
        assert_eq!(
            extract_title(b"<title>Caf\xe9</title>", true).as_deref(),
            Some("Caf\u{e9}")
        );
        assert_eq!(
            extract_title(b"<title>Caf\xe9</title>", false).as_deref(),
            Some("Caf\u{fffd}")
        );
        let long = format!("<title>{}</title>", "word ".repeat(100));
        let title = extract_title(long.as_bytes(), false).unwrap();
        assert_eq!(title.chars().count(), 160 - 1);
        assert!(!title.ends_with(' '));
    }

    #[test]
    fn connectors_pin_one_https_origin_to_private_ranges() {
        let connector =
            Connector::new("https://hooks.lan:8443", "10.20.0.0/16, fd00:1::/32", None).unwrap();
        assert!(connector.matches(&Url::parse("https://hooks.lan:8443/notify").unwrap()));
        for other in [
            "https://hooks.lan/notify",
            "https://other.lan:8443/",
            "http://hooks.lan:8443/",
        ] {
            assert!(!connector.matches(&Url::parse(other).unwrap()), "{other}");
        }
        assert!(connector.permits("10.20.3.4".parse().unwrap()));
        assert!(connector.permits("::ffff:10.20.3.4".parse().unwrap()));
        assert!(connector.permits("fd00:1::5".parse().unwrap()));
        assert!(!connector.permits("10.21.0.1".parse().unwrap()));
        assert!(!connector.permits("93.184.216.34".parse().unwrap()));
        for (origin, networks) in [
            ("http://hooks.lan", "10.0.0.0/8"),
            ("https://hooks.lan/path", "10.0.0.0/8"),
            ("https://user@hooks.lan", "10.0.0.0/8"),
            ("https://hooks.lan", ""),
            ("https://hooks.lan", "0.0.0.0/0"),
            ("https://hooks.lan", "127.0.0.0/8"),
            ("https://hooks.lan", "169.254.0.0/16"),
            ("https://hooks.lan", "fe80::/10"),
            ("https://hooks.lan", "::/8"),
            ("https://hooks.lan", "not-a-network"),
        ] {
            assert!(
                Connector::new(origin, networks, None).is_err(),
                "{origin} {networks}"
            );
        }
        assert!(Connector::new("https://hooks.lan", "10.0.0.0/8", Some(b"not pem")).is_err());

        let webhook = Webhook::new(
            "https://hooks.lan:8443/notify?topic=a#ignored",
            Some("Bearer synthetic"),
            Some(Connector::new("https://hooks.lan:8443", "10.20.0.0/16", None).unwrap()),
        )
        .unwrap();
        assert_eq!(webhook.origin(), "https://hooks.lan:8443");
        assert!(webhook.uses_connector());
        assert!(!format!("{webhook:?}").contains("synthetic"));
        assert!(
            Webhook::new(
                "https://elsewhere.example/notify",
                None,
                Some(Connector::new("https://hooks.lan", "10.0.0.0/8", None).unwrap())
            )
            .is_err()
        );
        assert!(Webhook::new("https://hooks.example:8443/notify", None, None).is_err());
        assert!(Webhook::new("http://hooks.example/notify", None, None).is_err());
        assert!(Webhook::new("https://hooks.example/notify", Some("bad\nvalue"), None).is_err());
        assert!(
            !Webhook::new("https://hooks.example/notify", None, None)
                .unwrap()
                .uses_connector()
        );
    }

    #[tokio::test]
    async fn connector_delivery_refuses_addresses_outside_the_pinned_ranges() {
        let connector = Connector::new("https://127.0.0.2", "10.0.0.0/8", None);
        assert!(connector.is_err());
        assert!(Connector::new("https://10.255.255.1", "10.0.0.0/24", None).is_err());
        // A name that resolves outside the pinned ranges is refused before connecting.
        let webhook = Webhook::new(
            "https://localhost/notify",
            None,
            Some(Connector::new("https://localhost", "10.0.0.0/24", None).unwrap()),
        )
        .unwrap();
        assert_eq!(
            Fetcher::new().deliver(&webhook, b"{}".to_vec()).await,
            Err(Failure::Blocked)
        );
        // A public webhook cannot reach a private literal address.
        let public = Webhook::new("https://10.0.0.1/notify", None, None).unwrap();
        assert_eq!(
            Fetcher::new().deliver(&public, b"{}".to_vec()).await,
            Err(Failure::Blocked)
        );
    }

    #[test]
    fn blocks_mixed_answers_and_nonpublic_literal_addresses() {
        let approved: SocketAddr = "93.184.216.34:443".parse().unwrap();
        let refused: SocketAddr = "127.0.0.1:443".parse().unwrap();
        assert_eq!(
            approved_addresses([approved, refused].into_iter(), public),
            Err(Failure::Blocked)
        );
        assert_eq!(
            approved_addresses([approved, approved].into_iter(), public).unwrap(),
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
