use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use url::form_urlencoded;

use crate::{input::valid_target, store::push_host_scope};

const DEFAULT_STATUS: u16 = 302;
const MAX_RULES: usize = 16;
const MAX_CONDITIONS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Policy {
    pub(crate) status: u16,
    pub(crate) starts_at: Option<i64>,
    pub(crate) expires_at: Option<i64>,
    pub(crate) max_visits: Option<i64>,
    pub(crate) forward_query: bool,
    pub(crate) forward_path: bool,
    pub(crate) rules: Vec<Rule>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            status: DEFAULT_STATUS,
            starts_at: None,
            expires_at: None,
            max_visits: None,
            forward_query: false,
            forward_path: false,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    target: String,
    conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Condition {
    Language { value: String },
    Device { value: Device },
    Os { value: Os },
    Browser { value: Browser },
    Query { key: String, value: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Device {
    Desktop,
    Mobile,
    Tablet,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Os {
    Windows,
    Macos,
    Linux,
    Android,
    Ios,
    Chromeos,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Browser {
    Edge,
    Firefox,
    Chrome,
    Safari,
    Opera,
    Unknown,
}

#[derive(Serialize)]
pub(crate) struct PolicyDetails {
    #[serde(flatten)]
    pub(crate) policy: Policy,
    pub(crate) visits_used: i64,
}

#[derive(Debug)]
pub(crate) enum PolicyError {
    Invalid(&'static str),
    Database(sqlx::Error),
}

impl From<sqlx::Error> for PolicyError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RedirectRequest<'a> {
    pub(crate) host: &'a str,
    pub(crate) slug: &'a str,
    pub(crate) query: Option<&'a str>,
    pub(crate) user_agent: Option<&'a str>,
    pub(crate) language: Option<&'a str>,
    pub(crate) head: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Redirect { target: String, status: u16 },
    Challenge(Challenge),
    Missing,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Challenge {
    pub(crate) id: i64,
    pub(crate) revision: i64,
    pub(crate) password_required: bool,
}

pub(crate) struct VisitSubject {
    pub(crate) id: i64,
    pub(crate) analytics_enabled: bool,
}

pub(crate) struct Resolution {
    pub(crate) outcome: Outcome,
    pub(crate) subject: Option<VisitSubject>,
}

#[derive(sqlx::FromRow)]
struct StoredPolicy {
    status: Option<u16>,
    starts_at: Option<i64>,
    expires_at: Option<i64>,
    max_visits: Option<i64>,
    visits_used: Option<i64>,
    forward_query: Option<bool>,
    forward_path: Option<bool>,
    rules: Option<String>,
}

impl StoredPolicy {
    fn decode_rules(&self) -> Result<Vec<Rule>, sqlx::Error> {
        match self.rules.as_deref() {
            Some(rules) => {
                serde_json::from_str(rules).map_err(|error| sqlx::Error::Decode(Box::new(error)))
            }
            None => Ok(Vec::new()),
        }
    }

    fn into_details(self) -> Result<PolicyDetails, sqlx::Error> {
        let rules = self.decode_rules()?;
        Ok(PolicyDetails {
            policy: Policy {
                status: self.status.unwrap_or(DEFAULT_STATUS),
                starts_at: self.starts_at,
                expires_at: self.expires_at,
                max_visits: self.max_visits,
                forward_query: self.forward_query.unwrap_or(false),
                forward_path: self.forward_path.unwrap_or(false),
                rules,
            },
            visits_used: self.visits_used.unwrap_or_default(),
        })
    }
}

#[derive(sqlx::FromRow)]
struct SelectedLink {
    id: i64,
    slug: String,
    target: String,
    enabled: bool,
    now: i64,
    analytics_enabled: bool,
    password_required: bool,
    one_time: bool,
    consumed: bool,
    protection_revision: Option<i64>,
    #[sqlx(flatten)]
    policy: StoredPolicy,
}

impl SelectedLink {
    fn resolution(&self, outcome: Outcome) -> Resolution {
        Resolution {
            outcome,
            subject: Some(VisitSubject {
                id: self.id,
                analytics_enabled: self.analytics_enabled,
            }),
        }
    }

    fn blocked(&self) -> bool {
        !self.enabled
            || self.consumed
            || self.policy.starts_at.is_some_and(|start| self.now < start)
            || self.policy.expires_at.is_some_and(|end| self.now >= end)
            || self
                .policy
                .max_visits
                .is_some_and(|limit| self.policy.visits_used.unwrap_or_default() >= limit)
    }

    fn challenge(&self) -> Option<Challenge> {
        self.protection_revision
            .filter(|_| self.one_time || self.password_required)
            .map(|revision| Challenge {
                id: self.id,
                revision,
                password_required: self.password_required,
            })
    }

    fn redirect(&self, request: RedirectRequest<'_>) -> Result<Outcome, sqlx::Error> {
        let rules = self.policy.decode_rules()?;
        let mut target = self.target.as_str();
        if !rules.is_empty() {
            let attributes = Attributes::from_request(request);
            if let Some(rule) = rules.iter().find(|rule| {
                rule.conditions
                    .iter()
                    .all(|condition| attributes.matches(condition))
            }) {
                target = &rule.target;
            }
        }

        // A non-exact selection has already matched a complete segment and opted in.
        let suffix = request
            .slug
            .strip_prefix(self.slug.as_str())
            .and_then(|suffix| suffix.strip_prefix('/'))
            .unwrap_or_default();
        let query = request
            .query
            .filter(|query| self.policy.forward_query.unwrap_or(false) && !query.is_empty());
        Ok(Outcome::Redirect {
            target: forward(target, suffix, query),
            status: self.policy.status.unwrap_or(DEFAULT_STATUS),
        })
    }
}

const POLICY_COLUMNS: &str = "policy.status, policy.starts_at, policy.expires_at,
    policy.max_visits, policy.visits_used, policy.forward_query, policy.forward_path, policy.rules";
const POLICY_JOIN: &str = " FROM links
    LEFT JOIN link_redirect_policies AS policy ON policy.link_id = links.id";

pub(crate) async fn read_policy(
    pool: &SqlitePool,
    hosts: &[String],
    id: i64,
) -> Result<Option<PolicyDetails>, sqlx::Error> {
    let mut query = QueryBuilder::new("SELECT ");
    query
        .push(POLICY_COLUMNS)
        .push(POLICY_JOIN)
        .push(" WHERE links.id = ")
        .push_bind(id);
    push_host_scope(&mut query, hosts);
    query
        .build_query_as::<StoredPolicy>()
        .fetch_optional(pool)
        .await?
        .map(StoredPolicy::into_details)
        .transpose()
}

pub(crate) async fn replace_policy(
    pool: &SqlitePool,
    hosts: &[String],
    id: i64,
    policy: Policy,
) -> Result<Option<PolicyDetails>, PolicyError> {
    validate_policy(&policy)?;
    let rules = serde_json::to_string(&policy.rules)
        .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
    let mut query = QueryBuilder::new(
        "INSERT INTO link_redirect_policies
         (link_id, status, starts_at, expires_at, max_visits, forward_query, forward_path, rules)
         SELECT id, ",
    );
    query
        .separated(", ")
        .push_bind(i64::from(policy.status))
        .push_bind(policy.starts_at)
        .push_bind(policy.expires_at)
        .push_bind(policy.max_visits)
        .push_bind(policy.forward_query)
        .push_bind(policy.forward_path)
        .push_bind(rules);
    query.push(" FROM links WHERE id = ").push_bind(id);
    push_host_scope(&mut query, hosts);
    query.push(
        " ON CONFLICT(link_id) DO UPDATE SET
            status = excluded.status, starts_at = excluded.starts_at,
            expires_at = excluded.expires_at, max_visits = excluded.max_visits,
            forward_query = excluded.forward_query, forward_path = excluded.forward_path,
            rules = excluded.rules
          RETURNING visits_used",
    );

    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    if matches!(policy.status, 301 | 308) {
        let mut protected = QueryBuilder::new(
            "SELECT EXISTS(SELECT 1 FROM links
             JOIN link_protection AS protection ON protection.link_id = links.id
             WHERE (protection.one_time = 1 OR protection.password_hash IS NOT NULL)
             AND links.id = ",
        );
        protected.push_bind(id);
        push_host_scope(&mut protected, hosts);
        protected.push(")");
        if protected.build_query_scalar::<bool>().fetch_one(&mut *transaction).await? {
            return Err(PolicyError::Invalid("protection_requires_temporary_redirect"));
        }
    }
    let visits = query
        .build_query_scalar::<i64>()
        .fetch_all(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(visits.into_iter().next().map(|visits_used| PolicyDetails {
        policy,
        visits_used,
    }))
}

fn validate_policy(policy: &Policy) -> Result<(), PolicyError> {
    if !matches!(policy.status, 301 | 302 | 307 | 308) {
        return Err(PolicyError::Invalid("invalid_redirect_status"));
    }
    if let (Some(start), Some(end)) = (policy.starts_at, policy.expires_at)
        && start >= end
    {
        return Err(PolicyError::Invalid("invalid_redirect_window"));
    }
    if policy.max_visits.is_some_and(|limit| limit <= 0) {
        return Err(PolicyError::Invalid("invalid_visit_limit"));
    }
    if matches!(policy.status, 301 | 308)
        && (policy.starts_at.is_some()
            || policy.expires_at.is_some()
            || policy.max_visits.is_some())
    {
        return Err(PolicyError::Invalid(
            "permanent_redirect_requires_unrestricted_link",
        ));
    }
    if policy.rules.len() > MAX_RULES {
        return Err(PolicyError::Invalid("invalid_redirect_rule"));
    }
    for rule in &policy.rules {
        if !valid_target(&rule.target) {
            return Err(PolicyError::Invalid("invalid_target"));
        }
        if rule.conditions.is_empty()
            || rule.conditions.len() > MAX_CONDITIONS
            || !rule.conditions.iter().all(valid_condition)
        {
            return Err(PolicyError::Invalid("invalid_redirect_rule"));
        }
    }
    Ok(())
}

fn valid_condition(condition: &Condition) -> bool {
    match condition {
        Condition::Language { value } => valid_language_tag(value),
        Condition::Query { key, value } => {
            !key.is_empty()
                && key.len() <= 128
                && value.len() <= 512
                && !key.chars().chain(value.chars()).any(char::is_control)
        }
        _ => true,
    }
}

fn valid_language_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 63
        && tag.split('-').all(|segment| {
            !segment.is_empty()
                && segment.len() <= 8
                && segment.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

fn select_link(request: RedirectRequest<'_>) -> QueryBuilder<Sqlite> {
    let mut query = QueryBuilder::new(
        "SELECT links.id, links.slug, links.target, links.enabled, links.analytics_enabled, unixepoch() AS now,
         protection.password_hash IS NOT NULL AS password_required,
         COALESCE(protection.one_time, 0) AS one_time, COALESCE(protection.consumed, 0) AS consumed,
         protection.revision AS protection_revision, ",
    );
    query
        .push(POLICY_COLUMNS)
        .push(POLICY_JOIN)
        .push(" LEFT JOIN link_protection AS protection ON protection.link_id = links.id")
        .push(" WHERE links.host = ")
        .push_bind(request.host);
    if request.slug.contains('/') {
        query.push(" AND links.slug IN (");
        let mut slugs = query.separated(", ");
        slugs.push_bind(request.slug);
        for (end, _) in request.slug.match_indices('/') {
            slugs.push_bind(&request.slug[..end]);
        }
        query
            .push(") AND (policy.forward_path = 1 OR links.slug = ")
            .push_bind(request.slug)
            .push(") ORDER BY length(links.slug) DESC LIMIT 1");
    } else {
        query.push(" AND links.slug = ").push_bind(request.slug);
    }
    query
}

#[cfg(test)]
pub(crate) async fn resolve(
    pool: &SqlitePool,
    request: RedirectRequest<'_>,
) -> Result<Outcome, sqlx::Error> {
    resolve_with_subject(pool, request)
        .await
        .map(|result| result.outcome)
}

pub(crate) async fn resolve_with_subject(
    pool: &SqlitePool,
    request: RedirectRequest<'_>,
) -> Result<Resolution, sqlx::Error> {
    let missing = Resolution {
        outcome: Outcome::Missing,
        subject: None,
    };
    let Some(link) = select_link(request)
        .build_query_as::<SelectedLink>()
        .fetch_optional(pool)
        .await?
    else {
        return Ok(missing);
    };
    if link.blocked() {
        return Ok(link.resolution(Outcome::Blocked));
    }
    if let Some(challenge) = link.challenge() {
        return Ok(link.resolution(Outcome::Challenge(challenge)));
    }
    if request.head || link.policy.max_visits.is_none() {
        return link
            .redirect(request)
            .map(|outcome| link.resolution(outcome));
    }

    // The winner and its policy may have changed while acquiring the write lock.
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let link = select_link(request)
        .build_query_as::<SelectedLink>()
        .fetch_optional(&mut *transaction)
        .await?;
    let outcome = match link {
        None => missing,
        Some(link) if link.blocked() => link.resolution(Outcome::Blocked),
        Some(link) => {
            if let Some(challenge) = link.challenge() {
                link.resolution(Outcome::Challenge(challenge))
            } else {
                let outcome = link.redirect(request)?;
                consume_limit(&mut transaction, &link).await?;
                link.resolution(outcome)
            }
        }
    };
    transaction.commit().await?;
    Ok(outcome)
}

pub(crate) async fn confirm(
    pool: &SqlitePool,
    request: RedirectRequest<'_>,
    challenge: Challenge,
) -> Result<Resolution, sqlx::Error> {
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let resolved = select_link(request)
        .build_query_as::<SelectedLink>()
        .fetch_optional(&mut *transaction)
        .await?;
    let Some(link) = resolved.filter(|link| !link.blocked() && link.challenge() == Some(challenge)) else {
        return Ok(Resolution { outcome: Outcome::Blocked, subject: None });
    };
    let Outcome::Redirect { target, .. } = link.redirect(request)? else {
        unreachable!("an eligible link produces a redirect");
    };
    if axum::http::HeaderValue::from_str(&target).is_err() {
        return Err(sqlx::Error::Protocol("invalid protected link destination".into()));
    }
    if link.one_time {
        sqlx::query("UPDATE link_protection SET consumed = 1 WHERE link_id = ?")
            .bind(link.id)
            .execute(&mut *transaction)
            .await?;
    }
    consume_limit(&mut transaction, &link).await?;
    transaction.commit().await?;
    Ok(link.resolution(Outcome::Redirect { target, status: 303 }))
}

async fn consume_limit(connection: &mut sqlx::SqliteConnection, link: &SelectedLink) -> Result<(), sqlx::Error> {
    if link.policy.max_visits.is_some() {
        sqlx::query("UPDATE link_redirect_policies SET visits_used = visits_used + 1 WHERE link_id = ?")
            .bind(link.id)
            .execute(connection)
            .await?;
    }
    Ok(())
}

struct Attributes<'a> {
    language: Option<&'a str>,
    device: Device,
    os: Os,
    browser: Browser,
    query: &'a str,
}

impl<'a> Attributes<'a> {
    fn from_request(request: RedirectRequest<'a>) -> Self {
        let agent = request.user_agent.unwrap_or_default().to_ascii_lowercase();
        let os = if agent.contains("windows") {
            Os::Windows
        } else if agent.contains("android") {
            Os::Android
        } else if agent.contains("iphone") || agent.contains("ipad") || agent.contains("ipod") {
            Os::Ios
        } else if agent.contains("cros") {
            Os::Chromeos
        } else if agent.contains("macintosh") || agent.contains("mac os x") {
            Os::Macos
        } else if agent.contains("linux") {
            Os::Linux
        } else {
            Os::Unknown
        };
        let device = if agent.contains("ipad") || agent.contains("tablet") {
            Device::Tablet
        } else if agent.contains("mobile")
            || agent.contains("iphone")
            || agent.contains("ipod")
            || agent.contains("windows phone")
        {
            Device::Mobile
        } else if os == Os::Android {
            Device::Tablet
        } else if os == Os::Unknown {
            Device::Unknown
        } else {
            Device::Desktop
        };
        let browser = if ["edg/", "edga/", "edgios/", "edge/"]
            .iter()
            .any(|token| agent.contains(*token))
        {
            Browser::Edge
        } else if agent.contains("opr/") || agent.contains("opios/") || agent.contains("opera") {
            Browser::Opera
        } else if agent.contains("firefox/") || agent.contains("fxios/") {
            Browser::Firefox
        } else if ["chrome/", "chromium/", "crios/"]
            .iter()
            .any(|token| agent.contains(*token))
        {
            Browser::Chrome
        } else if agent.contains("safari/") {
            Browser::Safari
        } else {
            Browser::Unknown
        };
        Self {
            language: request.language.and_then(preferred_language),
            device,
            os,
            browser,
            query: request.query.unwrap_or_default(),
        }
    }

    fn matches(&self, condition: &Condition) -> bool {
        match condition {
            Condition::Language { value } => {
                let tag = self.language.unwrap_or("unknown");
                tag.eq_ignore_ascii_case(value)
                    || (tag.as_bytes().get(value.len()) == Some(&b'-')
                        && tag[..value.len()].eq_ignore_ascii_case(value))
            }
            Condition::Device { value } => self.device == *value,
            Condition::Os { value } => self.os == *value,
            Condition::Browser { value } => self.browser == *value,
            Condition::Query { key, value } => form_urlencoded::parse(self.query.as_bytes()).any(
                |(candidate_key, candidate_value)| {
                    candidate_key == key.as_str() && candidate_value == value.as_str()
                },
            ),
        }
    }
}

fn preferred_language(header: &str) -> Option<&str> {
    let mut preferred = None;
    let mut best_quality = 0.0;
    for entry in header.split(',') {
        let (tag, quality) = match entry.split_once(';') {
            Some((tag, parameter)) => {
                let Some(quality) = parameter
                    .trim()
                    .split_once('=')
                    .filter(|(key, _)| key.trim().eq_ignore_ascii_case("q"))
                    .and_then(|(_, value)| value.trim().parse::<f32>().ok())
                else {
                    continue;
                };
                (tag.trim(), quality)
            }
            None => (entry.trim(), 1.0),
        };
        if quality > best_quality && quality <= 1.0 && valid_language_tag(tag) {
            preferred = Some(tag);
            best_quality = quality;
        }
    }
    preferred
}

fn forward(target: &str, suffix: &str, query: Option<&str>) -> String {
    if suffix.is_empty() && query.is_none() {
        return target.to_owned();
    }
    let mut location =
        String::with_capacity(target.len() + suffix.len() + query.map_or(0, str::len) + 2);
    let (base, fragment) = target
        .split_once('#')
        .map_or((target, None), |(base, fragment)| (base, Some(fragment)));
    let (path, existing_query) = base
        .split_once('?')
        .map_or((base, None), |(path, query)| (path, Some(query)));

    location.push_str(path);
    if !suffix.is_empty() {
        if !path.ends_with('/') {
            location.push('/');
        }
        location.push_str(suffix);
    }
    if let Some(existing) = existing_query {
        location.push('?');
        location.push_str(existing);
    }
    if let Some(query) = query {
        match existing_query {
            None => location.push('?'),
            Some(existing) if !existing.is_empty() && !existing.ends_with('&') => {
                location.push('&')
            }
            _ => {}
        }
        // Preserve ordinary URI bytes, including existing escapes and parameter order.
        const HEX: &[u8] = b"0123456789ABCDEF";
        for byte in query.bytes() {
            if (b'!'..=b'~').contains(&byte) && byte != b'#' {
                location.push(char::from(byte));
            } else {
                location.push('%');
                location.push(char::from(HEX[usize::from(byte >> 4)]));
                location.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    if let Some(fragment) = fragment {
        location.push('#');
        location.push_str(fragment);
    }
    location
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::open_database;
    use sqlx::sqlite::SqlitePoolOptions;

    const HOST: &str = "go.example";
    const TARGET: &str = "https://target.example/base/?Signature=XL%2f&key=A+B&key=C#original";

    async fn database() -> (tempfile::TempDir, SqlitePool) {
        let directory = tempfile::tempdir().unwrap();
        let pool = open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        (directory, pool)
    }

    async fn insert(pool: &SqlitePool, slug: &str, enabled: bool) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO links (host, slug, target, enabled) VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(HOST)
        .bind(slug)
        .bind(TARGET)
        .bind(enabled)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn configure(pool: &SqlitePool, id: i64, policy: Policy) {
        replace_policy(pool, &[HOST.to_owned()], id, policy)
            .await
            .unwrap()
            .unwrap();
    }

    fn request(slug: &str) -> RedirectRequest<'_> {
        RedirectRequest {
            host: HOST,
            slug,
            query: None,
            user_agent: None,
            language: None,
            head: false,
        }
    }

    #[tokio::test]
    async fn defaults_preserve_the_legacy_target_without_forwarding() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for migration in [
            include_str!("../migrations/0001_links.sql"),
            include_str!("../migrations/0003_link_metadata.sql"),
        ] {
            sqlx::raw_sql(migration).execute(&pool).await.unwrap();
        }
        insert(&pool, "Team/Docs", true).await;
        for migration in [
            include_str!("../migrations/0004_redirect_policies.sql"),
            include_str!("../migrations/0006_analytics.sql"),
        ] {
            sqlx::raw_sql(migration).execute(&pool).await.unwrap();
        }

        let mut req = request("Team/Docs");
        req.query = Some("target=https://other.example");
        assert_eq!(
            resolve(&pool, req).await.unwrap(),
            Outcome::Redirect {
                target: TARGET.to_owned(),
                status: 302,
            }
        );
        assert_eq!(
            resolve(&pool, request("Team/Docs/extra")).await.unwrap(),
            Outcome::Missing
        );
    }

    #[tokio::test]
    async fn longest_prefix_preserves_existing_url_bytes_and_only_appends_the_suffix() {
        let (_directory, pool) = database().await;
        let short = insert(&pool, "Team", true).await;
        configure(
            &pool,
            short,
            Policy {
                forward_path: true,
                ..Policy::default()
            },
        )
        .await;
        let long = insert(&pool, "Team/Docs", true).await;
        configure(
            &pool,
            long,
            Policy {
                status: 307,
                forward_path: true,
                forward_query: true,
                ..Policy::default()
            },
        )
        .await;
        let mut req = request("Team/Docs/Extra/Child");
        req.query = Some("utm=ONE%2fA+B&tag=x&tag=y");
        assert_eq!(
            resolve(&pool, req).await.unwrap(),
            Outcome::Redirect {
                target: concat!(
                    "https://target.example/base/Extra/Child?Signature=XL%2f&key=A+B&key=C",
                    "&utm=ONE%2fA+B&tag=x&tag=y#original"
                )
                .to_owned(),
                status: 307,
            }
        );
        assert_eq!(
            resolve(&pool, request("Team/DocsOther")).await.unwrap(),
            Outcome::Redirect {
                target:
                    "https://target.example/base/DocsOther?Signature=XL%2f&key=A+B&key=C#original"
                        .to_owned(),
                status: 302,
            }
        );
    }

    #[tokio::test]
    async fn blocked_exact_matches_do_not_fall_back_to_prefixes() {
        let (_directory, pool) = database().await;
        let prefix = insert(&pool, "Team", true).await;
        configure(
            &pool,
            prefix,
            Policy {
                forward_path: true,
                ..Policy::default()
            },
        )
        .await;
        insert(&pool, "Team/Disabled", false).await;
        let expired = insert(&pool, "Team/Expired", true).await;
        configure(
            &pool,
            expired,
            Policy {
                expires_at: Some(1),
                ..Policy::default()
            },
        )
        .await;
        let future = insert(&pool, "Team/Future", true).await;
        configure(
            &pool,
            future,
            Policy {
                starts_at: Some(i64::MAX),
                ..Policy::default()
            },
        )
        .await;
        for slug in ["Team/Disabled", "Team/Expired", "Team/Future"] {
            assert_eq!(
                resolve(&pool, request(slug)).await.unwrap(),
                Outcome::Blocked
            );
        }
    }

    #[tokio::test]
    async fn the_first_rule_with_all_conditions_matching_selects_the_destination() {
        let (_directory, pool) = database().await;
        let link = insert(&pool, "choose", true).await;
        let rules = serde_json::from_str(
            r#"[
                {"target":"https://first.example","conditions":[
                    {"type":"browser","value":"safari"},
                    {"type":"os","value":"android"}
                ]},
                {"target":"https://second.example","conditions":[
                    {"type":"language","value":"de"},
                    {"type":"device","value":"mobile"},
                    {"type":"os","value":"android"},
                    {"type":"browser","value":"chrome"},
                    {"type":"query","key":"source","value":"our docs"}
                ]},
                {"target":"https://third.example","conditions":[
                    {"type":"device","value":"mobile"}
                ]}
            ]"#,
        )
        .unwrap();
        configure(
            &pool,
            link,
            Policy {
                status: 308,
                rules,
                ..Policy::default()
            },
        )
        .await;
        let mut req = request("choose");
        req.user_agent = Some(concat!(
            "Mozilla/5.0 (Linux; Android 10; Pixel) AppleWebKit/537.36 (KHTML, like Gecko) ",
            "Chrome/133.0 Mobile Safari/537.36"
        ));
        req.language = Some("en-US;q=0, de-DE;q=0.9");
        req.query = Some("source=other&s%6furce=our+docs");
        assert_eq!(
            resolve(&pool, req).await.unwrap(),
            Outcome::Redirect {
                target: "https://second.example".to_owned(),
                status: 308,
            }
        );
        assert_eq!(
            resolve(&pool, request("choose")).await.unwrap(),
            Outcome::Redirect {
                target: TARGET.to_owned(),
                status: 308,
            }
        );
    }

    #[tokio::test]
    async fn concurrent_gets_share_the_visit_limit_and_head_does_not_consume_it() {
        let (directory, pool) = database().await;
        let link = insert(&pool, "limited", true).await;
        configure(
            &pool,
            link,
            Policy {
                max_visits: Some(1),
                ..Policy::default()
            },
        )
        .await;

        let mut head = request("limited");
        head.head = true;
        assert!(matches!(
            resolve(&pool, head).await.unwrap(),
            Outcome::Redirect { .. }
        ));

        let (first, second) = tokio::join!(
            resolve(&pool, request("limited")),
            resolve(&pool, request("limited"))
        );
        assert!(matches!(
            (first.unwrap(), second.unwrap()),
            (Outcome::Redirect { .. }, Outcome::Blocked)
                | (Outcome::Blocked, Outcome::Redirect { .. })
        ));
        assert_eq!(resolve(&pool, head).await.unwrap(), Outcome::Blocked);

        pool.close().await;
        let pool = open_database(&directory.path().join("links.sqlite"))
            .await
            .unwrap();
        configure(
            &pool,
            link,
            Policy {
                max_visits: Some(2),
                ..Policy::default()
            },
        )
        .await;
        let details = read_policy(&pool, &[HOST.to_owned()], link)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(details.visits_used, 1);
        assert!(matches!(
            resolve(&pool, request("limited")).await.unwrap(),
            Outcome::Redirect { .. }
        ));
        assert_eq!(
            resolve(&pool, request("limited")).await.unwrap(),
            Outcome::Blocked
        );
    }

    #[tokio::test]
    async fn policy_mutations_require_host_scope_and_reject_permanent_limits() {
        let (_directory, pool) = database().await;
        let link = insert(&pool, "scoped", true).await;
        let unauthorized = ["other.example".to_owned()];
        assert!(
            replace_policy(&pool, &unauthorized, link, Policy::default())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            read_policy(&pool, &unauthorized, link)
                .await
                .unwrap()
                .is_none()
        );
        let hosts = [HOST.to_owned()];
        for policy in [
            Policy {
                status: 301,
                max_visits: Some(1),
                ..Policy::default()
            },
            Policy {
                status: 308,
                starts_at: Some(1),
                ..Policy::default()
            },
            Policy {
                starts_at: Some(2),
                expires_at: Some(2),
                ..Policy::default()
            },
            Policy {
                max_visits: Some(0),
                ..Policy::default()
            },
            Policy {
                status: 303,
                ..Policy::default()
            },
        ] {
            assert!(matches!(
                replace_policy(&pool, &hosts, link, policy).await,
                Err(PolicyError::Invalid(_))
            ));
        }
        let details = read_policy(&pool, &hosts, link).await.unwrap().unwrap();
        assert_eq!(details.policy, Policy::default());
    }
}
