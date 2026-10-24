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

// ── Provider reasoning control (M004): per-model capability payload ────

/// The status of one reasoning-capability dimension.
///
/// Mirrors the five statuses of the per-model capability record
/// (ADR-0050) so a client can tell "nobody has published a contract for
/// this dimension" ([`ReasoningCapabilityAxis::Unknown`]) from "the model
/// refuses this" ([`ReasoningCapabilityAxis::Unsupported`]).
///
/// `Mixed` and `Conflicting` are **not established** either: a mixed
/// dimension is accepted by some model variants and refused by others, and
/// a conflicting one is contradicted by its sources. A frontend that cannot
/// tell which applies treats both the way it treats `Unknown`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningCapabilityAxis {
    /// Nobody has published a contract for this dimension.
    #[default]
    Unknown,
    /// The dimension is accepted.
    Supported,
    /// The dimension is refused.
    Unsupported,
    /// The dimension is accepted by some model variants and refused by
    /// others.
    Mixed,
    /// Two sources disagree about the dimension.
    Conflicting,
}

impl ReasoningCapabilityAxis {
    /// Whether this axis carries an established, affirmative contract.
    ///
    /// Only `Supported` qualifies. `Unsupported` is an established
    /// *refusal* and is answered separately; `Unknown`, `Mixed`, and
    /// `Conflicting` are all "not established" and take the fallback path.
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// The per-model reasoning capability of one model.
///
/// Presentation and selection data only: it grants nothing, authorizes
/// nothing, and is never an admission input. The daemon still refuses a
/// selection the model does not accept at turn admission; this payload
/// exists so a frontend can *offer* only what the model accepts.
///
/// **An absent payload means the peer predates the capability.** It reads
/// as "not established" — never as "this model accepts nothing", and never
/// as "this model accepts everything". A frontend without a payload falls
/// back to the known-label ladder and says that it is a fallback; see
/// [`reasoning_options`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReasoningCapabilityDto {
    /// The status of the **effort** dimension: whether a provider-scoped
    /// reasoning-effort label is accepted at all.
    #[serde(default)]
    pub effort: ReasoningCapabilityAxis,
    /// The effort labels this model accepts, in the capability's own order.
    ///
    /// Empty when the effort dimension is not established. The order is the
    /// capability's, not this crate's: a frontend cycles the labels in the
    /// order it is given them.
    #[serde(default)]
    pub accepted_effort_labels: Vec<String>,
    /// The status of the **disable** dimension: whether reasoning can be
    /// turned off at all.
    ///
    /// Distinct from "no effort control available" and from "always
    /// reasons".
    #[serde(default)]
    pub disable: ReasoningCapabilityAxis,
    /// The status of the **budget** dimension: whether a fixed thinking
    /// budget is accepted.
    #[serde(default)]
    pub budget: ReasoningCapabilityAxis,
    /// The smallest accepted fixed thinking budget, in tokens.
    #[serde(default)]
    pub min_budget_tokens: Option<u64>,
    /// The largest accepted fixed thinking budget, in tokens.
    #[serde(default)]
    pub max_budget_tokens: Option<u64>,
    /// Whether the model always reasons.
    ///
    /// `Some(true)` is the always-reasons property: the model has no off
    /// switch, so the disable dimension cannot be `Supported`. `None` means
    /// the property is not established.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always_reasons: Option<bool>,
}

impl ReasoningCapabilityDto {
    /// Whether a fixed budget of `tokens` lies inside this model's accepted
    /// range.
    ///
    /// A model that publishes no bound accepts any budget; a model that
    /// publishes one bound is bounded on that side only.
    pub fn budget_within_bounds(&self, tokens: u64) -> bool {
        if let Some(min) = self.min_budget_tokens {
            if tokens < min {
                return false;
            }
        }
        if let Some(max) = self.max_budget_tokens {
            if tokens > max {
                return false;
            }
        }
        true
    }
}

/// The reasoning options a frontend may offer for one model.
///
/// The set is always a **subset** of what the model accepts: a frontend
/// never widens it, and it never invents a label the capability did not
/// publish. It is derived by [`reasoning_options`] from the model's
/// capability payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningOptionSet {
    /// The options, in the order the frontend cycles them.
    pub options: Vec<crate::core::ReasoningMode>,
    /// Whether the effort options are the known-label **fallback** rather
    /// than the model's published accepted labels.
    ///
    /// A frontend that shows a fallback must say so: the known-label ladder
    /// is a convenience vocabulary, not a statement about the model.
    pub fallback: bool,
    /// Whether the model always reasons, so there is no off switch.
    pub always_reasons: bool,
}

impl ReasoningOptionSet {
    /// Whether the model accepts an off switch.
    ///
    /// False for a model that always reasons, and for a model whose disable
    /// dimension is refused.
    pub fn allows_disable(&self) -> bool {
        self.options
            .iter()
            .any(|option| matches!(option, crate::core::ReasoningMode::Off))
    }
}

/// Derive the reasoning options a frontend may offer for one model.
///
/// `capability` is the model's capability payload, or `None` when the peer
/// does not publish one. `None` reads as "not established" on every
/// dimension — never as "this model accepts nothing".
///
/// The rules, one per dimension:
///
/// - **effort** — established (`Supported`): the accepted labels, in the
///   capability's own order. Refused (`Unsupported`): no effort option at
///   all. Not established (`Unknown`, `Mixed`, `Conflicting`): the
///   known-label ladder, marked as a fallback.
/// - **disable** — established: an off option, *unless* the model always
///   reasons. Refused: no off option. Not established: the historic off
///   option, which the known-label vocabulary carried before the
///   capability existed.
/// - **budget** — established: a fixed-budget option, and only when the
///   offered value lies inside the model's accepted range. Refused: no
///   budget option. Not established: no budget option, because the
///   known-label vocabulary declares no budget contract.
///
/// The result is always a subset of what the model accepts.
pub fn reasoning_options(capability: Option<&ReasoningCapabilityDto>) -> ReasoningOptionSet {
    let mut options = vec![crate::core::ReasoningMode::Inherit];
    let mut fallback = false;
    let mut always_reasons = false;

    match capability {
        // No payload at all: the peer predates the capability, so every
        // dimension is not established and every dimension takes its
        // fallback.
        None => {
            fallback = true;
            options.extend(
                crate::core::KNOWN_REASONING_EFFORT_LABELS
                    .iter()
                    .map(|label| crate::core::ReasoningMode::Effort((*label).to_string())),
            );
            options.push(crate::core::ReasoningMode::Off);
        }
        Some(capability) => {
            match capability.effort {
                axis if axis.is_supported() && !capability.accepted_effort_labels.is_empty() => {
                    options.extend(
                        capability
                            .accepted_effort_labels
                            .iter()
                            .map(|label| crate::core::ReasoningMode::Effort(label.clone())),
                    );
                }
                // A supported effort axis with no published label is not a
                // published contract: fall back rather than offer nothing.
                axis if axis.is_supported() => {
                    fallback = true;
                    options.extend(
                        crate::core::KNOWN_REASONING_EFFORT_LABELS
                            .iter()
                            .map(|label| crate::core::ReasoningMode::Effort((*label).to_string())),
                    );
                }
                ReasoningCapabilityAxis::Unsupported => {}
                _ => {
                    fallback = true;
                    options.extend(
                        crate::core::KNOWN_REASONING_EFFORT_LABELS
                            .iter()
                            .map(|label| crate::core::ReasoningMode::Effort((*label).to_string())),
                    );
                }
            }

            always_reasons = capability.always_reasons.unwrap_or(false);
            if always_reasons {
                // The model always reasons: there is no off switch, whatever
                // the disable axis says. The frontend states this rather than
                // offering a control the provider would refuse.
            } else {
                match capability.disable {
                    axis if axis.is_supported() => options.push(crate::core::ReasoningMode::Off),
                    ReasoningCapabilityAxis::Unsupported => {}
                    _ => options.push(crate::core::ReasoningMode::Off),
                }
            }

            if capability.budget.is_supported() {
                if let Some(tokens) = reasoning_budget_value(capability) {
                    options.push(crate::core::ReasoningMode::Budget(tokens));
                }
            }
        }
    }

    ReasoningOptionSet {
        options,
        fallback,
        always_reasons,
    }
}

/// The fixed-budget value a frontend may offer for a model, when the model
/// accepts a budget.
///
/// The smallest accepted budget is the smallest commitment the model accepts,
/// so it is inside the model's range by construction. A model that publishes
/// only an upper bound offers that bound.
///
/// `None` when the model accepts no budget, or when the published bound does
/// not fit this platform's token counter.
pub fn reasoning_budget_value(capability: &ReasoningCapabilityDto) -> Option<usize> {
    let tokens = capability
        .min_budget_tokens
        .or(capability.max_budget_tokens)?;
    usize::try_from(tokens).ok()
}

/// The fixed-budget option for one model, when the model accepts a budget
/// and `tokens` lies inside its accepted range.
///
/// Returns `None` when the model does not accept a budget, or when the
/// value is outside the model's accepted range — a budget the model would
/// refuse is not offered.
pub fn reasoning_budget_option(
    capability: Option<&ReasoningCapabilityDto>,
    tokens: u64,
) -> Option<crate::core::ReasoningMode> {
    let capability = capability?;
    if !capability.budget.is_supported() {
        return None;
    }
    if !capability.budget_within_bounds(tokens) {
        return None;
    }
    Some(crate::core::ReasoningMode::Budget(
        usize::try_from(tokens).ok()?,
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderModelDto {
    pub id: String,
    pub name: String,
    pub context_window: u64,
    pub max_output_tokens: Option<u64>,
    pub supports_tools: bool,
    pub supports_vision: bool,
    /// The model's reasoning capability, when the daemon publishes one.
    ///
    /// Absent means the peer predates the capability, which reads as "not
    /// established" — not as "this model accepts nothing".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_capability: Option<ReasoningCapabilityDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionHealthDto {
    /// Catalog/health reachability axis (`healthy` | `unhealthy`). Says the
    /// provider catalog was or was not usable at `checked_at`.
    pub status: String,
    pub reason_code: Option<String>,
    pub checked_at: i64,
    pub duration_ms: u64,
    /// Credential-verification axis, separate from `status`.
    /// `verified` | `unverified` | `authentication_failed` |
    /// `no_credential_required`. Additive and optional so clients that do not
    /// know this field keep working unchanged; `None` means the peer predates
    /// the axis, which reads as "not established".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_status: Option<String>,
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
    /// Masked tail of the bound credential, computed daemon-side as
    /// `…` + **exactly four** trailing characters (e.g. `…LooA`).
    ///
    /// Two connections may legitimately share a display name and differ only
    /// by id and bound key, which makes them indistinguishable in an
    /// interactive picker. This is the discriminator for exactly that case.
    ///
    /// A hint is a disclosure, so it is deliberately minimal: never longer
    /// than four characters, never the *leading* characters of a key (vendor
    /// formats share prefixes, a tail does not), and never derived from
    /// anything but the stored credential's own tail. The full secret never
    /// crosses the wire and is never logged.
    ///
    /// Absent — not empty — when the connection has no credential binding, or
    /// when the stored value is shorter than the tail budget.
    ///
    /// Scope: the interactive connection dialog only. `codegg auth status`
    /// deliberately reports "no plaintext, no ciphertext, no fingerprint" and
    /// is unaffected by this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_hint: Option<String>,
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
    /// The selected model's reasoning capability, when the daemon publishes
    /// one.
    ///
    /// Absent means the peer predates the capability, which reads as "not
    /// established" — not as "this model accepts nothing".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_capability: Option<ReasoningCapabilityDto>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CoreRequest, CoreResponse, ReasoningMode};

    fn model_dto(capability: Option<ReasoningCapabilityDto>) -> ProviderModelDto {
        ProviderModelDto {
            id: "mimo-v2.6-flash".to_string(),
            name: "Mimo v2.6 Flash".to_string(),
            context_window: 200_000,
            max_output_tokens: Some(64_000),
            supports_tools: true,
            supports_vision: true,
            reasoning_capability: capability,
        }
    }

    fn selected_model_dto(capability: Option<ReasoningCapabilityDto>) -> SelectedModelDto {
        SelectedModelDto {
            connection_id: "conn-1".to_string(),
            model_id: "mimo-v2.6-flash".to_string(),
            model_name: "Mimo v2.6 Flash".to_string(),
            context_window: 200_000,
            max_output_tokens: Some(64_000),
            supports_tools: true,
            supports_vision: true,
            catalog_revision: "cat-1".to_string(),
            reasoning_capability: capability,
        }
    }

    fn capability(
        effort: ReasoningCapabilityAxis,
        labels: &[&str],
        disable: ReasoningCapabilityAxis,
        budget: ReasoningCapabilityAxis,
    ) -> ReasoningCapabilityDto {
        ReasoningCapabilityDto {
            effort,
            accepted_effort_labels: labels.iter().map(|l| (*l).to_string()).collect(),
            disable,
            budget,
            min_budget_tokens: None,
            max_budget_tokens: None,
            always_reasons: None,
        }
    }

    /// A response from a peer that predates the capability decodes cleanly
    /// and the payload reads as **absent** — "not established", never "this
    /// model accepts nothing".
    #[test]
    fn response_without_capability_payload_decodes_as_absent() {
        let json = serde_json::json!({
            "type": "provider_connection_models",
            "connection_id": "conn-1",
            "catalog_revision": "cat-1",
            "models": [{
                "id": "mimo-v2.6-flash",
                "name": "Mimo v2.6 Flash",
                "context_window": 200_000,
                "max_output_tokens": 64_000,
                "supports_tools": true,
                "supports_vision": true
            }]
        });
        let response: CoreResponse = serde_json::from_value(json).expect("decodes");
        let CoreResponse::ProviderConnectionModels { models, .. } = response else {
            panic!("expected provider_connection_models");
        };
        assert_eq!(models.len(), 1);
        assert!(
            models[0].reasoning_capability.is_none(),
            "an absent payload must read as absent, not as an empty capability"
        );

        let json = serde_json::json!({
            "type": "session_selection",
            "session_id": "sess-1",
            "selection": {
                "state": "selected",
                "connection": {
                    "id": "conn-1",
                    "provider_kind": "openai",
                    "display_name": "OpenAI",
                    "endpoint": "https://api.openai.com",
                    "tls_policy": "required",
                    "scope": "account",
                    "state": "ready",
                    "revision": 1,
                    "model_count": 1,
                    "catalog_revision": "cat-1",
                    "health": null
                },
                "model": {
                    "connection_id": "conn-1",
                    "model_id": "gpt-4o",
                    "model_name": "GPT-4o",
                    "context_window": 128_000,
                    "max_output_tokens": 16_384,
                    "supports_tools": true,
                    "supports_vision": true,
                    "catalog_revision": "cat-1"
                },
                "connection_revision": 1,
                "catalog_revision": "cat-1"
            }
        });
        let response: CoreResponse = serde_json::from_value(json).expect("decodes");
        let CoreResponse::SessionSelection { selection, .. } = response else {
            panic!("expected session_selection");
        };
        let SessionSelectionDto::Selected { model, .. } = selection else {
            panic!("expected a selected selection");
        };
        assert!(model.reasoning_capability.is_none());
    }

    /// A response that carries the payload round-trips it unchanged.
    #[test]
    fn response_with_capability_payload_round_trips() {
        let capability = capability(
            ReasoningCapabilityAxis::Supported,
            &["low", "medium", "high", "xhigh", "max"],
            ReasoningCapabilityAxis::Unsupported,
            ReasoningCapabilityAxis::Unknown,
        );
        let response = CoreResponse::ProviderConnectionModels {
            connection_id: "conn-1".to_string(),
            catalog_revision: Some("cat-1".to_string()),
            models: vec![model_dto(Some(capability.clone()))],
        };
        let encoded = serde_json::to_value(&response).expect("encodes");
        let decoded: CoreResponse = serde_json::from_value(encoded).expect("decodes");
        let CoreResponse::ProviderConnectionModels { models, .. } = decoded else {
            panic!("expected provider_connection_models");
        };
        assert_eq!(models[0].reasoning_capability.as_ref(), Some(&capability));

        let response = CoreResponse::SessionSelection {
            session_id: "sess-1".to_string(),
            selection: SessionSelectionDto::Selected {
                connection: Box::new(ProviderConnectionSummaryDto {
                    id: "conn-1".to_string(),
                    provider_kind: "minimax".to_string(),
                    display_name: "MiniMax".to_string(),
                    endpoint: "https://api.minimax.test".to_string(),
                    tls_policy: "required".to_string(),
                    scope: "account".to_string(),
                    state: "ready".to_string(),
                    revision: 1,
                    model_count: 1,
                    catalog_revision: Some("cat-1".to_string()),
                    health: None,
                    credential_hint: None,
                }),
                model: Box::new(selected_model_dto(Some(capability.clone()))),
                connection_revision: 1,
                catalog_revision: "cat-1".to_string(),
            },
        };
        let encoded = serde_json::to_value(&response).expect("encodes");
        let decoded: CoreResponse = serde_json::from_value(encoded).expect("decodes");
        let CoreResponse::SessionSelection { selection, .. } = decoded else {
            panic!("expected session_selection");
        };
        let SessionSelectionDto::Selected { model, .. } = selection else {
            panic!("expected a selected selection");
        };
        assert_eq!(model.reasoning_capability.as_ref(), Some(&capability));
    }

    /// The models channel keeps its `Vec<String>` shape: the capability
    /// payload rides on the model DTOs, never on the models snapshot.
    #[test]
    fn models_snapshot_and_refresh_shapes_are_unchanged() {
        let response = CoreResponse::ModelsSnapshot {
            current_model: Some("minimax/minimax-m3".to_string()),
            models: vec![
                "minimax/minimax-m3".to_string(),
                "openai/gpt-4o".to_string(),
            ],
        };
        let encoded = serde_json::to_value(&response).expect("encodes");
        assert_eq!(encoded["models"][0], "minimax/minimax-m3");
        assert!(
            encoded["models"].is_array(),
            "the models channel stays a list of model strings"
        );
        let decoded: CoreResponse = serde_json::from_value(encoded).expect("decodes");
        let CoreResponse::ModelsSnapshot { models, .. } = decoded else {
            panic!("expected models_snapshot");
        };
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|model| model.contains('/')));

        let request = CoreRequest::ModelsRefresh;
        let encoded = serde_json::to_value(&request).expect("encodes");
        assert_eq!(encoded["type"], "models_refresh");
        let decoded: CoreRequest = serde_json::from_value(encoded).expect("decodes");
        assert!(matches!(decoded, CoreRequest::ModelsRefresh));
    }

    /// An absent capability offers the known-label ladder **and says it is
    /// a fallback**: the ladder is a convenience vocabulary, not a statement
    /// about the model.
    #[test]
    fn absent_capability_falls_back_to_the_known_label_ladder() {
        let set = reasoning_options(None);
        assert!(set.fallback, "an absent payload must read as a fallback");
        assert!(!set.always_reasons);
        for label in crate::core::KNOWN_REASONING_EFFORT_LABELS {
            assert!(
                set.options
                    .contains(&ReasoningMode::Effort(label.to_string())),
                "the fallback offers the known label '{label}'"
            );
        }
        assert_eq!(set.options.first(), Some(&ReasoningMode::Inherit));
        // The fallback is not the model's full option set: it is flagged, and
        // it carries no budget, because the known-label vocabulary declares
        // no budget contract.
        assert!(!set
            .options
            .iter()
            .any(|option| matches!(option, ReasoningMode::Budget(_))));
    }

    /// capability's order.
    #[test]
    fn established_capability_offers_the_capabilitys_labels_in_its_order() {
        let capability = capability(
            ReasoningCapabilityAxis::Supported,
            &["low", "medium", "high", "xhigh", "max"],
            ReasoningCapabilityAxis::Unsupported,
            ReasoningCapabilityAxis::Unknown,
        );
        let set = reasoning_options(Some(&capability));
        assert!(!set.fallback);
        assert_eq!(
            set.options,
            vec![
                ReasoningMode::Inherit,
                ReasoningMode::Effort("low".to_string()),
                ReasoningMode::Effort("medium".to_string()),
                ReasoningMode::Effort("high".to_string()),
                ReasoningMode::Effort("xhigh".to_string()),
                ReasoningMode::Effort("max".to_string()),
            ]
        );
    }

    /// A model that permits no disable offers no disable.
    #[test]
    fn capability_without_disable_offers_no_off_option() {
        let capability = capability(
            ReasoningCapabilityAxis::Supported,
            &["low", "high"],
            ReasoningCapabilityAxis::Unsupported,
            ReasoningCapabilityAxis::Unsupported,
        );
        let set = reasoning_options(Some(&capability));
        assert!(!set.allows_disable());
        assert!(!set.options.contains(&ReasoningMode::Off));
        assert!(!set.always_reasons);
    }

    /// A model that always reasons is told so, and gets no off switch even
    /// when the disable axis claims support.
    #[test]
    fn always_reasons_model_is_stated_and_offers_no_off_option() {
        let mut capability = capability(
            ReasoningCapabilityAxis::Supported,
            &["low", "high"],
            ReasoningCapabilityAxis::Supported,
            ReasoningCapabilityAxis::Unsupported,
        );
        capability.always_reasons = Some(true);
        let set = reasoning_options(Some(&capability));
        assert!(set.always_reasons, "the always-reasons case is stated");
        assert!(!set.allows_disable());
        assert!(!set.options.contains(&ReasoningMode::Off));
    }

    /// A budget outside the model's accepted range is not offered.
    #[test]
    fn budget_outside_the_models_bounds_is_not_offered() {
        let mut capability = capability(
            ReasoningCapabilityAxis::Unsupported,
            &[],
            ReasoningCapabilityAxis::Supported,
            ReasoningCapabilityAxis::Supported,
        );
        capability.min_budget_tokens = Some(1024);
        capability.max_budget_tokens = Some(4096);

        assert!(!capability.budget_within_bounds(512));
        assert!(capability.budget_within_bounds(2048));
        assert!(!capability.budget_within_bounds(8192));

        assert_eq!(reasoning_budget_option(Some(&capability), 512), None);
        assert_eq!(reasoning_budget_option(Some(&capability), 8192), None);
        assert_eq!(
            reasoning_budget_option(Some(&capability), 2048),
            Some(ReasoningMode::Budget(2048))
        );

        // The derived set offers only an in-bounds budget: the smallest
        // commitment the model accepts.
        let set = reasoning_options(Some(&capability));
        let budget = set
            .options
            .iter()
            .find_map(|option| match option {
                ReasoningMode::Budget(tokens) => Some(*tokens),
                _ => None,
            })
            .expect("an in-bounds budget option");
        assert_eq!(budget, 1024);
        assert!(capability.budget_within_bounds(budget as u64));
    }

    /// A model that accepts no budget is offered no budget, and an unknown
    /// budget dimension falls back to the known-label vocabulary, which
    /// declares no budget contract.
    #[test]
    fn unsupported_and_unknown_budget_dimensions_offer_no_budget() {
        let unsupported = capability(
            ReasoningCapabilityAxis::Supported,
            &["high"],
            ReasoningCapabilityAxis::Supported,
            ReasoningCapabilityAxis::Unsupported,
        );
        assert!(!reasoning_options(Some(&unsupported))
            .options
            .iter()
            .any(|option| matches!(option, ReasoningMode::Budget(_))));

        let unknown = capability(
            ReasoningCapabilityAxis::Supported,
            &["high"],
            ReasoningCapabilityAxis::Supported,
            ReasoningCapabilityAxis::Unknown,
        );
        assert!(!reasoning_options(Some(&unknown))
            .options
            .iter()
            .any(|option| matches!(option, ReasoningMode::Budget(_))));
        assert_eq!(reasoning_budget_option(Some(&unknown), 2048), None);
    }

    /// The frontend's option list is a subset of what the capability
    /// permits: it never widens it.
    #[test]
    fn option_set_is_a_subset_of_what_the_capability_permits() {
        let capability = capability(
            ReasoningCapabilityAxis::Supported,
            &["low", "high"],
            ReasoningCapabilityAxis::Supported,
            ReasoningCapabilityAxis::Supported,
        );
        let set = reasoning_options(Some(&capability));
        for option in &set.options {
            match option {
                ReasoningMode::Effort(label) => assert!(
                    capability.accepted_effort_labels.iter().any(|l| l == label),
                    "the frontend offered a label the capability does not publish"
                ),
                ReasoningMode::Budget(tokens) => assert!(
                    capability.budget_within_bounds(*tokens as u64),
                    "the frontend offered a budget outside the model's range"
                ),
                ReasoningMode::Off => assert!(capability.disable.is_supported()),
                ReasoningMode::Inherit => {}
            }
        }
    }
}
