//! Secret-safe, daemon-owned provider connection metadata and persistence.
//!
//! A [`ProviderConnection`] never contains a credential. It carries an opaque
//! [`SecretRef`] and a [`SecretBindingLocator`] that a daemon manager can use
//! to ask the existing credential subsystem for a secret at resolution time.
//! Endpoint construction is purely syntactic and never performs network I/O.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use thiserror::Error;

use crate::error::StorageError;
use crate::identity::{IdentityParseError, PrincipalId, ProjectId, ProviderConnectionId};

const MAX_DISPLAY_NAME_LENGTH: usize = 200;
const MAX_REFERENCE_LENGTH: usize = 128;

/// Provider implementation/preset represented by a durable connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Eggpool,
    OpenAiCompatible,
    OpenAi,
    Anthropic,
    Google,
    AzureOpenAi,
    /// Provider implementations can be added without changing the storage
    /// shape. The value is an implementation identifier, not a secret.
    Other(String),
}

impl ProviderKind {
    fn storage_key(&self) -> Result<String, ProviderConnectionError> {
        let key = match self {
            Self::Eggpool => "eggpool".to_owned(),
            Self::OpenAiCompatible => "openai_compatible".to_owned(),
            Self::OpenAi => "openai".to_owned(),
            Self::Anthropic => "anthropic".to_owned(),
            Self::Google => "google".to_owned(),
            Self::AzureOpenAi => "azure_openai".to_owned(),
            Self::Other(value) => {
                validate_reference("provider kind", value)?;
                format!("other:{value}")
            }
        };
        Ok(key)
    }

    fn from_storage_key(value: &str) -> Result<Self, ProviderConnectionError> {
        Ok(match value {
            "eggpool" => Self::Eggpool,
            "openai_compatible" => Self::OpenAiCompatible,
            "openai" => Self::OpenAi,
            "anthropic" => Self::Anthropic,
            "google" => Self::Google,
            "azure_openai" => Self::AzureOpenAi,
            value if value.strip_prefix("other:").is_some() => {
                let custom = value.strip_prefix("other:").unwrap_or_default();
                validate_reference("provider kind", custom)?;
                Self::Other(custom.to_owned())
            }
            _ => {
                return Err(ProviderConnectionError::InvalidStorage(
                    "unknown provider kind".to_owned(),
                ))
            }
        })
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Eggpool => "eggpool",
            Self::OpenAiCompatible => "openai_compatible",
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
            Self::AzureOpenAi => "azure_openai",
            Self::Other(value) => value,
        }
    }
}

/// TLS handling requested for an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsPolicy {
    Required,
    Optional,
    Disabled,
}

impl TlsPolicy {
    fn storage_key(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Disabled => "disabled",
        }
    }

    fn from_storage_key(value: &str) -> Result<Self, ProviderConnectionError> {
        match value {
            "required" => Ok(Self::Required),
            "optional" => Ok(Self::Optional),
            "disabled" => Ok(Self::Disabled),
            _ => Err(ProviderConnectionError::InvalidStorage(
                "unknown TLS policy".to_owned(),
            )),
        }
    }
}

/// A normalized, credential-free endpoint.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    url: String,
}

impl Endpoint {
    /// Parse and normalize an HTTP(S) endpoint without making a request.
    pub fn new(value: &str, tls_policy: TlsPolicy) -> Result<Self, ProviderConnectionError> {
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(ProviderConnectionError::InvalidEndpoint(
                "endpoint must be a non-empty URL without control characters".to_owned(),
            ));
        }

        let parsed = Url::parse(value).map_err(|_| {
            ProviderConnectionError::InvalidEndpoint("endpoint must be a valid URL".to_owned())
        })?;
        let scheme = parsed.scheme().to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(ProviderConnectionError::InvalidEndpoint(
                "endpoint scheme must be http or https".to_owned(),
            ));
        }
        if parsed.host_str().is_none() {
            return Err(ProviderConnectionError::InvalidEndpoint(
                "endpoint must include a host".to_owned(),
            ));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(ProviderConnectionError::InvalidEndpoint(
                "endpoint userinfo is not permitted".to_owned(),
            ));
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(ProviderConnectionError::InvalidEndpoint(
                "endpoint query and fragment are not permitted".to_owned(),
            ));
        }

        match tls_policy {
            TlsPolicy::Required if scheme != "https" => {
                return Err(ProviderConnectionError::InvalidEndpoint(
                    "TLS is required for an https endpoint".to_owned(),
                ))
            }
            TlsPolicy::Disabled if scheme != "http" => {
                return Err(ProviderConnectionError::InvalidEndpoint(
                    "TLS must be disabled for an http endpoint".to_owned(),
                ))
            }
            _ => {}
        }

        Ok(Self {
            url: parsed.to_string(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.url
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Endpoint").field(&self.url).finish()
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.url)
    }
}

/// Scope metadata. Scope does not itself grant authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderScope {
    Personal { owner: PrincipalId },
    Project { project_id: ProjectId },
    Deployment { deployment_id: String },
}

impl ProviderScope {
    pub fn personal(owner: PrincipalId) -> Self {
        Self::Personal { owner }
    }

    pub fn project(project_id: ProjectId) -> Self {
        Self::Project { project_id }
    }

    pub fn deployment(deployment_id: impl Into<String>) -> Result<Self, ProviderConnectionError> {
        let deployment_id = deployment_id.into();
        validate_reference("deployment ID", &deployment_id)?;
        Ok(Self::Deployment { deployment_id })
    }

    fn storage_parts(&self) -> (&'static str, &str) {
        match self {
            Self::Personal { owner } => ("personal", owner.as_str()),
            Self::Project { project_id } => ("project", project_id.as_str()),
            Self::Deployment { deployment_id } => ("deployment", deployment_id),
        }
    }

    fn from_storage_parts(kind: &str, reference: String) -> Result<Self, ProviderConnectionError> {
        match kind {
            "personal" => Ok(Self::Personal {
                owner: PrincipalId::parse(&reference).map_err(ProviderConnectionError::Identity)?,
            }),
            "project" => Ok(Self::Project {
                project_id: ProjectId::parse(&reference)
                    .map_err(ProviderConnectionError::Identity)?,
            }),
            "deployment" => {
                validate_reference("deployment ID", &reference)?;
                Ok(Self::Deployment {
                    deployment_id: reference,
                })
            }
            _ => Err(ProviderConnectionError::InvalidStorage(
                "unknown provider connection scope".to_owned(),
            )),
        }
    }
}

/// Opaque locator for a secret held by a separate credential subsystem.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecretRef(String);

impl SecretRef {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn parse(value: &str) -> Result<Self, ProviderConnectionError> {
        validate_reference("secret reference", value)?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SecretRef {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretRef(REDACTED)")
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<secret-ref>")
    }
}

/// Provider/account locator for a secret. It intentionally has no plaintext
/// credential field and is safe to pass between daemon subsystems.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretBindingLocator {
    pub secret_ref: SecretRef,
    pub provider_ref: String,
    pub account_ref: String,
}

impl SecretBindingLocator {
    pub fn new(
        secret_ref: SecretRef,
        provider_ref: impl Into<String>,
        account_ref: impl Into<String>,
    ) -> Result<Self, ProviderConnectionError> {
        let provider_ref = provider_ref.into();
        let account_ref = account_ref.into();
        validate_reference("secret provider reference", &provider_ref)?;
        validate_reference("secret account reference", &account_ref)?;
        Ok(Self {
            secret_ref,
            provider_ref,
            account_ref,
        })
    }

    fn redacted(&self) -> SecretBindingSummary {
        SecretBindingSummary {
            provider_ref: self.provider_ref.clone(),
            account_ref: self.account_ref.clone(),
        }
    }
}

impl fmt::Debug for SecretBindingLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretBindingLocator")
            .field("secret_ref", &self.secret_ref)
            .field("provider_ref", &self.provider_ref)
            .field("account_ref", &self.account_ref)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretBindingSummary {
    pub provider_ref: String,
    pub account_ref: String,
}

/// Durable lifecycle state for a daemon-owned provider connection.
///
/// The original three-state column remains a compatibility projection. The
/// additive lifecycle table (introduced after the project-catalog migrations)
/// stores the extended states without rewriting the historical provider table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConnectionState {
    Active,
    Disabled,
    CredentialMissing,
    ProvisioningRotating,
    Tombstoned,
    Error,
    Stale,
}

impl ProviderConnectionState {
    pub fn storage_key(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::CredentialMissing => "credential_missing",
            Self::ProvisioningRotating => "provisioning_rotating",
            Self::Tombstoned => "tombstoned",
            Self::Error => "error",
            Self::Stale => "stale",
        }
    }

    fn from_storage_key(value: &str) -> Result<Self, ProviderConnectionError> {
        match value {
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            "credential_missing" => Ok(Self::CredentialMissing),
            "provisioning_rotating" => Ok(Self::ProvisioningRotating),
            "tombstoned" => Ok(Self::Tombstoned),
            "error" => Ok(Self::Error),
            "stale" => Ok(Self::Stale),
            _ => Err(ProviderConnectionError::InvalidStorage(
                "unknown provider connection state".to_owned(),
            )),
        }
    }

    pub fn can_transition_to(self, target: Self) -> bool {
        if self == target {
            return true;
        }
        match self {
            Self::Active | Self::Disabled | Self::CredentialMissing => matches!(
                target,
                Self::Active
                    | Self::Disabled
                    | Self::CredentialMissing
                    | Self::ProvisioningRotating
                    | Self::Tombstoned
                    | Self::Error
                    | Self::Stale
            ),
            Self::ProvisioningRotating => matches!(
                target,
                Self::Active
                    | Self::Disabled
                    | Self::CredentialMissing
                    | Self::Tombstoned
                    | Self::Error
                    | Self::Stale
            ),
            Self::Tombstoned => matches!(
                target,
                Self::Active | Self::Disabled | Self::CredentialMissing
            ),
            Self::Error => matches!(
                target,
                Self::Active
                    | Self::Disabled
                    | Self::CredentialMissing
                    | Self::Tombstoned
                    | Self::Stale
            ),
            Self::Stale => matches!(
                target,
                Self::Active
                    | Self::Disabled
                    | Self::CredentialMissing
                    | Self::Tombstoned
                    | Self::Error
            ),
        }
    }

    /// Whether a connection may be selected or resolved for a new request.
    pub fn is_selectable(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConnection {
    pub id: ProviderConnectionId,
    pub provider_kind: ProviderKind,
    pub display_name: String,
    pub endpoint: Endpoint,
    pub tls_policy: TlsPolicy,
    pub scope: ProviderScope,
    pub secret_binding: Option<SecretBindingLocator>,
    pub state: ProviderConnectionState,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl fmt::Debug for ProviderConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderConnection")
            .field("id", &self.id)
            .field("provider_kind", &self.provider_kind)
            .field("display_name", &self.display_name)
            .field("endpoint", &self.endpoint)
            .field("tls_policy", &self.tls_policy)
            .field("scope", &self.scope)
            .field("secret_binding", &self.secret_binding)
            .field("state", &self.state)
            .field("revision", &self.revision)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl ProviderConnection {
    pub fn summary(&self) -> ProviderConnectionSummary {
        ProviderConnectionSummary {
            id: self.id.clone(),
            provider_kind: self.provider_kind.clone(),
            display_name: self.display_name.clone(),
            endpoint: self.endpoint.clone(),
            tls_policy: self.tls_policy,
            scope: self.scope.clone(),
            state: self.state,
            revision: self.revision,
            has_secret_binding: self.secret_binding.is_some(),
        }
    }

    /// Explicitly named alias for callers handling externally visible data.
    pub fn redacted_summary(&self) -> ProviderConnectionSummary {
        self.summary()
    }

    pub fn detail(&self) -> ProviderConnectionDetail {
        ProviderConnectionDetail {
            summary: self.summary(),
            secret_binding: self
                .secret_binding
                .as_ref()
                .map(SecretBindingLocator::redacted),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    /// Explicitly named alias for callers handling externally visible data.
    pub fn redacted_detail(&self) -> ProviderConnectionDetail {
        self.detail()
    }

    pub fn transition(
        &self,
        target: ProviderConnectionState,
    ) -> Result<Self, ProviderConnectionError> {
        if !self.state.can_transition_to(target) {
            return Err(ProviderConnectionError::InvalidTransition {
                from: self.state,
                to: target,
            });
        }
        let mut next = self.clone();
        if self.state != target {
            next.state = target;
            next.revision = self.revision.saturating_add(1);
            next.updated_at = now_millis();
        }
        Ok(next)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConnectionSummary {
    pub id: ProviderConnectionId,
    pub provider_kind: ProviderKind,
    pub display_name: String,
    pub endpoint: Endpoint,
    pub tls_policy: TlsPolicy,
    pub scope: ProviderScope,
    pub state: ProviderConnectionState,
    pub revision: u64,
    pub has_secret_binding: bool,
}

/// Redacted detail intended for daemon/UI responses. It includes only safe
/// provider/account locators; the opaque secret reference is omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConnectionDetail {
    pub summary: ProviderConnectionSummary,
    pub secret_binding: Option<SecretBindingSummary>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Kinds of durable references that prevent a tombstoned connection from
/// being purged. The reference table is authoritative; these values are also
/// safe to expose in redacted operator diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConnectionReferenceKind {
    SelectedSession,
    ProvisioningOperation,
    ActiveRuntime,
}

impl ProviderConnectionReferenceKind {
    pub fn storage_key(self) -> &'static str {
        match self {
            Self::SelectedSession => "selected_session",
            Self::ProvisioningOperation => "provisioning_operation",
            Self::ActiveRuntime => "active_runtime",
        }
    }

    fn from_storage_key(value: &str) -> Result<Self, ProviderConnectionError> {
        match value {
            "selected_session" => Ok(Self::SelectedSession),
            "provisioning_operation" => Ok(Self::ProvisioningOperation),
            "active_runtime" => Ok(Self::ActiveRuntime),
            _ => Err(ProviderConnectionError::InvalidStorage(
                "unknown provider connection reference kind".to_owned(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConnectionReference {
    pub connection_id: ProviderConnectionId,
    pub kind: ProviderConnectionReferenceKind,
    pub reference_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConnectionTombstone {
    pub connection_id: ProviderConnectionId,
    pub tombstoned_at: i64,
    pub tombstoned_by_actor: String,
    pub last_known_revision: u64,
    pub last_known_catalog_revision: Option<String>,
    pub last_known_endpoint_authority: String,
}

#[derive(Debug, Clone)]
pub struct NewProviderConnection {
    pub provider_kind: ProviderKind,
    pub display_name: String,
    pub endpoint: Endpoint,
    pub tls_policy: TlsPolicy,
    pub scope: ProviderScope,
    pub secret_binding: Option<SecretBindingLocator>,
}

impl NewProviderConnection {
    fn validate(&self) -> Result<(), ProviderConnectionError> {
        validate_display_name(&self.display_name)?;
        // Revalidate the endpoint with the supplied policy in case a caller
        // assembled the struct rather than using Endpoint::new.
        Endpoint::new(self.endpoint.as_str(), self.tls_policy)?;
        let _ = self.provider_kind.storage_key()?;
        validate_scope(&self.scope)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ProviderConnectionUpdate {
    pub provider_kind: ProviderKind,
    pub display_name: String,
    pub endpoint: Endpoint,
    pub tls_policy: TlsPolicy,
    pub scope: ProviderScope,
    pub secret_binding: Option<SecretBindingLocator>,
}

impl From<&ProviderConnection> for ProviderConnectionUpdate {
    fn from(value: &ProviderConnection) -> Self {
        Self {
            provider_kind: value.provider_kind.clone(),
            display_name: value.display_name.clone(),
            endpoint: value.endpoint.clone(),
            tls_policy: value.tls_policy,
            scope: value.scope.clone(),
            secret_binding: value.secret_binding.clone(),
        }
    }
}

impl ProviderConnectionUpdate {
    fn validate(&self) -> Result<(), ProviderConnectionError> {
        NewProviderConnection {
            provider_kind: self.provider_kind.clone(),
            display_name: self.display_name.clone(),
            endpoint: self.endpoint.clone(),
            tls_policy: self.tls_policy,
            scope: self.scope.clone(),
            secret_binding: self.secret_binding.clone(),
        }
        .validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PurgeBlocker {
    SelectedSessions { count: u64 },
    ProvisioningOperation { operation_id: String },
    ActiveRuntime { reference_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PurgeOutcome {
    Purged,
    Blocked(Vec<PurgeBlocker>),
}

#[derive(Debug, Error)]
pub enum ProviderConnectionError {
    #[error("invalid provider connection: {0}")]
    Invalid(String),
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(String),
    #[error("invalid provider connection scope: {0}")]
    InvalidScope(String),
    #[error("invalid provider connection storage: {0}")]
    InvalidStorage(String),
    #[error("invalid provider connection identity: {0}")]
    Identity(#[from] IdentityParseError),
    #[error("provider connection not found: {0}")]
    NotFound(ProviderConnectionId),
    #[error(
        "provider connection revision conflict for {id}: expected {expected}, current {current}"
    )]
    RevisionConflict {
        id: ProviderConnectionId,
        expected: u64,
        current: u64,
    },
    #[error("provider connection conflicts with an existing scoped connection")]
    Conflict,
    #[error("invalid lifecycle transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: ProviderConnectionState,
        to: ProviderConnectionState,
    },
    #[error("provider connection purge is blocked")]
    PurgeBlocked(Vec<PurgeBlocker>),
    #[error("provider connection storage error: {0}")]
    Storage(#[from] StorageError),
}

/// Async SQLite store for daemon-owned provider connection metadata.
#[derive(Clone)]
pub struct ProviderConnectionStore {
    pool: SqlitePool,
}

/// Explicit lease for a daemon-owned runtime reference. The lease is
/// intentionally released asynchronously so purge cannot race an active
/// provider instance without the owner acknowledging completion.
pub struct ProviderConnectionRuntimeLease {
    store: ProviderConnectionStore,
    connection_id: ProviderConnectionId,
    reference_id: String,
    released: bool,
}

impl ProviderConnectionRuntimeLease {
    pub fn connection_id(&self) -> &ProviderConnectionId {
        &self.connection_id
    }

    pub fn reference_id(&self) -> &str {
        &self.reference_id
    }

    pub async fn release(mut self) -> Result<(), ProviderConnectionError> {
        if !self.released {
            self.store
                .remove_reference(
                    &self.connection_id,
                    ProviderConnectionReferenceKind::ActiveRuntime,
                    &self.reference_id,
                )
                .await?;
            self.released = true;
        }
        Ok(())
    }
}

impl Drop for ProviderConnectionRuntimeLease {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let store = self.store.clone();
        let connection_id = self.connection_id.clone();
        let reference_id = self.reference_id.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = store
                    .remove_reference(
                        &connection_id,
                        ProviderConnectionReferenceKind::ActiveRuntime,
                        &reference_id,
                    )
                    .await;
            });
        }
        self.released = true;
    }
}

impl ProviderConnectionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Borrow the underlying SQLite pool. Provided so read-only catalog
    /// helpers can run bounded queries without re-implementing every
    /// existing read path.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn create(
        &self,
        input: NewProviderConnection,
    ) -> Result<ProviderConnection, ProviderConnectionError> {
        input.validate()?;
        let id = ProviderConnectionId::new();
        let now = now_millis();
        let (scope_kind, scope_ref) = input.scope.storage_parts();
        let provider_kind = input.provider_kind.storage_key()?;
        let (secret_ref, secret_provider_ref, secret_account_ref) =
            secret_columns(input.secret_binding.as_ref());

        let result = sqlx::query(
            r#"
            INSERT INTO provider_connections (
                id, provider_kind, display_name, endpoint, tls_policy,
                scope_kind, scope_ref, secret_ref, secret_provider_ref,
                secret_account_ref, state, revision, time_created, time_updated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', 1, ?, ?)
            "#,
        )
        .bind(id.as_str())
        .bind(provider_kind)
        .bind(&input.display_name)
        .bind(input.endpoint.as_str())
        .bind(input.tls_policy.storage_key())
        .bind(scope_kind)
        .bind(scope_ref)
        .bind(secret_ref)
        .bind(secret_provider_ref)
        .bind(secret_account_ref)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => self
                .get(&id)
                .await?
                .ok_or(ProviderConnectionError::NotFound(id)),
            Err(error) => Err(map_sql_error(error)),
        }
    }

    pub async fn get(
        &self,
        id: &ProviderConnectionId,
    ) -> Result<Option<ProviderConnection>, ProviderConnectionError> {
        let row = sqlx::query_as::<_, ProviderConnectionRow>(
            "SELECT id, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, \
             secret_ref, secret_provider_ref, secret_account_ref, state, revision, \
             time_created, time_updated FROM provider_connections WHERE id = ?",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(StorageError::from)
        .map_err(ProviderConnectionError::Storage)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let connection = row.into_domain()?;
        Ok(Some(self.apply_lifecycle_state(connection).await?))
    }

    pub async fn list(&self) -> Result<Vec<ProviderConnection>, ProviderConnectionError> {
        let rows = sqlx::query_as::<_, ProviderConnectionRow>(
            "SELECT id, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, \
             secret_ref, secret_provider_ref, secret_account_ref, state, revision, \
             time_created, time_updated FROM provider_connections \
             ORDER BY time_updated DESC, id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::from)
        .map_err(ProviderConnectionError::Storage)?;
        let mut connections = Vec::with_capacity(rows.len());
        for row in rows {
            connections.push(self.apply_lifecycle_state(row.into_domain()?).await?);
        }
        Ok(connections)
    }

    pub async fn list_for_scope(
        &self,
        scope: &ProviderScope,
    ) -> Result<Vec<ProviderConnection>, ProviderConnectionError> {
        validate_scope(scope)?;
        let (kind, reference) = scope.storage_parts();
        let rows = sqlx::query_as::<_, ProviderConnectionRow>(
            "SELECT id, provider_kind, display_name, endpoint, tls_policy, scope_kind, scope_ref, \
             secret_ref, secret_provider_ref, secret_account_ref, state, revision, \
             time_created, time_updated FROM provider_connections \
             WHERE scope_kind = ? AND scope_ref = ? ORDER BY time_updated DESC, id ASC",
        )
        .bind(kind)
        .bind(reference)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::from)
        .map_err(ProviderConnectionError::Storage)?;
        let mut connections = Vec::with_capacity(rows.len());
        for row in rows {
            connections.push(self.apply_lifecycle_state(row.into_domain()?).await?);
        }
        Ok(connections)
    }

    async fn apply_lifecycle_state(
        &self,
        mut connection: ProviderConnection,
    ) -> Result<ProviderConnection, ProviderConnectionError> {
        let state = sqlx::query_scalar::<_, String>(
            "SELECT state FROM provider_connection_lifecycle WHERE connection_id = ?",
        )
        .bind(connection.id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(StorageError::from)
        .map_err(ProviderConnectionError::Storage)?;
        if let Some(state) = state {
            connection.state = ProviderConnectionState::from_storage_key(&state)?;
        }
        Ok(connection)
    }

    pub async fn update(
        &self,
        id: &ProviderConnectionId,
        expected_revision: u64,
        update: ProviderConnectionUpdate,
    ) -> Result<ProviderConnection, ProviderConnectionError> {
        update.validate()?;
        if let Some(current) = self.get(id).await? {
            if matches!(
                current.state,
                ProviderConnectionState::Tombstoned
                    | ProviderConnectionState::Error
                    | ProviderConnectionState::Stale
                    | ProviderConnectionState::ProvisioningRotating
            ) {
                return Err(ProviderConnectionError::InvalidTransition {
                    from: current.state,
                    to: current.state,
                });
            }
        }
        let (scope_kind, scope_ref) = update.scope.storage_parts();
        let provider_kind = update.provider_kind.storage_key()?;
        let (secret_ref, secret_provider_ref, secret_account_ref) =
            secret_columns(update.secret_binding.as_ref());
        let now = now_millis();
        let result = sqlx::query(
            r#"
            UPDATE provider_connections SET
                provider_kind = ?, display_name = ?, endpoint = ?, tls_policy = ?,
                scope_kind = ?, scope_ref = ?, secret_ref = ?, secret_provider_ref = ?,
                secret_account_ref = ?, revision = revision + 1, time_updated = ?
            WHERE id = ? AND revision = ?
            "#,
        )
        .bind(provider_kind)
        .bind(&update.display_name)
        .bind(update.endpoint.as_str())
        .bind(update.tls_policy.storage_key())
        .bind(scope_kind)
        .bind(scope_ref)
        .bind(secret_ref)
        .bind(secret_provider_ref)
        .bind(secret_account_ref)
        .bind(now)
        .bind(id.as_str())
        .bind(expected_revision as i64)
        .execute(&self.pool)
        .await;

        match result {
            Ok(result) if result.rows_affected() == 1 => self
                .get(id)
                .await?
                .ok_or_else(|| ProviderConnectionError::NotFound(id.clone())),
            Ok(_) => self.revision_conflict(id, expected_revision).await,
            Err(error) => Err(map_sql_error(error)),
        }
    }

    pub async fn transition(
        &self,
        id: &ProviderConnectionId,
        expected_revision: u64,
        target: ProviderConnectionState,
    ) -> Result<ProviderConnection, ProviderConnectionError> {
        let current = self
            .get(id)
            .await?
            .ok_or_else(|| ProviderConnectionError::NotFound(id.clone()))?;
        if current.revision != expected_revision {
            return Err(ProviderConnectionError::RevisionConflict {
                id: id.clone(),
                expected: expected_revision,
                current: current.revision,
            });
        }
        let next = current.transition(target)?;
        if next.revision == current.revision {
            return Ok(current);
        }
        let mut tx = self.pool.begin().await.map_err(map_sql_error)?;
        let result = sqlx::query(
            "UPDATE provider_connections SET state = ?, revision = ?, time_updated = ? \
             WHERE id = ? AND revision = ?",
        )
        .bind(compatibility_state(target).storage_key())
        .bind(next.revision as i64)
        .bind(next.updated_at)
        .bind(id.as_str())
        .bind(expected_revision as i64)
        .execute(&mut *tx)
        .await
        .map_err(map_sql_error)?;
        if result.rows_affected() != 1 {
            tx.rollback().await.map_err(map_sql_error)?;
            return self.revision_conflict(id, expected_revision).await;
        }
        if is_extended_state(target) {
            sqlx::query(
                "INSERT INTO provider_connection_lifecycle (connection_id, state, revision, time_updated) VALUES (?, ?, ?, ?) \
                 ON CONFLICT(connection_id) DO UPDATE SET state = excluded.state, revision = excluded.revision, time_updated = excluded.time_updated",
            )
            .bind(id.as_str())
            .bind(target.storage_key())
            .bind(next.revision as i64)
            .bind(next.updated_at)
            .execute(&mut *tx)
            .await
            .map_err(map_sql_error)?;
        } else {
            sqlx::query("DELETE FROM provider_connection_lifecycle WHERE connection_id = ?")
                .bind(id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(map_sql_error)?;
        }
        if target == ProviderConnectionState::Tombstoned {
            let catalog_revision = sqlx::query_scalar::<_, Option<String>>(
                "SELECT catalog_revision FROM provider_connection_health WHERE connection_id = ?",
            )
            .bind(id.as_str())
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_sql_error)?
            .flatten();
            sqlx::query(
                "INSERT INTO provider_connection_tombstones (connection_id, tombstoned_at, tombstoned_by_actor, last_known_revision, last_known_catalog_revision, last_known_endpoint_authority) \
                 VALUES (?, ?, 'system', ?, ?, ?) \
                 ON CONFLICT(connection_id) DO UPDATE SET tombstoned_at = excluded.tombstoned_at, last_known_revision = excluded.last_known_revision, last_known_catalog_revision = excluded.last_known_catalog_revision, last_known_endpoint_authority = excluded.last_known_endpoint_authority",
            )
            .bind(id.as_str())
            .bind(next.updated_at)
            .bind(next.revision as i64)
            .bind(catalog_revision)
            .bind(next.endpoint.to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_sql_error)?;
        }
        sqlx::query(
            "INSERT INTO provider_connection_audit_events (event_id, connection_id, action, actor_seam, old_revision, new_revision, endpoint_authority, outcome, time_created) VALUES (?, ?, ?, 'local_operator', ?, ?, ?, 'committed', ?)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(id.as_str())
        .bind(format!("transition:{}", target.storage_key()))
        .bind(expected_revision as i64)
        .bind(next.revision as i64)
        .bind(current.endpoint.to_string())
        .bind(now_millis())
        .execute(&mut *tx)
        .await
        .map_err(map_sql_error)?;
        tx.commit().await.map_err(map_sql_error)?;
        self.get(id)
            .await?
            .ok_or_else(|| ProviderConnectionError::NotFound(id.clone()))
    }

    pub async fn disable(
        &self,
        id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<ProviderConnection, ProviderConnectionError> {
        self.transition(id, expected_revision, ProviderConnectionState::Disabled)
            .await
    }

    pub async fn enable(
        &self,
        id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<ProviderConnection, ProviderConnectionError> {
        let current = self
            .get(id)
            .await?
            .ok_or_else(|| ProviderConnectionError::NotFound(id.clone()))?;
        let target = if current.secret_binding.is_some() {
            ProviderConnectionState::Active
        } else {
            ProviderConnectionState::CredentialMissing
        };
        self.transition(id, expected_revision, target).await
    }

    pub async fn restore(
        &self,
        id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<ProviderConnection, ProviderConnectionError> {
        self.enable(id, expected_revision).await
    }

    /// Logical deletion preserving connection identity, history, and
    /// reference diagnostics. Use [`Self::purge`] only after blockers clear.
    pub async fn delete(
        &self,
        id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<bool, ProviderConnectionError> {
        let current = self
            .get(id)
            .await?
            .ok_or_else(|| ProviderConnectionError::NotFound(id.clone()))?;
        self.transition(id, expected_revision, ProviderConnectionState::Tombstoned)
            .await
            .map(|_| current.state != ProviderConnectionState::Tombstoned)
    }

    pub async fn add_reference(
        &self,
        connection_id: &ProviderConnectionId,
        kind: ProviderConnectionReferenceKind,
        reference_id: &str,
    ) -> Result<(), ProviderConnectionError> {
        validate_reference("provider connection reference", reference_id)?;
        sqlx::query(
            "INSERT OR IGNORE INTO provider_connection_references (connection_id, reference_kind, reference_id, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(connection_id.as_str())
        .bind(kind.storage_key())
        .bind(reference_id)
        .bind(now_millis())
        .execute(&self.pool)
        .await
        .map_err(map_sql_error)?;
        Ok(())
    }

    /// Acquire a purge-blocking runtime reference after validating that the
    /// connection is currently active at the requested revision.
    pub async fn acquire_active_runtime_reference(
        &self,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
        reference_id: &str,
    ) -> Result<ProviderConnectionRuntimeLease, ProviderConnectionError> {
        let current = self
            .get(connection_id)
            .await?
            .ok_or_else(|| ProviderConnectionError::NotFound(connection_id.clone()))?;
        if current.revision != expected_revision {
            return Err(ProviderConnectionError::RevisionConflict {
                id: connection_id.clone(),
                expected: expected_revision,
                current: current.revision,
            });
        }
        if current.state != ProviderConnectionState::Active {
            return Err(ProviderConnectionError::InvalidTransition {
                from: current.state,
                to: ProviderConnectionState::Active,
            });
        }
        self.add_reference(
            connection_id,
            ProviderConnectionReferenceKind::ActiveRuntime,
            reference_id,
        )
        .await?;
        Ok(ProviderConnectionRuntimeLease {
            store: self.clone(),
            connection_id: connection_id.clone(),
            reference_id: reference_id.to_owned(),
            released: false,
        })
    }

    pub async fn remove_reference(
        &self,
        connection_id: &ProviderConnectionId,
        kind: ProviderConnectionReferenceKind,
        reference_id: &str,
    ) -> Result<(), ProviderConnectionError> {
        sqlx::query(
            "DELETE FROM provider_connection_references WHERE connection_id = ? AND reference_kind = ? AND reference_id = ?",
        )
        .bind(connection_id.as_str())
        .bind(kind.storage_key())
        .bind(reference_id)
        .execute(&self.pool)
        .await
        .map_err(map_sql_error)?;
        Ok(())
    }

    pub async fn references(
        &self,
        connection_id: &ProviderConnectionId,
    ) -> Result<Vec<ProviderConnectionReference>, ProviderConnectionError> {
        let rows = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT reference_kind, reference_id, created_at FROM provider_connection_references WHERE connection_id = ? ORDER BY created_at, reference_id",
        )
        .bind(connection_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(map_sql_error)?;
        rows.into_iter()
            .map(|(kind, reference_id, created_at)| {
                Ok(ProviderConnectionReference {
                    connection_id: connection_id.clone(),
                    kind: ProviderConnectionReferenceKind::from_storage_key(&kind)?,
                    reference_id,
                    created_at,
                })
            })
            .collect()
    }

    pub async fn purge_eligibility(
        &self,
        id: &ProviderConnectionId,
    ) -> Result<Vec<PurgeBlocker>, ProviderConnectionError> {
        let mut blockers = Vec::new();
        let refs = self.references(id).await?;
        let selected = refs
            .iter()
            .filter(|reference| reference.kind == ProviderConnectionReferenceKind::SelectedSession)
            .count() as u64;
        if selected > 0 {
            blockers.push(PurgeBlocker::SelectedSessions { count: selected });
        }
        for reference in refs {
            match reference.kind {
                ProviderConnectionReferenceKind::ProvisioningOperation => {
                    blockers.push(PurgeBlocker::ProvisioningOperation {
                        operation_id: reference.reference_id,
                    });
                }
                ProviderConnectionReferenceKind::ActiveRuntime => {
                    blockers.push(PurgeBlocker::ActiveRuntime {
                        reference_id: reference.reference_id,
                    });
                }
                ProviderConnectionReferenceKind::SelectedSession => {}
            }
        }
        Ok(blockers)
    }

    pub async fn purge(
        &self,
        id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<PurgeOutcome, ProviderConnectionError> {
        let current = self
            .get(id)
            .await?
            .ok_or_else(|| ProviderConnectionError::NotFound(id.clone()))?;
        if current.revision != expected_revision {
            return Err(ProviderConnectionError::RevisionConflict {
                id: id.clone(),
                expected: expected_revision,
                current: current.revision,
            });
        }
        if current.state != ProviderConnectionState::Tombstoned {
            return Err(ProviderConnectionError::InvalidTransition {
                from: current.state,
                to: ProviderConnectionState::Tombstoned,
            });
        }
        let blockers = self.purge_eligibility(id).await?;
        if !blockers.is_empty() {
            return Ok(PurgeOutcome::Blocked(blockers));
        }
        let mut tx = self.pool.begin().await.map_err(map_sql_error)?;
        for statement in [
            "DELETE FROM provider_connection_references WHERE connection_id = ?",
            "DELETE FROM provider_connection_audit_events WHERE connection_id = ?",
            "DELETE FROM provider_connection_tombstones WHERE connection_id = ?",
            "DELETE FROM provider_connection_lifecycle WHERE connection_id = ?",
            "DELETE FROM provider_connections WHERE id = ? AND revision = ?",
        ] {
            let mut query = sqlx::query(statement).bind(id.as_str());
            if statement.contains("revision = ?") {
                query = query.bind(expected_revision as i64);
            }
            query.execute(&mut *tx).await.map_err(map_sql_error)?;
        }
        tx.commit().await.map_err(map_sql_error)?;
        Ok(PurgeOutcome::Purged)
    }

    /// Compatibility name for older callers. Deletion is now always a
    /// tombstone transition; callers that need physical removal must use
    /// [`Self::purge`] after reference blockers clear.
    pub async fn delete_metadata(
        &self,
        id: &ProviderConnectionId,
        expected_revision: u64,
    ) -> Result<bool, ProviderConnectionError> {
        if self.get(id).await?.is_none() {
            return Ok(false);
        }
        self.delete(id, expected_revision).await
    }

    async fn revision_conflict(
        &self,
        id: &ProviderConnectionId,
        expected: u64,
    ) -> Result<ProviderConnection, ProviderConnectionError> {
        match self.get(id).await? {
            Some(current) => Err(ProviderConnectionError::RevisionConflict {
                id: id.clone(),
                expected,
                current: current.revision,
            }),
            None => Err(ProviderConnectionError::NotFound(id.clone())),
        }
    }
}

#[derive(Debug, FromRow)]
struct ProviderConnectionRow {
    id: String,
    provider_kind: String,
    display_name: String,
    endpoint: String,
    tls_policy: String,
    scope_kind: String,
    scope_ref: String,
    secret_ref: String,
    secret_provider_ref: String,
    secret_account_ref: String,
    state: String,
    revision: i64,
    time_created: i64,
    time_updated: i64,
}

impl ProviderConnectionRow {
    fn into_domain(self) -> Result<ProviderConnection, ProviderConnectionError> {
        let id =
            ProviderConnectionId::parse(&self.id).map_err(ProviderConnectionError::Identity)?;
        let provider_kind = ProviderKind::from_storage_key(&self.provider_kind)?;
        let tls_policy = TlsPolicy::from_storage_key(&self.tls_policy)?;
        let endpoint = Endpoint::new(&self.endpoint, tls_policy)?;
        let scope = ProviderScope::from_storage_parts(&self.scope_kind, self.scope_ref)?;
        let state = ProviderConnectionState::from_storage_key(&self.state)?;
        let revision = u64::try_from(self.revision)
            .map_err(|_| ProviderConnectionError::InvalidStorage("invalid revision".to_owned()))?;
        let secret_binding = if self.secret_ref.is_empty()
            && self.secret_provider_ref.is_empty()
            && self.secret_account_ref.is_empty()
        {
            None
        } else {
            Some(SecretBindingLocator::new(
                SecretRef::parse(&self.secret_ref)?,
                self.secret_provider_ref,
                self.secret_account_ref,
            )?)
        };
        validate_display_name(&self.display_name)?;
        Ok(ProviderConnection {
            id,
            provider_kind,
            display_name: self.display_name,
            endpoint,
            tls_policy,
            scope,
            secret_binding,
            state,
            revision,
            created_at: self.time_created,
            updated_at: self.time_updated,
        })
    }
}

fn validate_scope(scope: &ProviderScope) -> Result<(), ProviderConnectionError> {
    match scope {
        ProviderScope::Personal { .. } | ProviderScope::Project { .. } => Ok(()),
        ProviderScope::Deployment { deployment_id } => {
            validate_reference("deployment ID", deployment_id)
        }
    }
}

fn validate_display_name(value: &str) -> Result<(), ProviderConnectionError> {
    if value.trim().is_empty() || value.len() > MAX_DISPLAY_NAME_LENGTH {
        return Err(ProviderConnectionError::Invalid(
            "display name must be non-empty and bounded".to_owned(),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(ProviderConnectionError::Invalid(
            "display name must not contain control characters".to_owned(),
        ));
    }
    Ok(())
}

fn validate_reference(kind: &str, value: &str) -> Result<(), ProviderConnectionError> {
    if value.is_empty() || value.len() > MAX_REFERENCE_LENGTH {
        return Err(ProviderConnectionError::Invalid(format!(
            "{kind} must be non-empty and bounded"
        )));
    }
    if value
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
        || value.contains('/')
        || value.contains('\\')
    {
        return Err(ProviderConnectionError::Invalid(format!(
            "{kind} contains unsupported characters"
        )));
    }
    Ok(())
}

fn compatibility_state(state: ProviderConnectionState) -> ProviderConnectionState {
    match state {
        ProviderConnectionState::Active => ProviderConnectionState::Active,
        ProviderConnectionState::CredentialMissing => ProviderConnectionState::CredentialMissing,
        _ => ProviderConnectionState::Disabled,
    }
}

fn is_extended_state(state: ProviderConnectionState) -> bool {
    matches!(
        state,
        ProviderConnectionState::ProvisioningRotating
            | ProviderConnectionState::Tombstoned
            | ProviderConnectionState::Error
            | ProviderConnectionState::Stale
    )
}

fn secret_columns(binding: Option<&SecretBindingLocator>) -> (&str, &str, &str) {
    match binding {
        Some(binding) => (
            binding.secret_ref.as_str(),
            binding.provider_ref.as_str(),
            binding.account_ref.as_str(),
        ),
        None => ("", "", ""),
    }
}

fn map_sql_error(error: sqlx::Error) -> ProviderConnectionError {
    if error
        .to_string()
        .to_ascii_lowercase()
        .contains("unique constraint")
    {
        ProviderConnectionError::Conflict
    } else {
        ProviderConnectionError::Storage(StorageError::from(error))
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::time::Duration;

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate sqlite");
        pool
    }

    fn input(scope: ProviderScope) -> NewProviderConnection {
        NewProviderConnection {
            provider_kind: ProviderKind::Eggpool,
            display_name: "Shared Eggpool".to_owned(),
            endpoint: Endpoint::new("HTTPS://example.com:443/", TlsPolicy::Required).unwrap(),
            tls_policy: TlsPolicy::Required,
            scope,
            secret_binding: Some(
                SecretBindingLocator::new(SecretRef::new(), "eggpool", "account-a").unwrap(),
            ),
        }
    }

    #[test]
    fn endpoint_normalizes_and_rejects_credentials_or_network_only_shapes() {
        let endpoint = Endpoint::new(" HTTPS://example.com:443/ ", TlsPolicy::Required).unwrap();
        assert_eq!(endpoint.as_str(), "https://example.com/");
        assert!(Endpoint::new("https://user:password@example.com", TlsPolicy::Required).is_err());
        assert!(Endpoint::new("file:///tmp/provider", TlsPolicy::Optional).is_err());
        assert!(Endpoint::new("http://example.com", TlsPolicy::Required).is_err());
        assert!(Endpoint::new("https://example.com?key=value", TlsPolicy::Required).is_err());
    }

    #[test]
    fn scope_validation_requires_safe_deployment_reference() {
        assert!(ProviderScope::deployment("deployment-a").is_ok());
        assert!(ProviderScope::deployment("/tmp/deployment").is_err());
        assert!(ProviderScope::deployment("deployment a").is_err());
        let project = ProjectId::new();
        assert!(matches!(
            ProviderScope::project(project),
            ProviderScope::Project { .. }
        ));
    }

    #[test]
    fn redacted_views_and_debug_never_include_secret_reference() {
        let secret_ref = SecretRef::new();
        let secret_ref_text = secret_ref.as_str().to_owned();
        let connection = ProviderConnection {
            id: ProviderConnectionId::new(),
            provider_kind: ProviderKind::OpenAiCompatible,
            display_name: "Provider".to_owned(),
            endpoint: Endpoint::new("https://example.com", TlsPolicy::Required).unwrap(),
            tls_policy: TlsPolicy::Required,
            scope: ProviderScope::project(ProjectId::new()),
            secret_binding: Some(
                SecretBindingLocator::new(secret_ref, "openai", "account-a").unwrap(),
            ),
            state: ProviderConnectionState::Active,
            revision: 1,
            created_at: 1,
            updated_at: 1,
        };
        let debug = format!("{connection:?}");
        let detail = serde_json::to_string(&connection.detail()).unwrap();
        assert!(!debug.contains(&secret_ref_text));
        assert!(!detail.contains(&secret_ref_text));
        assert!(debug.contains("REDACTED"));
        assert!(detail.contains("account-a"));
    }

    #[test]
    fn lifecycle_transition_increments_revision() {
        let connection = ProviderConnection {
            id: ProviderConnectionId::new(),
            provider_kind: ProviderKind::Eggpool,
            display_name: "Provider".to_owned(),
            endpoint: Endpoint::new("https://example.com", TlsPolicy::Required).unwrap(),
            tls_policy: TlsPolicy::Required,
            scope: ProviderScope::project(ProjectId::new()),
            secret_binding: None,
            state: ProviderConnectionState::Active,
            revision: 4,
            created_at: 1,
            updated_at: 1,
        };
        let disabled = connection
            .transition(ProviderConnectionState::Disabled)
            .unwrap();
        assert_eq!(disabled.state, ProviderConnectionState::Disabled);
        assert_eq!(disabled.revision, 5);
        assert!(disabled
            .transition(ProviderConnectionState::CredentialMissing)
            .is_ok());
    }

    #[test]
    fn lifecycle_transition_matrix_covers_all_49_pairs() {
        let states = [
            ProviderConnectionState::Active,
            ProviderConnectionState::Disabled,
            ProviderConnectionState::CredentialMissing,
            ProviderConnectionState::ProvisioningRotating,
            ProviderConnectionState::Tombstoned,
            ProviderConnectionState::Error,
            ProviderConnectionState::Stale,
        ];
        let mut pairs = 0;
        for from in states {
            for to in states {
                pairs += 1;
                let connection = ProviderConnection {
                    id: ProviderConnectionId::new(),
                    provider_kind: ProviderKind::Eggpool,
                    display_name: "Provider".to_owned(),
                    endpoint: Endpoint::new("https://example.com", TlsPolicy::Required).unwrap(),
                    tls_policy: TlsPolicy::Required,
                    scope: ProviderScope::project(ProjectId::new()),
                    secret_binding: None,
                    state: from,
                    revision: 1,
                    created_at: 1,
                    updated_at: 1,
                };
                assert_eq!(
                    connection.transition(to).is_ok(),
                    from.can_transition_to(to)
                );
            }
        }
        assert_eq!(pairs, 49);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migration_is_idempotent_and_store_crud_is_revision_safe() {
        let database = pool().await;
        crate::session::schema::migrate(&database)
            .await
            .expect("second migration");
        let version: i64 = sqlx::query_scalar("SELECT version FROM migration_version WHERE id = 1")
            .fetch_one(&database)
            .await
            .unwrap();
        assert_eq!(version, 31);
        let store = ProviderConnectionStore::new(database.clone());
        let created = store
            .create(input(ProviderScope::project(ProjectId::new())))
            .await
            .unwrap();
        assert_eq!(created.revision, 1);
        assert!(created.secret_binding.is_some());
        assert_eq!(store.list().await.unwrap().len(), 1);
        let fetched = store.get(&created.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, created.id);

        let mut update = ProviderConnectionUpdate::from(&created);
        update.display_name = "Renamed".to_owned();
        let updated = store.update(&created.id, 1, update).await.unwrap();
        assert_eq!(updated.display_name, "Renamed");
        assert_eq!(updated.revision, 2);
        assert!(matches!(
            store
                .update(&created.id, 1, ProviderConnectionUpdate::from(&updated))
                .await,
            Err(ProviderConnectionError::RevisionConflict { .. })
        ));

        let disabled = store.disable(&created.id, 2).await.unwrap();
        assert_eq!(disabled.state, ProviderConnectionState::Disabled);
        assert_eq!(disabled.revision, 3);
        assert_eq!(
            store.get(&created.id).await.unwrap().unwrap().state,
            ProviderConnectionState::Disabled
        );
        assert!(store.delete_metadata(&created.id, 3).await.unwrap());
        let tombstone = store.get(&created.id).await.unwrap().unwrap();
        assert_eq!(tombstone.state, ProviderConnectionState::Tombstoned);
        assert!(matches!(
            store.purge(&created.id, tombstone.revision).await.unwrap(),
            PurgeOutcome::Purged
        ));
        assert!(!store.delete_metadata(&created.id, 1).await.unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn equivalent_scope_and_binding_is_unique() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database);
        let project = ProjectId::new();
        let first = store
            .create(input(ProviderScope::project(project.clone())))
            .await
            .unwrap();
        let mut duplicate = input(ProviderScope::project(project));
        duplicate.secret_binding = first.secret_binding.clone();
        assert!(matches!(
            store.create(duplicate).await,
            Err(ProviderConnectionError::Conflict)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tombstone_restore_and_purge_are_reference_safe() {
        let database = pool().await;
        let store = ProviderConnectionStore::new(database);
        let created = store
            .create(input(ProviderScope::project(ProjectId::new())))
            .await
            .unwrap();
        let tombstoned = store.delete(&created.id, created.revision).await.unwrap();
        assert!(tombstoned);
        let tombstone = store.get(&created.id).await.unwrap().unwrap();
        assert_eq!(tombstone.state, ProviderConnectionState::Tombstoned);
        store
            .add_reference(
                &created.id,
                ProviderConnectionReferenceKind::SelectedSession,
                "session-1",
            )
            .await
            .unwrap();
        assert!(matches!(
            store.purge(&created.id, tombstone.revision).await.unwrap(),
            PurgeOutcome::Blocked(_)
        ));
        store
            .remove_reference(
                &created.id,
                ProviderConnectionReferenceKind::SelectedSession,
                "session-1",
            )
            .await
            .unwrap();
        let restored = store
            .restore(&created.id, tombstone.revision)
            .await
            .unwrap();
        assert_eq!(restored.state, ProviderConnectionState::Active);
        let tombstoned_again = store.delete(&created.id, restored.revision).await.unwrap();
        assert!(tombstoned_again);
        let tombstone_again = store.get(&created.id).await.unwrap().unwrap();
        assert!(matches!(
            store
                .purge(&created.id, tombstone_again.revision)
                .await
                .unwrap(),
            PurgeOutcome::Purged
        ));
        assert!(store.get(&created.id).await.unwrap().is_none());
    }
}
