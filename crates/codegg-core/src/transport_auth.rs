//! Transport authentication and principal binding (M002).
//!
//! M001 owns the durable principal/membership domain in [`crate::team`].
//! This module owns the M002 transport seam: resolving transport evidence to
//! a canonical [`AuthenticatedPrincipal`], carrying that immutable principal
//! through client/request context, and managing personal-token lifecycle.
//!
//! ## Trust model
//!
//! - The daemon transport constructs the principal. Request payloads are
//!   locators and MUST NOT name a principal, role, or capability.
//! - Personal-local transports (Unix socket, stdio, in-process) resolve the
//!   deterministic [`crate::team::LOCAL_OWNER_PRINCIPAL_ID`] record without a
//!   login ceremony. The socket file lives in the user-scoped runtime
//!   directory; per-connection `SO_PEERCRED` UID validation is future
//!   hardening and is explicitly not claimed here.
//! - Network transports (HTTP/WebSocket) fail closed. A presented personal
//!   token must verify before any principal is bound. The legacy global
//!   bearer remains only as a bootstrap/compatibility credential that maps to
//!   `LocalOwner`; it MUST NOT masquerade as distinct team identities.
//! - Token records store a SHA-256 digest, owner, expiry, and revocation —
//!   never plaintext. Verification is constant-time over the digest.
//! - [`AuthenticatedPrincipal`] is immutable for a connection/auth session.
//!   [`RequestAuthorityContext`] carries it plus a correlation id; neither
//!   value is ever derived from a request payload field.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;

use crate::error::StorageError;
use crate::identity::{PrincipalId, ProjectId};
use crate::team::{PrincipalKind, PrincipalRecord, TeamError, TeamStore, LOCAL_OWNER_PRINCIPAL_ID};

/// Prefix for presented personal tokens. The full plaintext has the shape
/// `cggt_<token_id>.<secret>` where `secret` is 32 random bytes in
/// URL-safe base64 without padding.
pub const PERSONAL_TOKEN_PREFIX: &str = "cggt_";

/// Maximum label length for a personal token record.
pub const MAX_TOKEN_LABEL_LENGTH: usize = 200;

/// How a principal was authenticated. Recorded on the bound principal so
/// diagnostics can distinguish zero-login local ownership from verified
/// network credentials and from the legacy bootstrap seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    /// Trusted local transport (Unix socket, stdio, in-process). No login.
    LocalOwner,
    /// Verified personal token on a network transport.
    PersonalToken,
    /// Legacy global bearer (`CODEGG_SERVER_TOKEN` / `server.token`).
    /// Compatibility/bootstrap only; always maps to `LocalOwner` and MUST
    /// NOT be presented as a distinct team identity.
    BootstrapGlobalBearer,
    /// Harness-internal test transport. Never used by production daemons.
    InternalTest,
}

impl AuthMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalOwner => "local_owner",
            Self::PersonalToken => "personal_token",
            Self::BootstrapGlobalBearer => "bootstrap_global_bearer",
            Self::InternalTest => "internal_test",
        }
    }

    fn parse(value: &str) -> Result<Self, TransportAuthError> {
        match value {
            "local_owner" => Ok(Self::LocalOwner),
            "personal_token" => Ok(Self::PersonalToken),
            "bootstrap_global_bearer" => Ok(Self::BootstrapGlobalBearer),
            "internal_test" => Ok(Self::InternalTest),
            _ => Err(TransportAuthError::Invalid(format!(
                "unknown auth method {value:?}"
            ))),
        }
    }

    /// Lenient parse for audit readers; unknown values degrade to `None`.
    pub fn parse_for_audit(value: &str) -> Option<Self> {
        Self::parse(value).ok()
    }
}

/// Canonical transport class for a bound principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportClass {
    Local,
    AuthenticatedRemote,
    InternalTest,
}

impl TransportClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::AuthenticatedRemote => "authenticated_remote",
            Self::InternalTest => "internal_test",
        }
    }

    fn parse(value: &str) -> Result<Self, TransportAuthError> {
        match value {
            "local" => Ok(Self::Local),
            "authenticated_remote" => Ok(Self::AuthenticatedRemote),
            "internal_test" => Ok(Self::InternalTest),
            _ => Err(TransportAuthError::Invalid(format!(
                "unknown transport class {value:?}"
            ))),
        }
    }

    /// Lenient parse for audit readers; unknown values degrade to `None`.
    pub fn parse_for_audit(value: &str) -> Option<Self> {
        Self::parse(value).ok()
    }
}

/// Canonical authenticated principal bound by the daemon transport.
///
/// Constructed once per connection/auth session from trusted transport
/// evidence. All fields are private; there are no setters, so the binding
/// is immutable after construction. Contains no secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedPrincipal {
    principal_id: PrincipalId,
    kind: PrincipalKind,
    auth_method: AuthMethod,
    transport_class: TransportClass,
    /// Daemon-issued connection identity that owns this binding.
    client_id: String,
    authenticated_at_ms: i64,
}

impl AuthenticatedPrincipal {
    fn new(
        principal_id: PrincipalId,
        kind: PrincipalKind,
        auth_method: AuthMethod,
        transport_class: TransportClass,
        client_id: String,
    ) -> Self {
        Self {
            principal_id,
            kind,
            auth_method,
            transport_class,
            client_id,
            authenticated_at_ms: now_millis(),
        }
    }

    /// Zero-login local-owner binding for trusted local transports.
    pub fn local_owner(client_id: impl Into<String>) -> Self {
        let id = PrincipalId::parse(LOCAL_OWNER_PRINCIPAL_ID)
            .expect("local-owner satisfies the identity lexical contract");
        Self::new(
            id,
            PrincipalKind::LocalOwner,
            AuthMethod::LocalOwner,
            TransportClass::Local,
            client_id.into(),
        )
    }

    /// Binding for a verified personal token on a network transport.
    pub fn personal_token(principal: &PrincipalRecord, client_id: impl Into<String>) -> Self {
        Self::new(
            principal.id.clone(),
            principal.kind,
            AuthMethod::PersonalToken,
            TransportClass::AuthenticatedRemote,
            client_id.into(),
        )
    }

    /// Compatibility binding for the legacy global bearer. Always maps to
    /// `LocalOwner`; the caller MUST NOT synthesize a distinct identity.
    pub fn bootstrap_global_bearer(client_id: impl Into<String>) -> Self {
        let id = PrincipalId::parse(LOCAL_OWNER_PRINCIPAL_ID)
            .expect("local-owner satisfies the identity lexical contract");
        Self::new(
            id,
            PrincipalKind::LocalOwner,
            AuthMethod::BootstrapGlobalBearer,
            TransportClass::AuthenticatedRemote,
            client_id.into(),
        )
    }

    /// Harness-internal binding. Never produced by production transports.
    pub fn internal_test(principal: &PrincipalRecord, client_id: impl Into<String>) -> Self {
        Self::new(
            principal.id.clone(),
            principal.kind,
            AuthMethod::InternalTest,
            TransportClass::InternalTest,
            client_id.into(),
        )
    }

    pub fn principal_id(&self) -> &PrincipalId {
        &self.principal_id
    }

    pub fn kind(&self) -> PrincipalKind {
        self.kind
    }

    pub fn auth_method(&self) -> AuthMethod {
        self.auth_method
    }

    pub fn transport_class(&self) -> TransportClass {
        self.transport_class
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Rebind the same authentication to a concrete connection identity.
    /// The principal, kind, method, and transport class are preserved; only
    /// the owning `client_id` changes. Used when validation runs before the
    /// upgrade task mints its connection id.
    pub fn for_connection(&self, client_id: impl Into<String>) -> Self {
        Self {
            principal_id: self.principal_id.clone(),
            kind: self.kind,
            auth_method: self.auth_method,
            transport_class: self.transport_class,
            client_id: client_id.into(),
            authenticated_at_ms: self.authenticated_at_ms,
        }
    }

    pub fn authenticated_at_ms(&self) -> i64 {
        self.authenticated_at_ms
    }

    /// `true` for the compatibility bootstrap seam. Diagnostics and audit
    /// MUST treat this as `LocalOwner`, never as a distinct team identity.
    pub fn is_bootstrap_compatibility(&self) -> bool {
        self.auth_method == AuthMethod::BootstrapGlobalBearer
    }

    /// Projection-layer principal string for this binding. `LocalOwner`
    /// maps to the `"local-user"` compatibility value via the M001 adapter;
    /// every other principal maps to its canonical id. The legacy
    /// `"authenticated-remote"` synthetic is never produced here.
    pub fn projection_principal_string(&self) -> String {
        if self.kind == PrincipalKind::LocalOwner {
            return "local-user".to_owned();
        }
        self.principal_id.as_str().to_owned()
    }

    /// Build the canonical projection access context for this binding.
    ///
    /// The principal string comes from [`Self::projection_principal_string`]
    /// (transport-derived), never from a request payload. Remote bindings
    /// use a bounded project resolver supplied by the caller; local bindings
    /// use the allow-all resolver for the single-user daemon.
    pub fn to_projection_access_context(
        &self,
        correlation_id: impl Into<String>,
        capabilities: crate::projection_replay::context::ProjectionCapabilitySet,
        resolver: std::sync::Arc<dyn crate::projection_replay::context::ProjectionProjectResolver>,
    ) -> crate::projection_replay::context::ProjectionAccessContext {
        let transport_class = match self.transport_class {
            TransportClass::Local => {
                crate::projection_replay::context::ProjectionTransportClass::Local
            }
            TransportClass::AuthenticatedRemote => {
                crate::projection_replay::context::ProjectionTransportClass::AuthenticatedRemote
            }
            TransportClass::InternalTest => {
                crate::projection_replay::context::ProjectionTransportClass::InternalTest
            }
        };
        crate::projection_replay::context::ProjectionAccessContext::from_canonical_principal(
            self.projection_principal_string(),
            self.client_id.clone(),
            correlation_id.into(),
            capabilities,
            resolver,
            transport_class,
        )
    }
}

/// Immutable request authority context for one transport request.
///
/// The daemon transport constructs this once per request from the
/// connection-bound [`AuthenticatedPrincipal`]. Payload DTOs supply locators
/// only; they never contribute to this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestAuthorityContext {
    principal: AuthenticatedPrincipal,
    correlation_id: String,
}

impl RequestAuthorityContext {
    pub fn new(principal: AuthenticatedPrincipal, correlation_id: impl Into<String>) -> Self {
        Self {
            principal,
            correlation_id: correlation_id.into(),
        }
    }

    /// Local-owner authority for trusted local transports (socket/stdio/inproc).
    pub fn local(client_id: &str, correlation_id: impl Into<String>) -> Self {
        Self::new(
            AuthenticatedPrincipal::local_owner(client_id),
            correlation_id,
        )
    }

    pub fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }

    pub fn principal_id(&self) -> &PrincipalId {
        self.principal.principal_id()
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }
}

/// Durable personal-token record. Stores a SHA-256 digest, owner, expiry,
/// and revocation — never plaintext. The custom [`std::fmt::Debug`] impl
/// omits the digest so digests never enter logs/events.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalTokenRecord {
    pub token_id: String,
    pub principal_id: PrincipalId,
    pub token_digest_hex: String,
    /// Non-secret lookup prefix (first 8 chars of the token id) for
    /// diagnostics and listing. Never sufficient to authenticate.
    pub token_prefix: String,
    pub label: String,
    pub auth_method: AuthMethod,
    pub transport_class: TransportClass,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

impl std::fmt::Debug for PersonalTokenRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersonalTokenRecord")
            .field("token_id", &self.token_id)
            .field("principal_id", &self.principal_id)
            .field("token_prefix", &self.token_prefix)
            .field("label", &self.label)
            .field("auth_method", &self.auth_method)
            .field("transport_class", &self.transport_class)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .field("revoked_at", &self.revoked_at)
            .finish_non_exhaustive()
    }
}

impl PersonalTokenRecord {
    /// `true` when the token currently grants authentication.
    pub fn is_live(&self, now_ms: i64) -> bool {
        if self.revoked_at.is_some() {
            return false;
        }
        if let Some(expires) = self.expires_at {
            if now_ms >= expires {
                return false;
            }
        }
        true
    }
}

/// Errors for transport authentication and token lifecycle.
#[derive(Debug, Error)]
pub enum TransportAuthError {
    #[error("invalid transport auth value: {0}")]
    Invalid(String),
    #[error("unknown personal token")]
    UnknownToken,
    #[error("personal token is revoked")]
    Revoked,
    #[error("personal token is expired")]
    Expired,
    #[error("personal token principal is not active")]
    PrincipalNotActive,
    #[error("personal token verification failed")]
    VerificationFailed,
    #[error("team error: {0}")]
    Team(#[from] TeamError),
    #[error("transport auth storage error: {0}")]
    Storage(#[from] StorageError),
}

/// Daemon-owned personal-token lifecycle service.
///
/// Backed by the same SQLite pool as [`TeamStore`]. All writes are
/// transactional and restart-safe; revocation is idempotent and monotonic
/// (a revoked token can never become live again).
#[derive(Clone)]
pub struct PersonalTokenStore {
    pool: SqlitePool,
    team: TeamStore,
}

impl PersonalTokenStore {
    pub fn new(pool: SqlitePool) -> Self {
        let team = TeamStore::new(pool.clone());
        Self { pool, team }
    }

    pub fn with_team(pool: SqlitePool, team: TeamStore) -> Self {
        Self { pool, team }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Create a personal token for an existing active principal.
    ///
    /// Returns the one-time plaintext (never persisted) plus the durable
    /// record. `expires_at_ms` is an absolute millisecond timestamp or
    /// `None` for no expiry. The plaintext is returned once; only the
    /// digest is stored.
    pub async fn create_personal_token(
        &self,
        principal_id: &PrincipalId,
        label: &str,
        expires_at_ms: Option<i64>,
    ) -> Result<(String, PersonalTokenRecord), TransportAuthError> {
        let label = validate_label(label)?;
        let principal = self
            .team
            .get_principal(principal_id)
            .await?
            .ok_or(TransportAuthError::PrincipalNotActive)?;
        if principal.status != crate::team::PrincipalStatus::Active {
            return Err(TransportAuthError::PrincipalNotActive);
        }
        let token_id = uuid::Uuid::new_v4().to_string();
        let mut secret_bytes = [0u8; 32];
        {
            use rand::RngCore as _;
            rand::rng().fill_bytes(&mut secret_bytes);
        }
        let secret = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            secret_bytes,
        );
        let plaintext = format!("{PERSONAL_TOKEN_PREFIX}{token_id}.{secret}");
        let digest_hex = token_digest_hex(&secret);
        let prefix = token_id.chars().take(8).collect::<String>();
        let now = now_millis();
        sqlx::query(
            "INSERT INTO personal_auth_token (token_id, principal_id, token_digest, \
             token_prefix, label, auth_method, transport_class, time_created, expires_at, \
             revoked_at) VALUES (?, ?, ?, ?, ?, 'personal_token', 'authenticated_remote', ?, ?, NULL)",
        )
        .bind(&token_id)
        .bind(principal_id.as_str())
        .bind(&digest_hex)
        .bind(&prefix)
        .bind(&label)
        .bind(now)
        .bind(expires_at_ms)
        .execute(&self.pool)
        .await
        .map_err(|e| TransportAuthError::Storage(StorageError::Database(e.to_string())))?;
        let record = self
            .get_token_record(&token_id)
            .await?
            .ok_or(TransportAuthError::UnknownToken)?;
        Ok((plaintext, record))
    }

    /// Revoke a personal token. Idempotent: revoking an already-revoked
    /// token returns the current record without error.
    pub async fn revoke_personal_token(
        &self,
        token_id: &str,
    ) -> Result<PersonalTokenRecord, TransportAuthError> {
        let now = now_millis();
        sqlx::query(
            "UPDATE personal_auth_token SET revoked_at = COALESCE(revoked_at, ?) WHERE token_id = ?",
        )
        .bind(now)
        .bind(token_id)
        .execute(&self.pool)
        .await
        .map_err(|e| TransportAuthError::Storage(StorageError::Database(e.to_string())))?;
        self.get_token_record(token_id)
            .await?
            .ok_or(TransportAuthError::UnknownToken)
    }

    /// Verify a presented plaintext token against the digest store.
    ///
    /// Fails closed on unknown, revoked, expired, or disabled-principal
    /// tokens. Digest comparison is constant-time. Existing connections are
    /// unaffected: revocation fails only *new* authentication, and the
    /// caller must define the bounded disconnect/re-auth contract.
    pub async fn verify_personal_token(
        &self,
        presented: &str,
    ) -> Result<AuthenticatedPrincipal, TransportAuthError> {
        let (token_id, secret) = split_presented_token(presented)?;
        let record = self
            .get_token_record(&token_id)
            .await?
            .ok_or(TransportAuthError::UnknownToken)?;
        if record.revoked_at.is_some() {
            return Err(TransportAuthError::Revoked);
        }
        if let Some(expires) = record.expires_at {
            if now_millis() >= expires {
                return Err(TransportAuthError::Expired);
            }
        }
        // Constant-time digest comparison; length mismatch also fails.
        let presented_digest = token_digest_hex(&secret);
        if !constant_time_eq_hex(&presented_digest, &record.token_digest_hex) {
            return Err(TransportAuthError::VerificationFailed);
        }
        let principal = self
            .team
            .get_principal(&record.principal_id)
            .await?
            .ok_or(TransportAuthError::PrincipalNotActive)?;
        if principal.status != crate::team::PrincipalStatus::Active {
            return Err(TransportAuthError::PrincipalNotActive);
        }
        Ok(AuthenticatedPrincipal::personal_token(
            &principal,
            format!("token-{}", record.token_prefix),
        ))
    }

    /// Verify a presented token and bind it to one connection identity.
    /// The returned principal carries `client_id` so the binding is
    /// connection-scoped and immutable.
    pub async fn verify_for_client(
        &self,
        presented: &str,
        client_id: &str,
    ) -> Result<AuthenticatedPrincipal, TransportAuthError> {
        let (token_id, secret) = split_presented_token(presented)?;
        let record = self
            .get_token_record(&token_id)
            .await?
            .ok_or(TransportAuthError::UnknownToken)?;
        if record.revoked_at.is_some() {
            return Err(TransportAuthError::Revoked);
        }
        if let Some(expires) = record.expires_at {
            if now_millis() >= expires {
                return Err(TransportAuthError::Expired);
            }
        }
        let presented_digest = token_digest_hex(&secret);
        if !constant_time_eq_hex(&presented_digest, &record.token_digest_hex) {
            return Err(TransportAuthError::VerificationFailed);
        }
        let principal = self
            .team
            .get_principal(&record.principal_id)
            .await?
            .ok_or(TransportAuthError::PrincipalNotActive)?;
        if principal.status != crate::team::PrincipalStatus::Active {
            return Err(TransportAuthError::PrincipalNotActive);
        }
        Ok(AuthenticatedPrincipal::personal_token(
            &principal, client_id,
        ))
    }

    pub async fn get_token_record(
        &self,
        token_id: &str,
    ) -> Result<Option<PersonalTokenRecord>, TransportAuthError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
                Option<i64>,
                Option<i64>,
            ),
        >(
            "SELECT token_id, principal_id, token_digest, token_prefix, label, auth_method, \
             transport_class, time_created, expires_at, revoked_at FROM personal_auth_token \
             WHERE token_id = ?",
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TransportAuthError::Storage(StorageError::Database(e.to_string())))?;
        row.map(token_row).transpose()
    }

    pub async fn list_tokens_for_principal(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<Vec<PersonalTokenRecord>, TransportAuthError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                String,
                String,
                String,
                String,
                i64,
                Option<i64>,
                Option<i64>,
            ),
        >(
            "SELECT token_id, principal_id, token_digest, token_prefix, label, auth_method, \
             transport_class, time_created, expires_at, revoked_at FROM personal_auth_token \
             WHERE principal_id = ? ORDER BY time_created, token_id",
        )
        .bind(principal_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| TransportAuthError::Storage(StorageError::Database(e.to_string())))?;
        rows.into_iter().map(token_row).collect()
    }
}

/// Resolve the trusted-local principal for Unix-socket, stdio, and
/// in-process transports. Ensures the deterministic `LocalOwner` record
/// exists, then binds it to `client_id` with no login ceremony.
pub async fn bind_local_owner(
    team: &TeamStore,
    client_id: &str,
) -> Result<AuthenticatedPrincipal, TransportAuthError> {
    let record = team.ensure_local_owner().await?;
    debug_assert_eq!(record.id.as_str(), LOCAL_OWNER_PRINCIPAL_ID);
    Ok(AuthenticatedPrincipal::personal_token_local_owner(
        client_id,
    ))
}

impl AuthenticatedPrincipal {
    fn personal_token_local_owner(client_id: &str) -> Self {
        Self::local_owner(client_id)
    }
}

/// Canonical disposition for the legacy global bearer.
///
/// Returns `true` when `presented` starts with the personal-token prefix and
/// therefore MUST be verified as a personal token rather than compared
/// against the global bearer. This prevents a personal token from ever
/// matching the global-bearer path and vice versa.
pub fn is_personal_token_presentation(presented: &str) -> bool {
    presented.starts_with(PERSONAL_TOKEN_PREFIX)
}

/// Constant-time comparison of two hex digests. Returns `false` on length
/// mismatch without early content disclosure beyond length.
fn constant_time_eq_hex(left: &str, right: &str) -> bool {
    use subtle::ConstantTimeEq as _;
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    left.ct_eq(right).unwrap_u8() == 1
}

/// SHA-256 hex digest of a token secret. The secret is high-entropy; the
/// digest is safe to persist but is still omitted from `Debug` output.
fn token_digest_hex(secret: &str) -> String {
    use sha2::{Digest as _, Sha256};
    format!("{:x}", Sha256::digest(secret.as_bytes()))
}

fn split_presented_token(presented: &str) -> Result<(String, String), TransportAuthError> {
    let rest = presented
        .strip_prefix(PERSONAL_TOKEN_PREFIX)
        .ok_or(TransportAuthError::UnknownToken)?;
    let (token_id, secret) = rest
        .split_once('.')
        .ok_or(TransportAuthError::UnknownToken)?;
    if token_id.is_empty() || secret.is_empty() {
        return Err(TransportAuthError::UnknownToken);
    }
    if token_id.len() > 128 || secret.len() > 256 {
        return Err(TransportAuthError::UnknownToken);
    }
    Ok((token_id.to_owned(), secret.to_owned()))
}

/// Row shape for `personal_auth_token` reads.
type PersonalTokenRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    Option<i64>,
    Option<i64>,
);

fn validate_label(value: &str) -> Result<String, TransportAuthError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || value.len() > MAX_TOKEN_LABEL_LENGTH {
        return Err(TransportAuthError::Invalid(
            "token label must be non-empty and bounded".to_owned(),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(TransportAuthError::Invalid(
            "token label must not contain control characters".to_owned(),
        ));
    }
    Ok(trimmed.to_owned())
}

fn token_row(row: PersonalTokenRow) -> Result<PersonalTokenRecord, TransportAuthError> {
    let (
        token_id,
        principal_id,
        token_digest_hex,
        token_prefix,
        label,
        auth_method,
        transport_class,
        created_at,
        expires_at,
        revoked_at,
    ) = row;
    let principal_id = PrincipalId::parse(&principal_id)
        .map_err(|e| TransportAuthError::Team(TeamError::from(e)))?;
    let auth_method = AuthMethod::parse(&auth_method)?;
    let transport_class = TransportClass::parse(&transport_class)?;
    validate_label(&label)?;
    Ok(PersonalTokenRecord {
        token_id,
        principal_id,
        token_digest_hex,
        token_prefix,
        label,
        auth_method,
        transport_class,
        created_at,
        expires_at,
        revoked_at,
    })
}

/// Redact a presented bearer/personal token for diagnostics. Returns a
/// bounded, non-secret placeholder that preserves only the token kind.
pub fn redact_presented_token(presented: &str) -> String {
    if is_personal_token_presentation(presented) {
        return "[REDACTED:personal-token]".to_owned();
    }
    if presented.is_empty() {
        return "[REDACTED:empty-credential]".to_owned();
    }
    "[REDACTED:bearer-token]".to_owned()
}

/// `true` when `principal_id` belongs to `project_id`'s membership scope is
/// intentionally NOT decided here. Authentication (who) stays separate from
/// M003 authorization (may do what); this module never evaluates project
/// capabilities. The helper exists so reviewers can grep for the seam.
pub fn authentication_is_not_authorization(_project: &ProjectId, _principal: &PrincipalId) -> bool {
    false
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::{PrincipalKind, PrincipalStatus};

    async fn test_stores() -> (TeamStore, PersonalTokenStore) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate test store");
        let team = TeamStore::new(pool.clone());
        let tokens = PersonalTokenStore::with_team(pool, team.clone());
        (team, tokens)
    }

    #[test]
    fn authenticated_principal_is_immutable_and_secret_free() {
        let principal = AuthenticatedPrincipal::local_owner("client-1");
        assert_eq!(principal.principal_id().as_str(), LOCAL_OWNER_PRINCIPAL_ID);
        assert_eq!(principal.kind(), PrincipalKind::LocalOwner);
        assert_eq!(principal.auth_method(), AuthMethod::LocalOwner);
        assert_eq!(principal.transport_class(), TransportClass::Local);
        assert_eq!(principal.client_id(), "client-1");
        assert!(!principal.is_bootstrap_compatibility());
        let json = serde_json::to_string(&principal).expect("serialize principal");
        for forbidden in [
            "secret",
            "token",
            "password",
            "bearer",
            "credential",
            "digest",
        ] {
            assert!(
                !json.to_ascii_lowercase().contains(forbidden),
                "principal JSON must not contain {forbidden:?}: {json}"
            );
        }
        let bootstrap = AuthenticatedPrincipal::bootstrap_global_bearer("client-9");
        assert_eq!(bootstrap.principal_id().as_str(), LOCAL_OWNER_PRINCIPAL_ID);
        assert!(bootstrap.is_bootstrap_compatibility());
    }

    #[test]
    fn local_owner_projection_string_uses_compatibility_local_user() {
        let local = AuthenticatedPrincipal::local_owner("c1");
        assert_eq!(local.projection_principal_string(), "local-user");
    }

    #[test]
    fn request_authority_context_carries_transport_identity() {
        let principal = AuthenticatedPrincipal::local_owner("client-7");
        let ctx = RequestAuthorityContext::new(principal.clone(), "corr-1");
        assert_eq!(ctx.principal(), &principal);
        assert_eq!(ctx.principal_id().as_str(), LOCAL_OWNER_PRINCIPAL_ID);
        assert_eq!(ctx.correlation_id(), "corr-1");
        let local = RequestAuthorityContext::local("client-7", "corr-2");
        assert_eq!(local.principal_id().as_str(), LOCAL_OWNER_PRINCIPAL_ID);
    }

    #[test]
    fn redact_presented_token_never_echoes_secret() {
        let redacted = redact_presented_token("cggt_token-id.secret-value");
        assert_eq!(redacted, "[REDACTED:personal-token]");
        assert!(!redacted.contains("secret-value"));
        let bearer = redact_presented_token("some-global-bearer-value");
        assert_eq!(bearer, "[REDACTED:bearer-token]");
        assert!(!bearer.contains("some-global"));
    }

    #[test]
    fn token_record_debug_omits_digest() {
        let record = PersonalTokenRecord {
            token_id: "token-id-fixture".to_owned(),
            principal_id: PrincipalId::parse("principal-fixture").unwrap(),
            token_digest_hex: "digest-should-never-appear-in-logs".to_owned(),
            token_prefix: "token-id".to_owned(),
            label: "test".to_owned(),
            auth_method: AuthMethod::PersonalToken,
            transport_class: TransportClass::AuthenticatedRemote,
            created_at: 1,
            expires_at: None,
            revoked_at: None,
        };
        let debug = format!("{record:?}");
        assert!(!debug.contains("digest-should-never-appear-in-logs"));
        assert!(debug.contains("token-id-fixture"));
        let json = serde_json::to_string(&record).expect("serialize record");
        // Records persist digests by design, but principal payloads elsewhere
        // must stay secret-free; the digest field name is explicit storage.
        assert!(json.contains("token_digest_hex"));
    }

    #[test]
    fn is_personal_token_presentation_routes_correctly() {
        assert!(is_personal_token_presentation("cggt_abc.def"));
        assert!(!is_personal_token_presentation("some-global-bearer"));
        assert!(!is_personal_token_presentation(""));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_owner_binding_needs_no_login() {
        let (team, _) = test_stores().await;
        let bound = bind_local_owner(&team, "local-client-1").await.unwrap();
        assert_eq!(bound.principal_id().as_str(), LOCAL_OWNER_PRINCIPAL_ID);
        assert_eq!(bound.auth_method(), AuthMethod::LocalOwner);
        assert_eq!(bound.transport_class(), TransportClass::Local);
        // Idempotent across calls.
        let again = bind_local_owner(&team, "local-client-2").await.unwrap();
        assert_eq!(again.principal_id(), bound.principal_id());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn personal_token_create_verify_round_trip() {
        let (team, tokens) = test_stores().await;
        let principal = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let (plaintext, record) = tokens
            .create_personal_token(&principal.id, "laptop", None)
            .await
            .unwrap();
        assert!(plaintext.starts_with(PERSONAL_TOKEN_PREFIX));
        assert!(!record.token_digest_hex.is_empty());
        assert!(!plaintext.contains(&record.token_digest_hex));
        let bound = tokens
            .verify_for_client(&plaintext, "client-a")
            .await
            .unwrap();
        assert_eq!(bound.principal_id(), &principal.id);
        assert_eq!(bound.auth_method(), AuthMethod::PersonalToken);
        assert_eq!(bound.client_id(), "client-a");
        assert_eq!(bound.projection_principal_string(), principal.id.as_str());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn personal_token_wrong_secret_fails_closed_timing_safe() {
        let (team, tokens) = test_stores().await;
        let principal = team
            .create_principal(PrincipalKind::Human, "Grace")
            .await
            .unwrap();
        let (plaintext, _) = tokens
            .create_personal_token(&principal.id, "laptop", None)
            .await
            .unwrap();
        let mut tampered = plaintext.clone();
        tampered.push('x');
        assert!(tokens.verify_personal_token(&tampered).await.is_err());
        assert!(tokens
            .verify_personal_token("cggt_unknown.secret")
            .await
            .is_err());
        assert!(tokens.verify_personal_token("not-a-token").await.is_err());
        assert!(tokens.verify_personal_token("").await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn personal_token_revoke_fails_new_authentication() {
        let (team, tokens) = test_stores().await;
        let principal = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let (plaintext, record) = tokens
            .create_personal_token(&principal.id, "laptop", None)
            .await
            .unwrap();
        assert!(tokens.verify_personal_token(&plaintext).await.is_ok());
        let revoked = tokens
            .revoke_personal_token(&record.token_id)
            .await
            .unwrap();
        assert!(revoked.revoked_at.is_some());
        assert!(matches!(
            tokens.verify_personal_token(&plaintext).await,
            Err(TransportAuthError::Revoked)
        ));
        // Idempotent second revoke.
        let again = tokens
            .revoke_personal_token(&record.token_id)
            .await
            .unwrap();
        assert_eq!(again.revoked_at, revoked.revoked_at);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn personal_token_expiry_fails_closed() {
        let (team, tokens) = test_stores().await;
        let principal = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let past = now_millis() - 1_000;
        let (plaintext, _) = tokens
            .create_personal_token(&principal.id, "short-lived", Some(past))
            .await
            .unwrap();
        assert!(matches!(
            tokens.verify_personal_token(&plaintext).await,
            Err(TransportAuthError::Expired)
        ));
        let future = now_millis() + 3_600_000;
        let (live_plaintext, _) = tokens
            .create_personal_token(&principal.id, "long-lived", Some(future))
            .await
            .unwrap();
        assert!(tokens.verify_personal_token(&live_plaintext).await.is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn distinct_clients_bind_distinct_principals() {
        let (team, tokens) = test_stores().await;
        let alice = team
            .create_principal(PrincipalKind::Human, "Alice")
            .await
            .unwrap();
        let bob = team
            .create_principal(PrincipalKind::Human, "Bob")
            .await
            .unwrap();
        let (alice_token, _) = tokens
            .create_personal_token(&alice.id, "alice-laptop", None)
            .await
            .unwrap();
        let (bob_token, _) = tokens
            .create_personal_token(&bob.id, "bob-laptop", None)
            .await
            .unwrap();
        let alice_bound = tokens
            .verify_for_client(&alice_token, "client-alice")
            .await
            .unwrap();
        let bob_bound = tokens
            .verify_for_client(&bob_token, "client-bob")
            .await
            .unwrap();
        assert_eq!(alice_bound.principal_id(), &alice.id);
        assert_eq!(bob_bound.principal_id(), &bob.id);
        assert_ne!(alice_bound.principal_id(), bob_bound.principal_id());
        assert_ne!(
            alice_bound.projection_principal_string(),
            bob_bound.projection_principal_string()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_principal_cannot_authenticate() {
        let (team, tokens) = test_stores().await;
        let principal = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let (plaintext, _) = tokens
            .create_personal_token(&principal.id, "laptop", None)
            .await
            .unwrap();
        team.set_principal_status(
            &principal.id,
            principal.revision,
            crate::team::PrincipalStatus::Disabled,
        )
        .await
        .unwrap();
        assert!(matches!(
            tokens.verify_personal_token(&plaintext).await,
            Err(TransportAuthError::PrincipalNotActive)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn projection_context_uses_bound_canonical_principal() {
        use crate::projection_replay::context::{AllowAllProjectResolver, ProjectionCapabilitySet};
        let (team, tokens) = test_stores().await;
        let principal = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let (plaintext, _) = tokens
            .create_personal_token(&principal.id, "laptop", None)
            .await
            .unwrap();
        let bound = tokens
            .verify_for_client(&plaintext, "client-a")
            .await
            .unwrap();
        let ctx = bound.to_projection_access_context(
            "corr-1",
            ProjectionCapabilitySet::local_user(),
            std::sync::Arc::new(AllowAllProjectResolver),
        );
        assert_eq!(ctx.principal_id.as_str(), principal.id.as_str());
        assert_ne!(ctx.principal_id.as_str(), "authenticated-remote");
        assert!(matches!(
            ctx.transport_class,
            crate::projection_replay::context::ProjectionTransportClass::AuthenticatedRemote
        ));
        let local = AuthenticatedPrincipal::local_owner("local-client");
        let local_ctx = local.to_projection_access_context(
            "corr-2",
            ProjectionCapabilitySet::local_user(),
            std::sync::Arc::new(AllowAllProjectResolver),
        );
        assert_eq!(local_ctx.principal_id.as_str(), "local-user");
        let _ = PrincipalStatus::Active;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn personal_token_lifecycle_survives_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("m002-restart.db");
        let url = format!("sqlite:{}?mode=rwc", path.display());
        let pool = SqlitePool::connect(&url).await.expect("connect file db");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate file db");
        let team = TeamStore::new(pool.clone());
        let tokens = PersonalTokenStore::with_team(pool.clone(), team.clone());
        let principal = team
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let (plaintext, record) = tokens
            .create_personal_token(&principal.id, "laptop", None)
            .await
            .unwrap();
        // Close and reopen: lifecycle must be restart-safe.
        pool.close().await;
        let pool2 = SqlitePool::connect(&url).await.expect("reconnect file db");
        crate::session::schema::migrate(&pool2)
            .await
            .expect("remigrate file db");
        let team2 = TeamStore::new(pool2.clone());
        let tokens2 = PersonalTokenStore::with_team(pool2.clone(), team2);
        assert!(tokens2.verify_personal_token(&plaintext).await.is_ok());
        tokens2
            .revoke_personal_token(&record.token_id)
            .await
            .unwrap();
        pool2.close().await;
        let pool3 = SqlitePool::connect(&url).await.expect("reconnect file db");
        crate::session::schema::migrate(&pool3)
            .await
            .expect("remigrate file db");
        let tokens3 = PersonalTokenStore::new(pool3.clone());
        assert!(matches!(
            tokens3.verify_personal_token(&plaintext).await,
            Err(TransportAuthError::Revoked)
        ));
        pool3.close().await;
    }
}
