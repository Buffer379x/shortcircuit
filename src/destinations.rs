//! Opt-in destination host policy.
//!
//! When enabled, a destination is permitted unless a block rule matches its host. If at least
//! one allow rule exists, the host must also match an allow rule. A matching block rule always
//! takes precedence over a matching allow rule. Host comparison uses the normalized ASCII form
//! that the URL parser produces: lowercase, IDNA-encoded, and without a trailing dot. The stored
//! destination text is never rewritten.

use std::sync::{Arc, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use sqlx::{Executor, Sqlite, SqliteConnection, SqlitePool};
use url::{Host, Url};

use crate::input::normalize_host;

pub(crate) const MAX_RULES: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuleAction {
    Allow,
    Block,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    pub(crate) host: String,
    /// Also match every name below the host.
    #[serde(default)]
    pub(crate) subdomains: bool,
    pub(crate) action: RuleAction,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    pub(crate) enabled: bool,
    pub(crate) rules: Vec<Rule>,
}

/// Why a destination is not permitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Denial {
    /// A block rule matches the destination host.
    Blocked,
    /// Allow rules exist and none matches the destination host.
    NotAllowed,
    /// The destination has no host that the policy can evaluate.
    Unparsable,
}

#[derive(Debug)]
pub(crate) struct Compiled {
    generation: i64,
    policy: Policy,
}

impl Compiled {
    fn unloaded() -> Self {
        Self {
            generation: -1,
            policy: Policy::default(),
        }
    }

    /// A disabled policy, which permits every destination.
    #[cfg(test)]
    pub(crate) fn permissive() -> Self {
        Self::unloaded()
    }

    /// An enabled policy with one block rule for `host`.
    #[cfg(test)]
    pub(crate) fn blocking(host: &str) -> Self {
        Self {
            generation: 0,
            policy: Policy {
                enabled: true,
                rules: vec![Rule {
                    host: host.to_owned(),
                    subdomains: false,
                    action: RuleAction::Block,
                }],
            },
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.policy.enabled
    }

    pub(crate) fn permits(&self, target: &str) -> bool {
        self.check(target).is_ok()
    }

    pub(crate) fn check(&self, target: &str) -> Result<(), Denial> {
        if !self.policy.enabled {
            return Ok(());
        }
        let host = destination_host(target).ok_or(Denial::Unparsable)?;
        decide(&self.policy.rules, &host)
    }
}

fn decide(rules: &[Rule], host: &str) -> Result<(), Denial> {
    let mut allow_rules = false;
    let mut allowed = false;
    for rule in rules {
        let matched = rule_matches(rule, host);
        match rule.action {
            RuleAction::Block if matched => return Err(Denial::Blocked),
            RuleAction::Block => {}
            RuleAction::Allow => {
                allow_rules = true;
                allowed |= matched;
            }
        }
    }
    if allow_rules && !allowed {
        Err(Denial::NotAllowed)
    } else {
        Ok(())
    }
}

fn rule_matches(rule: &Rule, host: &str) -> bool {
    host == rule.host
        || (rule.subdomains
            && host.len() > rule.host.len()
            && host.ends_with(rule.host.as_str())
            && host.as_bytes()[host.len() - rule.host.len() - 1] == b'.')
}

/// The normalized host that the policy compares for a stored destination.
pub(crate) fn destination_host(target: &str) -> Option<String> {
    canonical_host(Url::parse(target).ok()?.host()?)
}

fn canonical_host(host: Host<&str>) -> Option<String> {
    match host {
        Host::Domain(name) => {
            let name = name.strip_suffix('.').unwrap_or(name);
            // A name with an empty label has no single DNS meaning, so the policy cannot
            // match it reliably. It is treated as unparsable and refused while enabled.
            (!name.is_empty() && !name.split('.').any(str::is_empty))
                .then(|| name.to_ascii_lowercase())
        }
        Host::Ipv4(address) => Some(address.to_string()),
        // Browsers reach an IPv4-mapped IPv6 literal over IPv4, so compare it as IPv4.
        Host::Ipv6(address) => Some(match address.to_ipv4_mapped() {
            Some(mapped) => mapped.to_string(),
            None => format!("[{address}]"),
        }),
    }
}

/// Normalizes an administrator-supplied rule. Unicode names are converted with IDNA.
pub(crate) fn normalize_rule(rule: &Rule) -> Result<Rule, &'static str> {
    let input = rule.host.trim();
    if input.is_empty()
        || input.len() > 255
        || input.contains(['*', '/', ':', '@', ' ']) && !input.starts_with('[')
    {
        return Err("invalid_destination_rule");
    }
    let parsed = Host::parse(input).map_err(|_| "invalid_destination_rule")?;
    let is_name = matches!(parsed, Host::Domain(_));
    let host = match parsed {
        Host::Domain(name) => {
            let canonical =
                canonical_host(Host::Domain(name.as_str())).ok_or("invalid_destination_rule")?;
            normalize_host(&canonical).ok_or("invalid_destination_rule")?
        }
        Host::Ipv4(address) => address.to_string(),
        Host::Ipv6(address) => format!("[{address}]"),
    };
    if rule.subdomains && !is_name {
        return Err("invalid_destination_rule");
    }
    Ok(Rule {
        host,
        subdomains: rule.subdomains,
        action: rule.action,
    })
}

/// Validates and normalizes a complete policy before it is stored.
pub(crate) fn normalize_policy(policy: &Policy) -> Result<Policy, &'static str> {
    if policy.rules.len() > MAX_RULES {
        return Err("too_many_destination_rules");
    }
    let mut rules = Vec::with_capacity(policy.rules.len());
    for rule in &policy.rules {
        let rule = normalize_rule(rule)?;
        if rules.iter().any(|existing: &Rule| {
            existing.host == rule.host && existing.subdomains == rule.subdomains
        }) {
            return Err("duplicate_destination_rule");
        }
        rules.push(rule);
    }
    // Match the stored order: block rules first, then by host and subdomain flag.
    rules.sort_by(|a, b| {
        (a.action == RuleAction::Allow, &a.host, a.subdomains).cmp(&(
            b.action == RuleAction::Allow,
            &b.host,
            b.subdomains,
        ))
    });
    Ok(Policy {
        enabled: policy.enabled,
        rules,
    })
}

#[derive(sqlx::FromRow)]
struct StoredPolicy {
    generation: i64,
    enabled: bool,
    rules: String,
}

/// Reads the stored policy with one statement, so any executor can supply it.
pub(crate) async fn load<'e>(
    executor: impl Executor<'e, Database = Sqlite>,
) -> Result<Compiled, sqlx::Error> {
    let stored = sqlx::query_as::<_, StoredPolicy>(
        "SELECT generation, enabled, (
            SELECT coalesce(json_group_array(json_object(
                'host', host,
                'subdomains', json(iif(subdomains, 'true', 'false')),
                'action', action
            ) ORDER BY action DESC, host, subdomains), '[]') FROM destination_rules
         ) AS rules
         FROM destination_policy WHERE id = 1",
    )
    .fetch_one(executor)
    .await?;
    let rules = serde_json::from_str(&stored.rules)
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
    Ok(Compiled {
        generation: stored.generation,
        policy: Policy {
            enabled: stored.enabled,
            rules,
        },
    })
}

pub(crate) async fn read(pool: &SqlitePool) -> Result<Policy, sqlx::Error> {
    Ok(load(pool).await?.policy)
}

/// Replaces the policy and increments its generation in the given transaction.
pub(crate) async fn replace(
    connection: &mut SqliteConnection,
    policy: &Policy,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM destination_rules")
        .execute(&mut *connection)
        .await?;
    for rule in &policy.rules {
        sqlx::query("INSERT INTO destination_rules (host, subdomains, action) VALUES (?, ?, ?)")
            .bind(&rule.host)
            .bind(rule.subdomains)
            .bind(match rule.action {
                RuleAction::Allow => "allow",
                RuleAction::Block => "block",
            })
            .execute(&mut *connection)
            .await?;
    }
    sqlx::query(
        "UPDATE destination_policy SET enabled = ?, generation = generation + 1 WHERE id = 1",
    )
    .bind(policy.enabled)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Checks destinations written by an administrator against the stored policy.
/// Destinations equal to an already saved value are accepted so unrelated edits keep working.
pub(crate) async fn check_writes<'e, 'a>(
    executor: impl Executor<'e, Database = Sqlite>,
    targets: impl IntoIterator<Item = &'a str>,
    unchanged: &[&str],
) -> Result<Result<(), Denial>, sqlx::Error> {
    let compiled = load(executor).await?;
    if !compiled.enabled() {
        return Ok(Ok(()));
    }
    for target in targets {
        if unchanged.contains(&target) {
            continue;
        }
        if let Err(denial) = compiled.check(target) {
            return Ok(Err(denial));
        }
    }
    Ok(Ok(()))
}

/// A process-local copy of the compiled policy for the public redirect path.
/// The redirect query returns the stored generation, so a change made through any connection
/// is observed by the next request without an additional query when the policy is unchanged.
#[derive(Clone)]
pub(crate) struct Guard(Arc<RwLock<Arc<Compiled>>>);

impl Default for Guard {
    fn default() -> Self {
        Self(Arc::new(RwLock::new(Arc::new(Compiled::unloaded()))))
    }
}

impl Guard {
    pub(crate) async fn current<'e>(
        &self,
        executor: impl Executor<'e, Database = Sqlite>,
        generation: i64,
    ) -> Result<Arc<Compiled>, sqlx::Error> {
        {
            let cached = self.0.read().unwrap_or_else(PoisonError::into_inner);
            if cached.generation == generation {
                return Ok(Arc::clone(&cached));
            }
        }
        let loaded = Arc::new(load(executor).await?);
        let mut cached = self.0.write().unwrap_or_else(PoisonError::into_inner);
        if loaded.generation >= cached.generation {
            *cached = Arc::clone(&loaded);
        }
        Ok(loaded)
    }

    /// Reads the generation first. Used outside the link lookup, for example for fallbacks.
    pub(crate) async fn current_from(
        &self,
        pool: &SqlitePool,
    ) -> Result<Arc<Compiled>, sqlx::Error> {
        let generation: i64 =
            sqlx::query_scalar("SELECT generation FROM destination_policy WHERE id = 1")
                .fetch_one(pool)
                .await?;
        self.current(pool, generation).await
    }
}

/// How the policy affects saved destinations.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Impact {
    /// Links whose saved default destination is not permitted.
    pub(crate) links: i64,
    /// Links with at least one redirect rule whose destination is not permitted.
    pub(crate) rule_links: i64,
    /// Domain root or missing-path destinations that are not permitted.
    pub(crate) fallbacks: i64,
    /// Up to ten affected links, lowest ID first.
    pub(crate) examples: Vec<AffectedLink>,
}

#[derive(Debug, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub(crate) struct AffectedLink {
    pub(crate) id: i64,
    pub(crate) host: String,
    pub(crate) slug: String,
}

#[derive(sqlx::FromRow)]
struct SavedDestinations {
    id: i64,
    host: String,
    slug: String,
    target: String,
    rules: Option<String>,
}

#[derive(Deserialize)]
struct RuleTarget {
    target: String,
}

const MAX_EXAMPLES: usize = 10;

/// Counts saved destinations that the given policy would not permit.
pub(crate) async fn impact(pool: &SqlitePool, policy: &Policy) -> Result<Impact, sqlx::Error> {
    let compiled = Compiled {
        generation: 0,
        policy: policy.clone(),
    };
    let mut result = Impact::default();
    if !compiled.enabled() {
        return Ok(result);
    }
    let mut transaction = pool.begin().await?;
    let rows = sqlx::query_as::<_, SavedDestinations>(
        "SELECT links.id, links.host, links.slug, links.target, policy.rules
         FROM links LEFT JOIN link_redirect_policies AS policy ON policy.link_id = links.id
         ORDER BY links.id",
    )
    .fetch_all(&mut *transaction)
    .await?;
    for row in rows {
        let default_denied = !compiled.permits(&row.target);
        let rule_denied = row
            .rules
            .as_deref()
            .and_then(|rules| serde_json::from_str::<Vec<RuleTarget>>(rules).ok())
            .is_some_and(|rules| rules.iter().any(|rule| !compiled.permits(&rule.target)));
        result.links += i64::from(default_denied);
        result.rule_links += i64::from(rule_denied);
        if (default_denied || rule_denied) && result.examples.len() < MAX_EXAMPLES {
            result.examples.push(AffectedLink {
                id: row.id,
                host: row.host,
                slug: row.slug,
            });
        }
    }
    let fallbacks = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT root_target, missing_target FROM domains",
    )
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;
    result.fallbacks = fallbacks
        .iter()
        .flat_map(|(root, missing)| [root, missing])
        .flatten()
        .filter(|target| !compiled.permits(target))
        .count() as i64;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(host: &str, subdomains: bool, action: RuleAction) -> Rule {
        Rule {
            host: host.to_owned(),
            subdomains,
            action,
        }
    }

    fn policy(rules: Vec<Rule>) -> Compiled {
        Compiled {
            generation: 1,
            policy: normalize_policy(&Policy {
                enabled: true,
                rules,
            })
            .unwrap(),
        }
    }

    #[test]
    fn rules_normalize_case_idn_trailing_dots_and_addresses() {
        for (input, expected) in [
            ("Example.COM.", "example.com"),
            ("  docs.example ", "docs.example"),
            ("Bücher.Example", "xn--bcher-kva.example"),
            ("xn--bcher-kva.example", "xn--bcher-kva.example"),
            ("192.0.2.1", "192.0.2.1"),
            ("[2001:DB8::1]", "[2001:db8::1]"),
        ] {
            assert_eq!(
                normalize_rule(&rule(input, false, RuleAction::Block))
                    .unwrap()
                    .host,
                expected,
                "{input}"
            );
        }
        for input in [
            "",
            "*.example.com",
            "https://example.com",
            "example.com/path",
            "user@example.com",
            "exa mple.com",
            "-bad.example",
            "under_score.example",
        ] {
            assert!(
                normalize_rule(&rule(input, false, RuleAction::Allow)).is_err(),
                "{input}"
            );
        }
        assert!(normalize_rule(&rule("192.0.2.1", true, RuleAction::Allow)).is_err());
        assert_eq!(
            normalize_policy(&Policy {
                enabled: true,
                rules: vec![
                    rule("Example.com", false, RuleAction::Allow),
                    rule("example.COM.", false, RuleAction::Block),
                ],
            }),
            Err("duplicate_destination_rule")
        );
    }

    #[test]
    fn destinations_compare_their_parsed_host() {
        for (target, expected) in [
            ("https://EXAMPLE.com/Path?Q=1#F", "example.com"),
            ("https://example.com./", "example.com"),
            ("https://xn--bcher-kva.example/", "xn--bcher-kva.example"),
            ("https://ex%61mple.com/", "example.com"),
            ("http://[2001:db8::1]:8080/", "[2001:db8::1]"),
            ("http://0x7f.1/", "127.0.0.1"),
            ("http://[::ffff:7f00:1]/", "127.0.0.1"),
            ("http://[::FFFF:192.0.2.1]:8080/", "192.0.2.1"),
        ] {
            assert_eq!(
                destination_host(target).as_deref(),
                Some(expected),
                "{target}"
            );
        }
        for target in ["https://blocked.example../", "https://a..example/"] {
            assert_eq!(destination_host(target), None, "{target}");
        }
        let blocked = policy(vec![
            rule("127.0.0.1", false, RuleAction::Block),
            rule("blocked.example", true, RuleAction::Block),
        ]);
        assert!(!blocked.permits("http://[::ffff:7f00:1]/"));
        assert_eq!(
            blocked.check("https://blocked.example../"),
            Err(Denial::Unparsable)
        );
        assert!(blocked.permits("http://[2001:db8::1]/"));
    }

    #[test]
    fn block_rules_take_precedence_and_subdomains_are_explicit() {
        let compiled = policy(vec![
            rule("example.com", true, RuleAction::Allow),
            rule("private.example.com", false, RuleAction::Block),
            rule("partner.test", false, RuleAction::Allow),
            rule("partner.test", true, RuleAction::Block),
        ]);
        for (target, expected) in [
            ("https://example.com/", Ok(())),
            ("https://DOCS.Example.com./x", Ok(())),
            ("https://a.b.example.com/", Ok(())),
            ("https://private.example.com/", Err(Denial::Blocked)),
            ("https://deeper.private.example.com/", Ok(())),
            ("https://badexample.com/", Err(Denial::NotAllowed)),
            ("https://example.com.evil.test/", Err(Denial::NotAllowed)),
            ("https://partner.test/", Err(Denial::Blocked)),
            ("https://other.test/", Err(Denial::NotAllowed)),
        ] {
            assert_eq!(compiled.check(target), expected, "{target}");
        }

        let exact = policy(vec![rule("example.com", false, RuleAction::Allow)]);
        assert!(exact.permits("https://example.com/"));
        assert!(!exact.permits("https://www.example.com/"));

        let blocklist = policy(vec![rule("Bücher.example", true, RuleAction::Block)]);
        assert!(!blocklist.permits("https://shop.xn--bcher-kva.example/"));
        assert!(blocklist.permits("https://example.org/"));

        let disabled = Compiled {
            generation: 1,
            policy: Policy {
                enabled: false,
                rules: vec![rule("example.com", true, RuleAction::Block)],
            },
        };
        assert!(disabled.permits("https://example.com/"));
        let empty = policy(Vec::new());
        assert!(empty.permits("https://anything.example/"));
    }

    #[tokio::test]
    async fn stored_policies_round_trip_and_bump_the_generation() {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("policy.db"))
            .await
            .unwrap();
        let initial = load(&pool).await.unwrap();
        assert_eq!(initial.generation, 0);
        assert_eq!(initial.policy, Policy::default());
        let saved = normalize_policy(&Policy {
            enabled: true,
            rules: vec![
                rule("Example.com", true, RuleAction::Allow),
                rule("bad.example.com", false, RuleAction::Block),
            ],
        })
        .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        replace(&mut transaction, &saved).await.unwrap();
        transaction.commit().await.unwrap();
        let loaded = load(&pool).await.unwrap();
        assert_eq!(loaded.generation, 1);
        assert_eq!(loaded.policy, saved);

        let guard = Guard::default();
        let cached = guard.current_from(&pool).await.unwrap();
        assert!(!cached.permits("https://bad.example.com/"));
        let mut transaction = pool.begin().await.unwrap();
        replace(&mut transaction, &Policy::default()).await.unwrap();
        transaction.commit().await.unwrap();
        assert!(
            guard
                .current_from(&pool)
                .await
                .unwrap()
                .permits("https://bad.example.com/")
        );
    }
}
