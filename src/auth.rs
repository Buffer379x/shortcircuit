use std::{fmt, sync::Arc};

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{Error as PasswordError, PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, SqliteConnection, SqlitePool};
use subtle::ConstantTimeEq;
use tokio::{sync::Semaphore, task};

use crate::admin::admin_security::{AuditAction, AuditActor, AuditOutcome, record_audit};

pub const BOOTSTRAP_LIFETIME_SECONDS: i64 = 15 * 60;
const SESSION_IDLE_SECONDS: i64 = 30 * 60;
const SESSION_ABSOLUTE_SECONDS: i64 = 12 * 60 * 60;
const MAX_PASSWORD_BYTES: usize = 1024;
const PASSWORD_WORKERS: usize = 2;
const TOKEN_BYTES: usize = 32;
const SESSION_DOMAIN: &[u8] = b"shortcircuit/session/v1\0";
const CSRF_DOMAIN: &[u8] = b"shortcircuit/csrf/v1\0";
const KEY_DOMAIN: &[u8] = b"shortcircuit/api-key/v1\0";
const KEY_SCOPES: [(&str, i64); 3] = [("read", 1), ("write", 2), ("delete", 4)];

// Keep unknown usernames on the same password-verification path as existing accounts.
const DUMMY_PASSWORD_HASH: &str = concat!(
    "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA",
    "$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
);

#[derive(Clone)]
pub struct Auth {
    pool: SqlitePool,
    password_workers: Arc<Semaphore>,
}

#[derive(Clone, Copy)]
pub enum BootstrapKind {
    Setup,
    Recovery,
}

impl BootstrapKind {
    fn name(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Recovery => "recovery",
        }
    }

    fn domain(self) -> &'static [u8] {
        match self {
            Self::Setup => b"shortcircuit/setup/v1\0",
            Self::Recovery => b"shortcircuit/recovery/v1\0",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    Invalid,
    Unauthorized,
    Conflict,
    Busy,
    NotFound,
    Unavailable,
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "invalid authentication input",
            Self::Unauthorized => "invalid or expired credentials",
            Self::Conflict => "operation conflicts with the account state",
            Self::Busy => "password verification is busy",
            Self::NotFound => "authentication record not found",
            Self::Unavailable => "authentication service unavailable",
        })
    }
}

impl std::error::Error for AuthError {}

impl From<sqlx::Error> for AuthError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}

#[derive(Serialize)]
pub struct Session {
    pub username: String,
    pub csrf: String,
}

pub struct IssuedSession {
    pub token: String,
    pub session: Session,
}

#[derive(Serialize)]
pub struct KeyMeta {
    pub id: i64,
    pub name: String,
    pub scopes: Vec<String>,
    pub host: Option<String>,
    pub expires_at: Option<i64>,
    pub created_at: i64,
}

pub struct IssuedKey {
    pub key: KeyMeta,
    pub token: String,
}

pub struct KeyAccess {
    pub(crate) id: i64,
    pub host: Option<String>,
    scopes: i64,
}

impl KeyAccess {
    pub fn allows(&self, scope: &str) -> bool {
        scope_bit(scope).is_some_and(|bit| self.scopes & bit != 0)
    }
}

type KeyRow = (i64, String, i64, Option<String>, Option<i64>, i64);
type RotationSource = (String, i64, Option<String>, Option<i64>, i64);

struct VerifiedSession {
    digest: [u8; TOKEN_BYTES],
    password_hash: String,
}

impl From<KeyRow> for KeyMeta {
    fn from((id, name, scopes, host, expires_at, created_at): KeyRow) -> Self {
        Self {
            id,
            name,
            scopes: KEY_SCOPES
                .into_iter()
                .filter(|(_, bit)| scopes & *bit != 0)
                .map(|(name, _)| name.to_owned())
                .collect(),
            host,
            expires_at,
            created_at,
        }
    }
}

impl Auth {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            password_workers: Arc::new(Semaphore::new(PASSWORD_WORKERS)),
        }
    }

    pub async fn has_account(&self) -> Result<bool, AuthError> {
        Ok(
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM admin_accounts WHERE id = 1)")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn issue_bootstrap(&self, kind: BootstrapKind) -> Result<String, AuthError> {
        let token = new_token()?;
        let digest = token_hash(kind.domain(), &token);
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
            "INSERT INTO admin_bootstrap (kind, token_hash, expires_at)
             SELECT ?, ?, unixepoch() + ?
             WHERE EXISTS(SELECT 1 FROM admin_accounts WHERE id = 1) = ?
             ON CONFLICT(kind) DO UPDATE SET
                 token_hash = excluded.token_hash, expires_at = excluded.expires_at",
        )
        .bind(kind.name())
        .bind(digest.as_slice())
        .bind(BOOTSTRAP_LIFETIME_SECONDS)
        .bind(matches!(kind, BootstrapKind::Recovery))
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(AuthError::Conflict);
        }
        let action = match kind {
            BootstrapKind::Setup => AuditAction::SetupTokenIssued,
            BootstrapKind::Recovery => AuditAction::RecoveryTokenIssued,
        };
        record_audit(
            &mut transaction,
            action,
            AuditActor::System,
            None,
            AuditOutcome::Success,
        )
        .await?;
        transaction.commit().await?;
        Ok(token)
    }

    pub async fn setup(
        &self,
        token: &str,
        username: &str,
        password: &str,
    ) -> Result<IssuedSession, AuthError> {
        let username = valid_label(username).ok_or(AuthError::Invalid)?;
        let kind = BootstrapKind::Setup;
        let digest = self.check_bootstrap(kind, token).await?;
        let hash = self.hash_password(password).await?;
        let mut transaction = self.pool.begin().await?;
        consume_bootstrap(&mut transaction, kind, &digest).await?;
        let result = sqlx::query(
            "INSERT INTO admin_accounts (id, username, password_hash)
             VALUES (1, ?, ?) ON CONFLICT(id) DO NOTHING",
        )
        .bind(username)
        .bind(&hash)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(AuthError::Conflict);
        }
        let session = insert_session(&mut transaction, &hash).await?;
        record_audit(
            &mut transaction,
            AuditAction::AccountSetup,
            AuditActor::Admin,
            None,
            AuditOutcome::Success,
        )
        .await?;
        transaction.commit().await?;
        Ok(session)
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<IssuedSession, AuthError> {
        let username = valid_label(username).ok_or(AuthError::Unauthorized)?;
        let stored: Option<String> =
            sqlx::query_scalar("SELECT password_hash FROM admin_accounts WHERE username = ?")
                .bind(username)
                .fetch_optional(&self.pool)
                .await?;
        let upgraded = self
            .verify_password(
                password,
                stored.as_deref().unwrap_or(DUMMY_PASSWORD_HASH),
                true,
            )
            .await?;
        let mut hash = stored.ok_or(AuthError::Unauthorized)?;
        let mut transaction = self.pool.begin().await?;
        if let Some(upgraded) = upgraded {
            let result = sqlx::query(
                "UPDATE admin_accounts SET password_hash = ? WHERE id = 1 AND password_hash = ?",
            )
            .bind(&upgraded)
            .bind(&hash)
            .execute(&mut *transaction)
            .await?;
            if result.rows_affected() != 1 {
                return Err(AuthError::Unauthorized);
            }
            hash = upgraded;
        }
        let session = insert_session(&mut transaction, &hash).await?;
        sqlx::query(
            "DELETE FROM admin_sessions
             WHERE last_seen_at <= unixepoch() - ? OR created_at <= unixepoch() - ?",
        )
        .bind(SESSION_IDLE_SECONDS)
        .bind(SESSION_ABSOLUTE_SECONDS)
        .execute(&mut *transaction)
        .await?;
        record_audit(
            &mut transaction,
            AuditAction::Login,
            AuditActor::Admin,
            None,
            AuditOutcome::Success,
        )
        .await?;
        transaction.commit().await?;
        Ok(session)
    }

    pub async fn recover(&self, token: &str, password: &str) -> Result<IssuedSession, AuthError> {
        let kind = BootstrapKind::Recovery;
        let digest = self.check_bootstrap(kind, token).await?;
        let hash = self.hash_password(password).await?;
        let mut transaction = self.pool.begin().await?;
        consume_bootstrap(&mut transaction, kind, &digest).await?;
        let result = sqlx::query("UPDATE admin_accounts SET password_hash = ? WHERE id = 1")
            .bind(&hash)
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() != 1 {
            return Err(AuthError::Unauthorized);
        }
        let session = replace_sessions(&mut transaction, &hash).await?;
        record_audit(
            &mut transaction,
            AuditAction::AccountRecovery,
            AuditActor::Admin,
            None,
            AuditOutcome::Success,
        )
        .await?;
        transaction.commit().await?;
        Ok(session)
    }

    pub async fn session(&self, token: &str) -> Result<Session, AuthError> {
        let digest = credential_hash(SESSION_DOMAIN, token)?;
        let usernames = sqlx::query_scalar(
            "UPDATE admin_sessions SET last_seen_at = max(last_seen_at, unixepoch())
             WHERE token_hash = ? AND last_seen_at > unixepoch() - ?
                 AND created_at > unixepoch() - ?
             RETURNING (SELECT username FROM admin_accounts WHERE id = account_id)",
        )
        .bind(digest.as_slice())
        .bind(SESSION_IDLE_SECONDS)
        .bind(SESSION_ABSOLUTE_SECONDS)
        .fetch_all(&self.pool)
        .await?;
        let username = usernames
            .into_iter()
            .next()
            .ok_or(AuthError::Unauthorized)?;
        Ok(session_credentials(username, token))
    }

    pub async fn logout(&self, token: &str) -> Result<(), AuthError> {
        let digest = credential_hash(SESSION_DOMAIN, token)?;
        sqlx::query("DELETE FROM admin_sessions WHERE token_hash = ?")
            .bind(digest.as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn change_password(
        &self,
        token: &str,
        current: &str,
        password: &str,
    ) -> Result<IssuedSession, AuthError> {
        let verified = self.verify_current_session(token, current).await?;
        let hash = self.hash_password(password).await?;
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
            "UPDATE admin_accounts SET password_hash = ?
             WHERE id = 1 AND password_hash = ? AND EXISTS(
                 SELECT 1 FROM admin_sessions WHERE token_hash = ?
                     AND last_seen_at > unixepoch() - ? AND created_at > unixepoch() - ?
             )",
        )
        .bind(&hash)
        .bind(verified.password_hash)
        .bind(verified.digest.as_slice())
        .bind(SESSION_IDLE_SECONDS)
        .bind(SESSION_ABSOLUTE_SECONDS)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(AuthError::Unauthorized);
        }
        let session = replace_sessions(&mut transaction, &hash).await?;
        record_audit(
            &mut transaction,
            AuditAction::PasswordChange,
            AuditActor::Admin,
            None,
            AuditOutcome::Success,
        )
        .await?;
        transaction.commit().await?;
        Ok(session)
    }

    pub(crate) async fn reauthenticate(&self, token: &str, current: &str) -> Result<(), AuthError> {
        let verified = self.verify_current_session(token, current).await?;
        verified.require_current(&self.pool).await
    }

    async fn verify_current_session(
        &self,
        token: &str,
        current: &str,
    ) -> Result<VerifiedSession, AuthError> {
        let digest = credential_hash(SESSION_DOMAIN, token)?;
        let password_hash = session_password_hash(&self.pool, &digest).await?;
        self.verify_password(current, &password_hash, false).await?;
        Ok(VerifiedSession {
            digest,
            password_hash,
        })
    }

    pub async fn list_keys(&self) -> Result<Vec<KeyMeta>, AuthError> {
        let rows = sqlx::query_as::<_, KeyRow>(
            "SELECT id, name, scopes, host, expires_at, created_at FROM admin_api_keys ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(KeyMeta::from).collect())
    }

    pub async fn create_key(
        &self,
        name: &str,
        scopes: Vec<String>,
        host: Option<String>,
        expires_at: Option<i64>,
    ) -> Result<IssuedKey, AuthError> {
        let name = valid_label(name).ok_or(AuthError::Invalid)?;
        let mut bits = 0;
        for scope in &scopes {
            bits |= scope_bit(scope).ok_or(AuthError::Invalid)?;
        }
        if bits == 0 {
            return Err(AuthError::Invalid);
        }
        let host = host
            .as_deref()
            .map(|host| crate::input::normalize_host(host).ok_or(AuthError::Invalid))
            .transpose()?;
        let secret = new_token()?;
        let digest = token_hash(KEY_DOMAIN, &secret);
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query_as::<_, KeyRow>(
            "INSERT INTO admin_api_keys (account_id, token_hash, name, scopes, host, expires_at, created_at)
             SELECT 1, ?, ?, ?, ?, ?, unixepoch() WHERE coalesce(? > unixepoch(), 1)
             RETURNING id, name, scopes, host, expires_at, created_at",
        )
        .bind(digest.as_slice())
        .bind(name)
        .bind(bits)
        .bind(host)
        .bind(expires_at)
        .bind(expires_at)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AuthError::Invalid)?;
        record_audit(
            &mut transaction,
            AuditAction::KeyCreate,
            AuditActor::Admin,
            Some(row.0),
            AuditOutcome::Success,
        )
        .await?;
        transaction.commit().await?;
        Ok(issued_key(row, &secret))
    }

    pub async fn rotate_key(
        &self,
        id: i64,
        token: &str,
        current: &str,
    ) -> Result<IssuedKey, AuthError> {
        let verified = self.verify_current_session(token, current).await?;
        let secret = new_token()?;
        let digest = token_hash(KEY_DOMAIN, &secret);
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        verified.require_current(&mut *transaction).await?;
        let (name, scopes, host, expires_at, now) = sqlx::query_as::<_, RotationSource>(
            "SELECT name, scopes, host, expires_at, unixepoch() FROM admin_api_keys WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AuthError::NotFound)?;
        if expires_at.is_some_and(|expiry| expiry <= now) {
            return Err(AuthError::Conflict);
        }
        let row = sqlx::query_as::<_, KeyRow>(
            "INSERT INTO admin_api_keys (account_id, token_hash, name, scopes, host, expires_at, created_at)
             VALUES (1, ?, ?, ?, ?, ?, ?)
             RETURNING id, name, scopes, host, expires_at, created_at",
        )
        .bind(digest.as_slice())
        .bind(name)
        .bind(scopes)
        .bind(host)
        .bind(expires_at)
        .bind(now)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM admin_api_keys WHERE id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        record_audit(
            &mut transaction,
            AuditAction::KeyRotate,
            AuditActor::Admin,
            Some(id),
            AuditOutcome::Success,
        )
        .await?;
        transaction.commit().await?;
        Ok(issued_key(row, &secret))
    }

    pub async fn key(&self, token: &str) -> Result<KeyAccess, AuthError> {
        let (id, secret) = token.split_once('.').ok_or(AuthError::Unauthorized)?;
        let id = id
            .parse::<i64>()
            .ok()
            .filter(|value| *value > 0 && value.to_string() == id)
            .ok_or(AuthError::Unauthorized)?;
        let digest = credential_hash(KEY_DOMAIN, secret)?;
        let (stored, scopes, host): (Vec<u8>, i64, Option<String>) = sqlx::query_as(
            "SELECT token_hash, scopes, host FROM admin_api_keys
             WHERE id = ? AND (expires_at IS NULL OR expires_at > unixepoch())",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(AuthError::Unauthorized)?;
        if !bool::from(stored.as_slice().ct_eq(digest.as_slice())) {
            return Err(AuthError::Unauthorized);
        }
        Ok(KeyAccess { id, host, scopes })
    }

    pub async fn revoke_key(&self, id: i64) -> Result<(), AuthError> {
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query("DELETE FROM admin_api_keys WHERE id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AuthError::NotFound);
        }
        record_audit(
            &mut transaction,
            AuditAction::KeyRevoke,
            AuditActor::Admin,
            Some(id),
            AuditOutcome::Success,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn check_bootstrap(
        &self,
        kind: BootstrapKind,
        token: &str,
    ) -> Result<[u8; TOKEN_BYTES], AuthError> {
        let digest = credential_hash(kind.domain(), token)?;
        let available: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM admin_bootstrap
             WHERE kind = ? AND token_hash = ? AND expires_at > unixepoch())",
        )
        .bind(kind.name())
        .bind(digest.as_slice())
        .fetch_one(&self.pool)
        .await?;
        if !available {
            return Err(AuthError::Unauthorized);
        }
        Ok(digest)
    }

    pub(crate) async fn hash_password(&self, password: &str) -> Result<String, AuthError> {
        if password.len() > MAX_PASSWORD_BYTES || password.chars().count() < 12 {
            return Err(AuthError::Invalid);
        }
        let password = password.to_owned();
        self.password_work(move || make_password_hash(&password))
            .await
    }

    pub(crate) async fn verify_password(
        &self,
        password: &str,
        stored: &str,
        upgrade: bool,
    ) -> Result<Option<String>, AuthError> {
        if password.is_empty() || password.len() > MAX_PASSWORD_BYTES {
            return Err(AuthError::Unauthorized);
        }
        let password = password.to_owned();
        let stored = stored.to_owned();
        self.password_work(move || {
            let hash = PasswordHash::new(&stored).map_err(|_| AuthError::Unavailable)?;
            let parameters = Params::try_from(&hash).map_err(|_| AuthError::Unavailable)?;
            let argon2 = Argon2::default();
            argon2
                .verify_password(password.as_bytes(), &hash)
                .map_err(|error| match error {
                    PasswordError::PasswordInvalid => AuthError::Unauthorized,
                    _ => AuthError::Unavailable,
                })?;
            let target = argon2.params();
            let weaker = parameters.m_cost() <= target.m_cost()
                && parameters.t_cost() <= target.t_cost()
                && (parameters.m_cost() < target.m_cost() || parameters.t_cost() < target.t_cost());
            if upgrade && weaker {
                return make_password_hash(&password).map(Some);
            }
            Ok(None)
        })
        .await
    }

    async fn password_work<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, AuthError> + Send + 'static,
    ) -> Result<T, AuthError> {
        let permit = self
            .password_workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthError::Busy)?;
        task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|_| AuthError::Unavailable)?
    }
}

impl VerifiedSession {
    async fn require_current<'e>(
        &self,
        executor: impl sqlx::Executor<'e, Database = Sqlite>,
    ) -> Result<(), AuthError> {
        let current = session_password_hash(executor, &self.digest).await?;
        if current == self.password_hash {
            Ok(())
        } else {
            Err(AuthError::Unauthorized)
        }
    }
}

async fn session_password_hash<'e>(
    executor: impl sqlx::Executor<'e, Database = Sqlite>,
    digest: &[u8; TOKEN_BYTES],
) -> Result<String, AuthError> {
    sqlx::query_scalar(
        "SELECT account.password_hash FROM admin_accounts AS account
         JOIN admin_sessions AS session ON session.account_id = account.id
         WHERE session.token_hash = ? AND session.last_seen_at > unixepoch() - ?
             AND session.created_at > unixepoch() - ?",
    )
    .bind(digest.as_slice())
    .bind(SESSION_IDLE_SECONDS)
    .bind(SESSION_ABSOLUTE_SECONDS)
    .fetch_optional(executor)
    .await?
    .ok_or(AuthError::Unauthorized)
}

fn issued_key(row: KeyRow, secret: &str) -> IssuedKey {
    let key = KeyMeta::from(row);
    let token = format!("{}.{secret}", key.id);
    IssuedKey { key, token }
}

async fn consume_bootstrap(
    connection: &mut SqliteConnection,
    kind: BootstrapKind,
    digest: &[u8; TOKEN_BYTES],
) -> Result<(), AuthError> {
    let result = sqlx::query(
        "DELETE FROM admin_bootstrap WHERE kind = ? AND token_hash = ? AND expires_at > unixepoch()",
    )
    .bind(kind.name())
    .bind(digest.as_slice())
    .execute(connection)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AuthError::Unauthorized);
    }
    Ok(())
}

async fn insert_session(
    connection: &mut SqliteConnection,
    expected_password_hash: &str,
) -> Result<IssuedSession, AuthError> {
    let token = new_token()?;
    let digest = token_hash(SESSION_DOMAIN, &token);
    // A password change can finish while a login is verifying its old hash.
    let usernames = sqlx::query_scalar(
        "INSERT INTO admin_sessions (account_id, token_hash, created_at, last_seen_at)
         SELECT id, ?, unixepoch(), unixepoch() FROM admin_accounts
         WHERE id = 1 AND password_hash = ?
         RETURNING (SELECT username FROM admin_accounts WHERE id = account_id)",
    )
    .bind(digest.as_slice())
    .bind(expected_password_hash)
    .fetch_all(connection)
    .await?;
    let username = usernames
        .into_iter()
        .next()
        .ok_or(AuthError::Unauthorized)?;
    let session = session_credentials(username, &token);
    Ok(IssuedSession { token, session })
}

async fn replace_sessions(
    connection: &mut SqliteConnection,
    password_hash: &str,
) -> Result<IssuedSession, AuthError> {
    sqlx::query("DELETE FROM admin_sessions")
        .execute(&mut *connection)
        .await?;
    sqlx::query("DELETE FROM admin_bootstrap")
        .execute(&mut *connection)
        .await?;
    insert_session(connection, password_hash).await
}

fn session_credentials(username: String, token: &str) -> Session {
    Session {
        username,
        csrf: lowercase_hex(&token_hash(CSRF_DOMAIN, token)),
    }
}

fn make_password_hash(password: &str) -> Result<String, AuthError> {
    let mut salt = [0; 16];
    getrandom::fill(&mut salt).map_err(|_| AuthError::Unavailable)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::DEFAULT);
    let hash = argon2
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|_| AuthError::Unavailable)?;
    Ok(hash.to_string())
}

fn valid_label(label: &str) -> Option<&str> {
    let label = label.trim();
    (!label.is_empty() && label.chars().count() <= 64 && !label.chars().any(char::is_control))
        .then_some(label)
}

fn scope_bit(scope: &str) -> Option<i64> {
    KEY_SCOPES
        .into_iter()
        .find_map(|(name, bit)| (name == scope).then_some(bit))
}

fn new_token() -> Result<String, AuthError> {
    let mut bytes = [0; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Unavailable)?;
    Ok(lowercase_hex(&bytes))
}

fn credential_hash(domain: &[u8], token: &str) -> Result<[u8; TOKEN_BYTES], AuthError> {
    if token.len() != 2 * TOKEN_BYTES
        || !token
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(AuthError::Unauthorized);
    }
    Ok(token_hash(domain, token))
}

fn token_hash(domain: &[u8], token: &str) -> [u8; TOKEN_BYTES] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(token.as_bytes());
    digest.finalize().into()
}

pub(crate) fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut token = String::with_capacity(2 * bytes.len());
    for &byte in bytes {
        token.push(char::from(HEX[usize::from(byte >> 4)]));
        token.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "synthetic-test-password";
    const NEW_PASSWORD: &str = "synthetic-replacement-password";

    async fn database() -> (tempfile::TempDir, Auth) {
        let directory = tempfile::tempdir().unwrap();
        let pool = crate::open_database(&directory.path().join("test.db"))
            .await
            .unwrap();
        (directory, Auth::new(pool))
    }

    async fn account(auth: &Auth) -> IssuedSession {
        let setup = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        auth.setup(&setup, "operator", PASSWORD).await.unwrap()
    }

    #[tokio::test]
    async fn simultaneous_setup_claims_can_create_only_one_account() {
        let (_directory, auth) = database().await;
        let token = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        let (first, second) = tokio::join!(
            auth.setup(&token, "first", PASSWORD),
            auth.setup(&token, "second", PASSWORD),
        );
        assert_ne!(first.is_ok(), second.is_ok());
        assert!(
            matches!(&first, Err(AuthError::Unauthorized))
                || matches!(&second, Err(AuthError::Unauthorized))
        );
        assert!(auth.has_account().await.unwrap());
        assert!(auth.setup(&token, "third", PASSWORD).await.is_err());
        assert!(matches!(
            auth.issue_bootstrap(BootstrapKind::Setup).await,
            Err(AuthError::Conflict)
        ));
        let session = first.ok().or_else(|| second.ok()).unwrap();
        assert!(auth.session(&session.token).await.is_ok());
    }

    #[tokio::test]
    async fn failed_setup_rolls_back_the_account_and_the_claim() {
        let (_directory, auth) = database().await;
        let token = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        sqlx::query(
            "CREATE TRIGGER fail_session BEFORE INSERT ON admin_sessions
             BEGIN SELECT RAISE(ABORT, 'test session failure'); END",
        )
        .execute(&auth.pool)
        .await
        .unwrap();
        assert!(matches!(
            auth.setup(&token, "operator", PASSWORD).await,
            Err(AuthError::Unavailable)
        ));
        assert!(!auth.has_account().await.unwrap());
        sqlx::query("DROP TRIGGER fail_session")
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(auth.setup(&token, "operator", PASSWORD).await.is_ok());
    }

    #[tokio::test]
    async fn an_expired_claim_cannot_create_an_account() {
        let (_directory, auth) = database().await;
        let token = auth.issue_bootstrap(BootstrapKind::Setup).await.unwrap();
        sqlx::query("UPDATE admin_bootstrap SET expires_at = unixepoch()")
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(matches!(
            auth.setup(&token, "operator", PASSWORD).await,
            Err(AuthError::Unauthorized)
        ));
        assert!(!auth.has_account().await.unwrap());
    }

    #[tokio::test]
    async fn simultaneous_recovery_claims_revoke_old_sessions_once() {
        let (_directory, auth) = database().await;
        let old = account(&auth).await;
        let token = auth.issue_bootstrap(BootstrapKind::Recovery).await.unwrap();
        let (first, second) = tokio::join!(
            auth.recover(&token, NEW_PASSWORD),
            auth.recover(&token, NEW_PASSWORD),
        );
        assert_ne!(first.is_ok(), second.is_ok());
        assert!(
            matches!(&first, Err(AuthError::Unauthorized))
                || matches!(&second, Err(AuthError::Unauthorized))
        );
        let recovered = first.ok().or_else(|| second.ok()).unwrap();
        assert!(auth.session(&old.token).await.is_err());
        assert!(auth.session(&recovered.token).await.is_ok());
        assert!(matches!(
            auth.login("operator", PASSWORD).await,
            Err(AuthError::Unauthorized)
        ));
        assert!(auth.login("operator", NEW_PASSWORD).await.is_ok());
    }

    #[tokio::test]
    async fn password_change_rotates_sessions_and_invalidates_pending_recovery() {
        let (_directory, auth) = database().await;
        let old = account(&auth).await;
        let parallel = auth.login("operator", PASSWORD).await.unwrap();
        let recovery = auth.issue_bootstrap(BootstrapKind::Recovery).await.unwrap();
        assert!(matches!(
            auth.change_password(&old.token, NEW_PASSWORD, PASSWORD)
                .await,
            Err(AuthError::Unauthorized)
        ));
        let changed = auth
            .change_password(&old.token, PASSWORD, NEW_PASSWORD)
            .await
            .unwrap();
        assert!(auth.session(&old.token).await.is_err());
        assert!(auth.session(&parallel.token).await.is_err());
        assert!(auth.recover(&recovery, PASSWORD).await.is_err());
        assert!(auth.session(&changed.token).await.is_ok());
        auth.logout(&changed.token).await.unwrap();
        assert!(auth.session(&changed.token).await.is_err());
    }

    #[tokio::test]
    async fn successful_login_upgrades_a_weaker_password_hash() {
        let (_directory, auth) = database().await;
        let existing = account(&auth).await;
        let weak = Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(1024, 1, 1, None).unwrap(),
        )
        .hash_password_with_salt(PASSWORD.as_bytes(), &[7; 16])
        .unwrap()
        .to_string();
        sqlx::query("UPDATE admin_accounts SET password_hash = ?")
            .bind(&weak)
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(auth.login("operator", PASSWORD).await.is_ok());
        assert!(auth.session(&existing.token).await.is_ok());
        let upgraded: String = sqlx::query_scalar("SELECT password_hash FROM admin_accounts")
            .fetch_one(&auth.pool)
            .await
            .unwrap();
        assert!(upgraded.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert_ne!(weak, upgraded);
    }

    #[tokio::test]
    async fn session_touches_extend_only_the_idle_deadline() {
        let (_directory, auth) = database().await;
        let issued = account(&auth).await;
        let row: (Vec<u8>, i64, i64) = sqlx::query_as(
            "UPDATE admin_sessions SET created_at = unixepoch() - 3600,
             last_seen_at = unixepoch() - 1200
             RETURNING token_hash, created_at, last_seen_at",
        )
        .fetch_one(&auth.pool)
        .await
        .unwrap();
        assert_eq!(issued.token.len(), 64);
        assert_eq!(row.0.len(), 32);
        assert_ne!(issued.token, issued.session.csrf);
        let session = auth.session(&issued.token).await.unwrap();
        assert_eq!(session.csrf, issued.session.csrf);
        let touched: (i64, i64) =
            sqlx::query_as("SELECT created_at, last_seen_at FROM admin_sessions")
                .fetch_one(&auth.pool)
                .await
                .unwrap();
        assert_eq!(touched.0, row.1);
        assert!(touched.1 > row.2);
        sqlx::query("UPDATE admin_sessions SET last_seen_at = unixepoch() - 1800")
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(auth.session(&issued.token).await.is_err());
        let active = auth.login("operator", PASSWORD).await.unwrap();
        sqlx::query("UPDATE admin_sessions SET created_at = unixepoch() - 43200")
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(auth.session(&active.token).await.is_err());
    }

    #[tokio::test]
    async fn keys_enforce_their_scopes_expiry_and_revocation() {
        let (_directory, auth) = database().await;
        account(&auth).await;
        let issued = auth
            .create_key(
                "reader",
                vec!["read".into()],
                Some("go.example".into()),
                None,
            )
            .await
            .unwrap();
        let listed = auth.list_keys().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, issued.key.id);
        assert_eq!(listed[0].name, "reader");
        let access = auth.key(&issued.token).await.unwrap();
        assert!(access.allows("read"));
        assert!(!access.allows("write"));
        assert_eq!(access.host.as_deref(), Some("go.example"));
        let (id, _) = issued.token.split_once('.').unwrap();
        assert!(auth.key(&format!("{id}.{}", "0".repeat(64))).await.is_err());
        sqlx::query("UPDATE admin_api_keys SET expires_at = unixepoch()")
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(auth.key(&issued.token).await.is_err());
        let active = auth
            .create_key("writer", vec!["write".into()], None, None)
            .await
            .unwrap();
        assert!(auth.key(&active.token).await.is_ok());
        auth.revoke_key(active.key.id).await.unwrap();
        assert!(auth.key(&active.token).await.is_err());
    }

    #[tokio::test]
    async fn key_rotation_replaces_only_the_selected_active_key() {
        let (_directory, auth) = database().await;
        let session = account(&auth).await;
        let expiry: i64 = sqlx::query_scalar("SELECT unixepoch() + 3600")
            .fetch_one(&auth.pool)
            .await
            .unwrap();
        let old = auth
            .create_key("reader", vec!["read".into()], Some("go.example".into()), Some(expiry))
            .await
            .unwrap();
        assert!(matches!(
            auth.rotate_key(old.key.id, &session.token, NEW_PASSWORD).await,
            Err(AuthError::Unauthorized)
        ));
        assert!(auth.key(&old.token).await.is_ok());
        let (first, second) = tokio::join!(
            auth.rotate_key(old.key.id, &session.token, PASSWORD),
            auth.rotate_key(old.key.id, &session.token, PASSWORD),
        );
        assert_ne!(first.is_ok(), second.is_ok());
        let rotated = first.ok().or_else(|| second.ok()).unwrap();
        assert_ne!(rotated.key.id, old.key.id);
        assert_eq!(rotated.key.name, old.key.name);
        assert_eq!(rotated.key.scopes, old.key.scopes);
        assert_eq!(rotated.key.host, old.key.host);
        assert_eq!(rotated.key.expires_at, old.key.expires_at);
        assert!(auth.key(&old.token).await.is_err());
        assert!(auth.key(&rotated.token).await.is_ok());
        assert_eq!(auth.list_keys().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn key_rotation_rolls_back_when_audit_insertion_fails() {
        let (_directory, auth) = database().await;
        let session = account(&auth).await;
        let old = auth
            .create_key("reader", vec!["read".into()], None, None)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TRIGGER fail_rotation_audit BEFORE INSERT ON admin_audit
             WHEN NEW.action = 'key_rotate' BEGIN SELECT RAISE(ABORT, 'test audit failure'); END",
        )
        .execute(&auth.pool)
        .await
        .unwrap();
        assert!(matches!(
            auth.rotate_key(old.key.id, &session.token, PASSWORD).await,
            Err(AuthError::Unavailable)
        ));
        assert!(auth.key(&old.token).await.is_ok());
        assert_eq!(auth.list_keys().await.unwrap().len(), 1);
    }
}
