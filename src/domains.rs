use std::{collections::BTreeMap, net::IpAddr};

use axum::http::uri::Authority;
use serde::Serialize;
use sqlx::{Executor, Sqlite, SqlitePool};
use url::Url;

use crate::input::{PublicHosts, normalize_host, valid_target};

const MAX_MANAGED_DOMAINS: i64 = 256;

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct Domain {
    pub(crate) host: String,
    pub(crate) origin: String,
    pub(crate) enabled: bool,
    pub(crate) root_target: Option<String>,
    pub(crate) missing_target: Option<String>,
    pub(crate) block_crawlers: bool,
}

pub(crate) async fn list(
    pool: &SqlitePool,
    hosts: &PublicHosts,
    local_port: u16,
) -> Result<Vec<Domain>, sqlx::Error> {
    let mut managed: BTreeMap<_, _> = sqlx::query_as::<_, Domain>(
        "SELECT host, origin, enabled, root_target, missing_target, block_crawlers FROM domains",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|domain| (domain.host.clone(), domain))
    .collect();
    let mut domains: Vec<_> = hosts
        .all()
        .iter()
        .map(|host| {
            managed
                .remove(host)
                .unwrap_or_else(|| default_domain(host, local_port))
        })
        .collect();
    domains.extend(managed.into_values());
    Ok(domains)
}

pub(crate) async fn active_hosts<'e>(
    executor: impl Executor<'e, Database = Sqlite>,
    hosts: &PublicHosts,
) -> Result<Vec<String>, sqlx::Error> {
    let mut managed: BTreeMap<_, _> =
        sqlx::query_as::<_, (String, bool)>("SELECT host, enabled FROM domains")
            .fetch_all(executor)
            .await?
            .into_iter()
            .collect();
    let mut active: Vec<_> = hosts
        .all()
        .iter()
        .filter(|host| managed.remove(*host).unwrap_or(true))
        .cloned()
        .collect();
    active.extend(
        managed
            .into_iter()
            .filter(|(_, enabled)| *enabled)
            .map(|(host, _)| host),
    );
    Ok(active)
}

pub(crate) async fn lookup(
    pool: &SqlitePool,
    hosts: &PublicHosts,
    host: &str,
    local_port: u16,
) -> Result<Option<Domain>, sqlx::Error> {
    let domain = sqlx::query_as::<_, Domain>(
        "SELECT host, origin, enabled, root_target, missing_target, block_crawlers
         FROM domains WHERE host = ?",
    )
    .bind(host)
    .fetch_optional(pool)
    .await?;
    Ok(domain.or_else(|| {
        hosts
            .contains(host)
            .then(|| default_domain(host, local_port))
    }))
}

pub(crate) async fn save(
    pool: &SqlitePool,
    domain: &Domain,
    block_crawlers: Option<bool>,
) -> Result<Option<Domain>, sqlx::Error> {
    let rows = sqlx::query_as::<_, Domain>(
        "INSERT INTO domains (host, origin, enabled, root_target, missing_target, block_crawlers)
         SELECT ?, ?, ?, ?, ?, coalesce(?, 0)
         WHERE EXISTS (SELECT 1 FROM domains WHERE host = ?)
            OR (SELECT COUNT(*) FROM domains) < ?
         ON CONFLICT (host) DO UPDATE SET
            origin = excluded.origin,
            enabled = excluded.enabled,
            root_target = excluded.root_target,
            missing_target = excluded.missing_target,
            block_crawlers = coalesce(?, domains.block_crawlers)
         RETURNING host, origin, enabled, root_target, missing_target, block_crawlers",
    )
    .bind(&domain.host)
    .bind(&domain.origin)
    .bind(domain.enabled)
    .bind(&domain.root_target)
    .bind(&domain.missing_target)
    .bind(block_crawlers)
    .bind(&domain.host)
    .bind(MAX_MANAGED_DOMAINS)
    .bind(block_crawlers)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().next())
}

pub(crate) fn canonical_origin(host: &str, origin: &str) -> Option<String> {
    if !valid_target(origin) {
        return None;
    }
    let (_, source) = origin.split_once("://")?;
    let authority: Authority = source.strip_suffix('/').unwrap_or(source).parse().ok()?;
    if normalize_host(authority.host())?.as_str() != host {
        return None;
    }
    let url = Url::parse(origin).ok()?;
    if url.host_str()? != host
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

pub(crate) fn fallback_loops(host: &str, path: &str, target: &str) -> bool {
    Url::parse(target).is_ok_and(|url| {
        url.path() == path && url.host_str().and_then(normalize_host).as_deref() == Some(host)
    })
}

fn default_domain(host: &str, local_port: u16) -> Domain {
    let loopback = host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    let origin = if loopback && local_port == 80 {
        format!("http://{host}")
    } else if loopback {
        format!("http://{host}:{local_port}")
    } else {
        format!("https://{host}")
    };
    Domain {
        host: host.to_owned(),
        origin,
        enabled: true,
        root_target: None,
        missing_target: None,
        block_crawlers: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_must_be_bare_and_match_the_domain_host() {
        assert_eq!(
            canonical_origin("go.example", "https://GO.example:443/").as_deref(),
            Some("https://go.example")
        );
        for origin in [
            "https://other.example",
            "https://@go.example",
            "https://go.example/.",
            "https://go.example/path",
            "https://go.example?",
            "https://go.example#",
            "https://g%6f.example",
            "ftp://go.example",
        ] {
            assert_eq!(canonical_origin("go.example", origin), None, "{origin}");
        }
    }

    #[test]
    fn fallback_cannot_redirect_to_the_same_host_and_path() {
        assert!(fallback_loops(
            "go.example",
            "/missing",
            "https://GO.example.:8443/other/../missing?x=1#here"
        ));
        assert!(!fallback_loops(
            "go.example",
            "/missing",
            "https://go.example/landing"
        ));
        assert!(!fallback_loops(
            "go.example",
            "/missing",
            "https://other.example/missing"
        ));
    }

    #[tokio::test]
    async fn managed_domains_override_environment_hosts_without_removing_slugs() {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("domains.sqlite"))
            .await
            .unwrap();
        let hosts = PublicHosts::parse("go.example,localhost,[::1]").unwrap();
        sqlx::query(
            "INSERT INTO links (host, slug, target)
             VALUES ('go.example', 'Docs', 'https://example.test')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let initial = list(&pool, &hosts, 8180).await.unwrap();
        assert_eq!(initial[0].origin, "https://go.example");
        assert_eq!(initial[1].origin, "http://localhost:8180");
        assert_eq!(initial[2].origin, "http://[::1]:8180");
        assert!(
            lookup(&pool, &hosts, "unknown.example", 8180)
                .await
                .unwrap()
                .is_none()
        );

        let mut go = Domain {
            host: "go.example".into(),
            origin: "https://go.example:8443".into(),
            enabled: false,
            root_target: None,
            missing_target: None,
            block_crawlers: false,
        };
        assert!(save(&pool, &go, None).await.unwrap().is_some());
        let saved = lookup(&pool, &hosts, "go.example", 8180)
            .await
            .unwrap()
            .unwrap();
        assert!(!saved.enabled);
        assert_eq!(saved.origin, "https://go.example:8443");
        assert_eq!(
            active_hosts(&pool, &hosts).await.unwrap(),
            ["localhost", "[::1]"]
        );

        let extra = Domain {
            host: "new.example".into(),
            origin: "https://new.example".into(),
            enabled: true,
            root_target: None,
            missing_target: Some("https://example.test/help".into()),
            block_crawlers: false,
        };
        assert!(save(&pool, &extra, None).await.unwrap().is_some());
        go.enabled = true;
        assert!(save(&pool, &go, None).await.unwrap().is_some());
        assert_eq!(
            active_hosts(&pool, &hosts).await.unwrap(),
            ["go.example", "localhost", "[::1]", "new.example"]
        );
        let stored: (String, String) = sqlx::query_as("SELECT slug, target FROM links")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            stored,
            ("Docs".to_owned(), "https://example.test".to_owned())
        );
    }
}
