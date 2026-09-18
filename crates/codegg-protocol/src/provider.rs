//! Secret-safe protocol contracts for daemon-owned provider connections.
//!
//! `SecretInput` is deliberately serializable because the local authenticated
//! IPC request must carry the value to the daemon. Its `Debug` and `Display`
//! implementations are redacted so request tracing, test failures, and
//! diagnostics cannot accidentally print the key.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Bounded API-key envelope for the local create request.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretInput(String);

impl SecretInput {
    pub const MAX_LEN: usize = 16 * 1024;

    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() || value.len() > Self::MAX_LEN || value.chars().any(char::is_control) {
            return Err("secret input must be non-empty, bounded, and free of control characters");
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Debug for SecretInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretInput(REDACTED)")
    }
}

impl fmt::Display for SecretInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<secret-input>")
    }
}

/// Opaque reference to a secret already held by the daemon's bounded local
/// input buffer. Rotation requests carry this handle, never credential bytes.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretInputRef {
    pub handle: String,
}

impl SecretInputRef {
    pub const MAX_HANDLE_LEN: usize = 256;

    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let handle = value.into();
        if handle.is_empty()
            || handle.len() > Self::MAX_HANDLE_LEN
            || handle.chars().any(char::is_control)
        {
            return Err(
                "secret input handle must be non-empty, bounded, and free of control characters",
            );
        }
        Ok(Self { handle })
    }
}

impl fmt::Debug for SecretInputRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretInputRef(REDACTED)")
    }
}

impl fmt::Display for SecretInputRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<secret-input-ref>")
    }
}

/// Metadata changes supported by the staged rotation transaction. Endpoint
/// values are validated again by the daemon before they reach storage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ConnectionRotateChange {
    CredentialOnly,
    EndpointOnly {
        endpoint: String,
        tls_policy: String,
        #[serde(default)]
        display_name: Option<String>,
    },
    CredentialAndEndpoint {
        endpoint: String,
        tls_policy: String,
        #[serde(default)]
        display_name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionRotateStatusDto {
    pub request_id: String,
    pub connection_id: String,
    pub state: String,
    #[serde(default)]
    pub new_revision: Option<u64>,
    #[serde(default)]
    pub catalog_revision: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionRefreshStatusDto {
    pub operation_id: String,
    pub connection_id: String,
    pub state: String,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub catalog_revision: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionDetailDto {
    pub connection_id: String,
    pub display_name: String,
    pub endpoint_authority: String,
    pub tls_policy: String,
    pub scope: String,
    pub state: String,
    pub revision: u64,
    #[serde(default)]
    pub catalog_revision: Option<String>,
    #[serde(default)]
    pub health: Option<ConnectionHealthDto>,
    #[serde(default)]
    pub actor_seam: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PurgeBlocker {
    SelectedSessions { count: u64 },
    ProvisioningOperation { operation_id: String },
    ActiveRuntime { reference_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PurgeOutcome {
    Purged,
    Blocked(Vec<PurgeBlocker>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionLifecycleProjection {
    pub connection_id: String,
    pub state: String,
    #[serde(default)]
    pub last_health_at: Option<i64>,
    #[serde(default)]
    pub current_selected_model_id: Option<String>,
    #[serde(default)]
    pub removed_models: Vec<String>,
}

/// TLS policy selected by the Eggpool connect form.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EggpoolTlsPolicy {
    Required,
    Optional,
    Disabled,
}

/// Scope metadata supplied by a trusted frontend context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EggpoolConnectionScope {
    Personal { owner_id: String },
    Project { project_id: String },
    Deployment { deployment_id: String },
}

/// Secret-free create request. The secret field is only accepted over the
/// daemon's local authenticated IPC boundary.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateEggpoolConnectionRequest {
    pub host: String,
    pub port: Option<u16>,
    pub tls_policy: EggpoolTlsPolicy,
    pub api_key: SecretInput,
    pub display_name: Option<String>,
    pub scope: EggpoolConnectionScope,
    #[serde(default)]
    pub operation_id: Option<String>,
}

impl fmt::Debug for CreateEggpoolConnectionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateEggpoolConnectionRequest")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls_policy", &self.tls_policy)
            .field("api_key", &self.api_key)
            .field("display_name", &self.display_name)
            .field("scope", &self.scope)
            .field("operation_id", &self.operation_id)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderModelDto {
    pub id: String,
    pub name: String,
    pub context_window: u64,
    pub max_output_tokens: Option<u64>,
    pub supports_tools: bool,
    pub supports_vision: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionHealthDto {
    pub status: String,
    pub reason_code: Option<String>,
    pub checked_at: i64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConnectionSummaryDto {
    pub id: String,
    pub provider_kind: String,
    pub display_name: String,
    pub endpoint: String,
    pub tls_policy: String,
    pub scope: String,
    pub state: String,
    pub revision: u64,
    pub model_count: usize,
    pub catalog_revision: Option<String>,
    pub health: Option<ConnectionHealthDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateEggpoolConnectionResult {
    pub operation_id: String,
    pub connection: ProviderConnectionSummaryDto,
    pub models: Vec<ProviderModelDto>,
    pub catalog_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionProvisioningStatusDto {
    pub operation_id: String,
    pub state: String,
    pub connection_id: Option<String>,
    pub reason_code: Option<String>,
}

// ── Provider-neutral provisioning (M002) ─────────────────────────────
// The Eggpool-named request above remains only as a temporary compatibility
// adapter. New production callers must use `CreateProviderConnectionRequest`
// through `CoreRequest::ProviderConnectionCreate`.

/// TLS policy for a provider-neutral create request.
///
/// Wire-compatible with [`EggpoolTlsPolicy`]; conversions keep the
/// Eggpool-named request working as a thin adapter over the generic service.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTlsPolicy {
    Required,
    Optional,
    Disabled,
}

impl From<EggpoolTlsPolicy> for ProviderTlsPolicy {
    fn from(policy: EggpoolTlsPolicy) -> Self {
        match policy {
            EggpoolTlsPolicy::Required => Self::Required,
            EggpoolTlsPolicy::Optional => Self::Optional,
            EggpoolTlsPolicy::Disabled => Self::Disabled,
        }
    }
}

impl From<ProviderTlsPolicy> for EggpoolTlsPolicy {
    fn from(policy: ProviderTlsPolicy) -> Self {
        match policy {
            ProviderTlsPolicy::Required => Self::Required,
            ProviderTlsPolicy::Optional => Self::Optional,
            ProviderTlsPolicy::Disabled => Self::Disabled,
        }
    }
}

/// Scope metadata for a provider-neutral create request. Scope never grants
/// authorization; it is daemon-owned context like the Eggpool variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProviderConnectionScope {
    Personal { owner_id: String },
    Project { project_id: String },
    Deployment { deployment_id: String },
}

impl From<EggpoolConnectionScope> for ProviderConnectionScope {
    fn from(scope: EggpoolConnectionScope) -> Self {
        match scope {
            EggpoolConnectionScope::Personal { owner_id } => Self::Personal { owner_id },
            EggpoolConnectionScope::Project { project_id } => Self::Project { project_id },
            EggpoolConnectionScope::Deployment { deployment_id } => {
                Self::Deployment { deployment_id }
            }
        }
    }
}

impl From<ProviderConnectionScope> for EggpoolConnectionScope {
    fn from(scope: ProviderConnectionScope) -> Self {
        match scope {
            ProviderConnectionScope::Personal { owner_id } => Self::Personal { owner_id },
            ProviderConnectionScope::Project { project_id } => Self::Project { project_id },
            ProviderConnectionScope::Deployment { deployment_id } => {
                Self::Deployment { deployment_id }
            }
        }
    }
}

/// Credential kind carried by a provider-neutral create request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCredentialKind {
    #[default]
    ApiKey,
    Bearer,
}

/// Provider-neutral secret-bearing create request.
///
/// The secret field is only accepted over the daemon's local authenticated
/// IPC boundary; remote WebSocket transport denies every secret-bearing
/// request (see `CoreRequest::is_secret_bearing`).
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateProviderConnectionRequest {
    /// Stable setup-catalog provider ID (`openai`, `mistral`, `eggpool`,
    /// `custom`, …).
    pub provider_id: String,
    /// Endpoint input. Semantics depend on the catalog endpoint policy:
    /// Eggpool-preset host, full base URL for endpoint-requiring providers,
    /// or an optional override. Absent for fixed-endpoint providers.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Optional explicit port for host-style presets (Eggpool).
    #[serde(default)]
    pub port: Option<u16>,
    /// TLS policy for endpoint-capable providers. `None` infers the policy
    /// from the endpoint scheme.
    #[serde(default)]
    pub tls_policy: Option<ProviderTlsPolicy>,
    /// The credential secret. Never logged, never stored in SQLite.
    pub credential: SecretInput,
    /// Which credential kind `credential` carries.
    #[serde(default)]
    pub credential_kind: ProviderCredentialKind,
    #[serde(default)]
    pub display_name: Option<String>,
    pub scope: ProviderConnectionScope,
    #[serde(default)]
    pub operation_id: Option<String>,
}

impl fmt::Debug for CreateProviderConnectionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateProviderConnectionRequest")
            .field("provider_id", &self.provider_id)
            .field("endpoint", &self.endpoint)
            .field("port", &self.port)
            .field("tls_policy", &self.tls_policy)
            .field("credential", &self.credential)
            .field("credential_kind", &self.credential_kind)
            .field("display_name", &self.display_name)
            .field("scope", &self.scope)
            .field("operation_id", &self.operation_id)
            .finish()
    }
}

impl From<CreateEggpoolConnectionRequest> for CreateProviderConnectionRequest {
    /// Compatibility adapter: the legacy Eggpool-named request becomes the
    /// generic request with the Eggpool preset ID. Eggpool credentials are
    /// API keys.
    fn from(request: CreateEggpoolConnectionRequest) -> Self {
        Self {
            provider_id: "eggpool".to_string(),
            endpoint: Some(request.host),
            port: request.port,
            tls_policy: Some(request.tls_policy.into()),
            credential: request.api_key,
            credential_kind: ProviderCredentialKind::ApiKey,
            display_name: request.display_name,
            scope: request.scope.into(),
            operation_id: request.operation_id,
        }
    }
}

/// Provider-neutral create result. Same shape as the Eggpool result: the
/// connection summary, bounded model catalog, and catalog revision carry no
/// secrets.
pub type CreateProviderConnectionResult = CreateEggpoolConnectionResult;

/// One secret-free setup-catalog entry for provider-selection surfaces.
///
/// This is presentation metadata only: stable ID, display name,
/// connectability, accepted credential kinds, endpoint requirements, and
/// non-secret defaults. It never grants authorization and never carries
/// secrets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderSetupEntryDto {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub connectable: bool,
    /// Accepted credential kinds, e.g. `["api_key"]` or `["api_key", "bearer"]`.
    pub credential_kinds: Vec<String>,
    /// Endpoint policy: `fixed`, `optional_override`, `required`, or
    /// `proxy_preset`.
    pub endpoint_policy: String,
    /// Non-secret default endpoint/origin when the catalog pins one.
    #[serde(default)]
    pub default_endpoint: Option<String>,
    pub requires_endpoint: bool,
    #[serde(default)]
    pub env_var: Option<String>,
}

// ── Provider Connections Milestone 3: session selection ────────────────

/// Wire-level model descriptor returned alongside a session's selected
/// connection. Mirrors `ProviderModelDto` plus a `catalog_revision` so
/// clients can detect stale selections without further round trips.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectedModelDto {
    pub connection_id: String,
    pub model_id: String,
    pub model_name: String,
    pub context_window: u64,
    pub max_output_tokens: Option<u64>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub catalog_revision: String,
}

/// Result of resolving the current session selection. One of three
/// variants is always present:
///
/// - `selected`: a durable connection + model is bound to the session.
/// - `legacy_unresolved`: the session persists a legacy `provider/model`
///   string but no matching connection was found.
/// - `unselected`: the session has neither a connection nor a legacy
///   provider/model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionSelectionDto {
    Selected {
        connection: Box<ProviderConnectionSummaryDto>,
        model: Box<SelectedModelDto>,
        connection_revision: u64,
        catalog_revision: String,
    },
    LegacyUnresolved {
        legacy_provider: String,
        legacy_model: Option<String>,
        reason: String,
    },
    Unselected {},
}

/// Request to update a session's connection + model selection. The
/// `expected_connection_revision` and `expected_catalog_revision` fields
/// implement optimistic concurrency: when set, an update that conflicts
/// returns `selection_revision_conflict` rather than overwriting the
/// stored selection. When `None`, the update is treated as a fresh
/// selection that replaces whatever the session had.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateSessionSelectionRequest {
    pub session_id: String,
    pub connection_id: String,
    pub model_id: String,
    pub expected_connection_revision: Option<u64>,
    pub expected_catalog_revision: Option<String>,
}
