use std::{
    net::{IpAddr, Ipv6Addr},
    sync::Arc,
};

use axum::http::{HeaderMap, Uri, header::HOST, uri::Authority};
use url::Url;

#[derive(Clone)]
pub struct PublicHosts(Arc<[String]>);

impl PublicHosts {
    pub fn parse(values: &str) -> Result<Self, &'static str> {
        let mut hosts = Vec::new();
        for value in values.split(',') {
            let host = normalize_host(value.trim()).ok_or("invalid PUBLIC_HOSTS entry")?;
            if !hosts.contains(&host) {
                hosts.push(host);
            }
            if hosts.len() > 32 {
                return Err("too many PUBLIC_HOSTS entries");
            }
        }
        Ok(Self(hosts.into()))
    }

    pub(crate) fn primary(&self) -> &str {
        &self.0[0]
    }

    pub(crate) fn all(&self) -> &[String] {
        &self.0
    }

    pub(crate) fn contains(&self, host: &str) -> bool {
        self.0.iter().any(|configured| configured == host)
    }
}

pub(crate) fn normalize_host(value: &str) -> Option<String> {
    if let Some(address) = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        let address: Ipv6Addr = address.parse().ok()?;
        return Some(format!("[{address}]"));
    }

    if let Ok(IpAddr::V4(address)) = value.parse() {
        return Some(address.to_string());
    }

    let name = value.strip_suffix('.').unwrap_or(value);
    let valid = name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        });
    valid.then(|| name.to_ascii_lowercase())
}

fn request_authority(value: &str) -> Option<(String, Option<u16>)> {
    if value.contains('@') {
        return None;
    }
    let authority: Authority = value.parse().ok()?;
    let host = authority.host();
    let suffix = authority.as_str().strip_prefix(host)?;
    let port = if suffix.is_empty() {
        None
    } else {
        Some(authority.port_u16()?)
    };
    Some((normalize_host(host)?, port))
}

pub(crate) fn request_host(uri: &Uri, headers: &HeaderMap) -> Option<String> {
    let mut values = headers.get_all(HOST).iter();
    let header = values.next();
    if values.next().is_some() {
        return None;
    }
    let header = match header {
        Some(header) => Some(request_authority(header.to_str().ok()?)?),
        None => None,
    };
    if let Some(authority) = uri.authority() {
        let uri_host = request_authority(authority.as_str())?;
        if header.is_some_and(|header| header != uri_host) {
            return None;
        }
        return Some(uri_host.0);
    }
    header.map(|(host, _)| host)
}

pub(crate) fn valid_slug(slug: &str) -> bool {
    slug.len() <= 200
        && slug.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

pub(crate) fn valid_target(value: &str) -> bool {
    if value.len() > 4096
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&byte))
    {
        return false;
    }
    let Some((_, after_scheme)) = value.split_once("://") else {
        return false;
    };
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }

    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%'
            && !(bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit())
                && bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit()))
        {
            return false;
        }
    }

    Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.has_host()
            && url.username().is_empty()
            && url.password().is_none()
    })
}
