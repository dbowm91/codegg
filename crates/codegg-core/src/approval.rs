//! Approval routing domain and durable runtime preferences (M003).
//!
//! This module owns the frontend-neutral approval/sandbox vocabulary and the
//! daemon-owned principal-scoped preference store. It is deliberately
//! UI/server/plugin/auth-free: it stores only non-secret preference strings
//! (`interactive`/`automatic`/`yolo`, sandbox profile names, connection and
//! model identity) plus revision/timestamp metadata. Credentials, prompts,
//! tool output, and filesystem secrets never enter this store.
//!
//! Long-term references: `plans/000-long-term-specification.md`,
//! ADR-0004 (`plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`),
//! and the execution-reliability roadmap M003.

use codegg_protocol::core::{
    ReasoningOverride, MAX_REASONING_BUDGET_TOKENS, MAX_REASONING_LABEL_BYTES,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::StorageError;

/// User-facing approval mode. Closed enum per ADR-0004.
///
/// - `Interactive`: escalations go to the human/frontend.
/// - `Automatic`: escalations are reviewed by the dedicated bounded reviewer
///   (M006). Until M006 lands, callers must defer to the human.
/// - `Yolo`: escalations are accepted automatically within the already
///   resolved authority/sandbox ceiling. Never overrides an explicit deny.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    #[default]
    Interactive,
    Automatic,
    Yolo,
}

impl ApprovalMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Automatic => "automatic",
            Self::Yolo => "yolo",
        }
    }

    /// Parse a wire/config mode name. Fails closed on unknown input.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "interactive" => Some(Self::Interactive),
            "automatic" | "auto" => Some(Self::Automatic),
            "yolo" => Some(Self::Yolo),
            _ => None,
        }
    }

    /// Autonomy rank for child-ceiling checks. Higher means more autonomous.
    const fn rank(self) -> u8 {
        match self {
            Self::Interactive => 0,
            Self::Automatic => 1,
            Self::Yolo => 2,
        }
    }

    /// `true` when the mode may resolve an escalation without a human.
    pub const fn allows_auto_resolve(self) -> bool {
        matches!(self, Self::Yolo)
    }
}

/// Filesystem containment profile. Orthogonal to [`ApprovalMode`] per
/// ADR-0004: changing the approval mode never changes containment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxProfile {
    ReadOnly,
    #[default]
    WorkspaceWrite,
    FullHost,
}

impl SandboxProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::WorkspaceWrite => "workspace_write",
            Self::FullHost => "full_host",
        }
    }

    /// Parse a wire/config profile name. Fails closed on unknown input.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "read_only" | "readonly" | "read-only" => Some(Self::ReadOnly),
            "workspace_write" | "workspace-write" => Some(Self::WorkspaceWrite),
            "full_host" | "full-host" | "fullhost" => Some(Self::FullHost),
            _ => None,
        }
    }

    const fn rank(self) -> u8 {
        match self {
            Self::ReadOnly => 0,
            Self::WorkspaceWrite => 1,
            Self::FullHost => 2,
        }
    }

    /// `true` when the profile requests OS filesystem containment
    /// (`ReadOnly`/`WorkspaceWrite`). `FullHost` intentionally requests
    /// no CodeGG filesystem containment.
    pub const fn requires_filesystem_containment(self) -> bool {
        !matches!(self, Self::FullHost)
    }

    /// `true` only for the explicit no-containment profile.
    pub const fn is_full_host(self) -> bool {
        matches!(self, Self::FullHost)
    }

    /// The more contained of two profiles.
    ///
    /// `ReadOnly` is narrower than `WorkspaceWrite`, which is narrower than
    /// `FullHost`. Combining a carried profile with a ceiling therefore
    /// never widens: the result keeps whichever of the two is more
    /// contained.
    pub const fn narrow(self, other: Self) -> Self {
        if other.rank() < self.rank() {
            other
        } else {
            self
        }
    }

    /// Legacy `SandboxMode` config name for compatibility mapping.
    ///
    /// `ReadOnly`/`WorkspaceWrite` map 1:1. `FullHost` maps to the
    /// historic `danger_full_access` string, but the semantics differ:
    /// legacy `DangerFullAccess` behaved as another writable-roots mode,
    /// while `FullHost` means no CodeGG filesystem containment at all.
    /// Callers must not reinterpret old `danger_full_access` config as
    /// `FullHost` without explicit user confirmation.
    pub const fn legacy_sandbox_mode_name(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::WorkspaceWrite => "workspace_write",
            Self::FullHost => "danger_full_access",
        }
    }
}

/// Network policy for child execution. Offline is an independent principal
/// preference and never follows implicitly from the filesystem profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkIntent {
    #[default]
    LegacyUnrestricted,
    OfflineRequired,
}

impl NetworkIntent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyUnrestricted => "legacy_unrestricted",
            Self::OfflineRequired => "offline_required",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "legacy_unrestricted" | "unrestricted" => Some(Self::LegacyUnrestricted),
            "offline_required" | "offline" => Some(Self::OfflineRequired),
            _ => None,
        }
    }
}

/// Obtained filesystem enforcement for one execution context (M005).
///
/// This is the *obtained* fact, distinct from the *requested*
/// [`SandboxProfile`]. A constrained request that cannot be enforced
/// must surface [`FilesystemEnforcement::Unavailable`] (fail closed),
/// never a silent fallback to [`FilesystemEnforcement::FullHost`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemEnforcement {
    /// OS filesystem containment is active.
    Enforced { backend: String, abi: Option<u32> },
    /// Containment was requested but is not available on this host.
    Unavailable { reason: String },
    /// No CodeGG filesystem containment (explicit `FullHost` only).
    FullHost,
}

impl FilesystemEnforcement {
    pub fn enforced(backend: impl Into<String>, abi: Option<u32>) -> Self {
        let backend = truncate_to(backend.into(), 64);
        Self::Enforced {
            backend: if backend.is_empty() {
                "unknown".to_string()
            } else {
                backend
            },
            abi,
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: truncate_to(reason.into(), 512),
        }
    }

    pub const fn is_enforced(&self) -> bool {
        matches!(self, Self::Enforced { .. })
    }

    pub const fn is_full_host(&self) -> bool {
        matches!(self, Self::FullHost)
    }
}

/// Obtained network containment for one execution context (M005).
///
/// A network boundary is only ever reported when it was **obtained**.
///
/// The legacy Landlock route provides no network dimension, so a constrained
/// launch on that route truthfully reports
/// [`NetworkEnforcement::Unrestricted`]. The Bubblewrap route can place the
/// child in a private network namespace, and when it does, the launch reports
/// [`NetworkEnforcement::Enforced`] — the M005 offline-network candidate.
///
/// The distinction matters because "filesystem success never implies network
/// isolation": the two dimensions are obtained separately and are reported
/// separately. Permission/security policy may still classify network
/// commands, but that is not OS isolation and must never be rendered as
/// "sandboxed network".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkEnforcement {
    /// OS network containment is active. Produced by a launch that observed
    /// its own sandbox setup completing with the network namespace requested.
    Enforced { backend: String },
    /// No OS network isolation. The truthful state for a launch that did not
    /// obtain a network namespace.
    Unrestricted,
    /// Network state could not be determined.
    Unavailable { reason: String },
}

impl NetworkEnforcement {
    pub const fn is_enforced(&self) -> bool {
        matches!(self, Self::Enforced { .. })
    }

    /// A stable, human-readable name for the obtained boundary.
    ///
    /// The name states the *fact*, never the request: `unrestricted` means no
    /// network namespace was obtained, not that none was asked for.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Enforced { .. } => "enforced",
            Self::Unrestricted => "unrestricted",
            Self::Unavailable { .. } => "unavailable",
        }
    }

    /// The mechanism that obtained the boundary, when one did.
    pub fn backend(&self) -> Option<&str> {
        match self {
            Self::Enforced { backend } => Some(backend.as_str()),
            _ => None,
        }
    }
}

/// Requested profile plus obtained filesystem/network enforcement (M005).
///
/// `requested` is the daemon-resolved [`SandboxProfile`];
/// `filesystem`/`network` are the separately reported facts per ADR-0004
/// ("Landlock does not imply network isolation").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxEnforcement {
    pub requested: SandboxProfile,
    pub filesystem: FilesystemEnforcement,
    pub network: NetworkEnforcement,
}

impl SandboxEnforcement {
    /// Explicit `FullHost`: no CodeGG filesystem containment, network
    /// unrestricted. Only constructed for an explicit `FullHost` request.
    pub fn for_full_host() -> Self {
        Self {
            requested: SandboxProfile::FullHost,
            filesystem: FilesystemEnforcement::FullHost,
            network: NetworkEnforcement::Unrestricted,
        }
    }

    /// Constrained request with active OS containment.
    ///
    /// This is the **profile** on the host, not a launch. Its network field
    /// is the profile's network posture: filesystem success never implies
    /// network isolation. An *obtained* network boundary belongs to a
    /// launch, which reports it on its own outcome — see
    /// `NetworkEnforcement`.
    pub fn for_constrained_enforced(
        requested: SandboxProfile,
        backend: impl Into<String>,
        abi: Option<u32>,
    ) -> Self {
        debug_assert!(requested.requires_filesystem_containment());
        Self {
            requested,
            filesystem: FilesystemEnforcement::enforced(backend, abi),
            network: NetworkEnforcement::Unrestricted,
        }
    }

    /// Constrained request without OS containment (unsupported host,
    /// missing helper, setup failure). Fail-closed signal: callers must
    /// deny the action or escalate to the user, never run as `FullHost`.
    pub fn for_constrained_unavailable(
        requested: SandboxProfile,
        reason: impl Into<String>,
    ) -> Self {
        debug_assert!(requested.requires_filesystem_containment());
        Self {
            requested,
            filesystem: FilesystemEnforcement::Unavailable {
                reason: truncate_to(reason.into(), 512),
            },
            network: NetworkEnforcement::Unrestricted,
        }
    }

    pub fn is_enforced(&self) -> bool {
        self.filesystem.is_enforced()
    }

    pub fn is_full_host(&self) -> bool {
        matches!(self.requested, SandboxProfile::FullHost) && self.filesystem.is_full_host()
    }

    /// Short human-readable summary for status lines and diagnostics.
    /// Never renders filesystem success as network containment.
    pub fn describe(&self) -> String {
        let fs = match &self.filesystem {
            FilesystemEnforcement::Enforced { backend, abi } => match abi {
                Some(abi) => format!("filesystem enforced ({backend} abi {abi})"),
                None => format!("filesystem enforced ({backend})"),
            },
            FilesystemEnforcement::Unavailable { reason } => {
                format!("filesystem unavailable ({reason})")
            }
            FilesystemEnforcement::FullHost => {
                "no CodeGG filesystem containment (explicit FullHost)".to_string()
            }
        };
        let net = match &self.network {
            NetworkEnforcement::Enforced { backend } => {
                format!("network enforced ({backend})")
            }
            NetworkEnforcement::Unrestricted => {
                "network unrestricted (no OS isolation)".to_string()
            }
            NetworkEnforcement::Unavailable { reason } => {
                format!("network unknown ({reason})")
            }
        };
        format!("{} requested; {fs}; {net}", self.requested.as_str(),)
    }
}

/// Bounded escalation hint when a command genuinely needs a path outside
/// the current `WorkspaceWrite` roots (M005 §6).
///
/// M005 does not implement a temporary-expansion store; the correct
/// behavior is to deny with this typed hint (or route through
/// `ApprovalRouter` as an escalation) rather than silently switching
/// the whole turn to `FullHost`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxEscalationRequest {
    pub requested_path: String,
    pub profile: SandboxProfile,
    pub reason: String,
}

impl SandboxEscalationRequest {
    pub fn new(
        requested_path: impl Into<String>,
        profile: SandboxProfile,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            requested_path: truncate_to(requested_path.into(), 1024),
            profile,
            reason: truncate_to(reason.into(), 512),
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "sandbox escalation: path '{}' is outside {} roots ({})",
            self.requested_path,
            self.profile.as_str(),
            self.reason,
        )
    }
}

/// Resolve a child sandbox profile against its parent ceiling (M005 §6).
///
/// Thin named wrapper over [`child_sandbox_allowed`] so subagent
/// construction reads as policy, not a bare boolean. `FullHost` under a
/// `WorkspaceWrite`/`ReadOnly` parent fails with [`PreferenceError::CeilingExceeded`].
pub fn resolve_child_sandbox(
    parent: SandboxProfile,
    requested: SandboxProfile,
) -> Result<SandboxProfile, PreferenceError> {
    if child_sandbox_allowed(parent, requested) {
        Ok(requested)
    } else {
        Err(PreferenceError::CeilingExceeded(format!(
            "child sandbox {} exceeds parent {}",
            requested.as_str(),
            parent.as_str()
        )))
    }
}

fn truncate_to(mut value: String, max_len: usize) -> String {
    if value.len() > max_len {
        value.truncate(max_len);
    }
    value.replace('\0', "")
}

/// A child loop may use a mode no broader than its parent effective mode.
pub fn child_mode_allowed(parent: ApprovalMode, child: ApprovalMode) -> bool {
    child.rank() <= parent.rank()
}

/// A child loop may use a sandbox profile no broader than its parent.
pub fn child_sandbox_allowed(parent: SandboxProfile, child: SandboxProfile) -> bool {
    child.rank() <= parent.rank()
}

/// Resolve the effective approval mode per ADR-0004 precedence:
///
/// explicit invocation/session/turn override > project/admin ceiling >
/// persisted principal preference > configured/built-in default.
///
/// `project_ceiling` is the most autonomous mode the project/admin policy
/// permits (`None` means no additional ceiling). The result never exceeds
/// the ceiling: when the preferred mode is broader than the ceiling, the
/// ceiling wins. A preference never overrides a deny or authority ceiling;
/// deny handling lives above this helper.
pub fn resolve_effective_mode(
    explicit_override: Option<ApprovalMode>,
    project_ceiling: Option<ApprovalMode>,
    persisted: Option<ApprovalMode>,
    configured_default: ApprovalMode,
) -> ApprovalMode {
    let preferred = explicit_override
        .or(persisted)
        .unwrap_or(configured_default);
    match project_ceiling {
        Some(ceiling) if preferred.rank() > ceiling.rank() => ceiling,
        _ => preferred,
    }
}

/// Immutable effective execution-policy snapshot captured at the
/// turn/accepted-tool-batch boundary.
///
/// Mode changes from another frontend apply on the next safe turn boundary
/// and cannot retroactively bless a pending action: the router resolves
/// every escalation against the snapshot captured when the batch was
/// accepted, never against live mutable state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPolicySnapshot {
    approval_mode: ApprovalMode,
    sandbox_profile: SandboxProfile,
    #[serde(default)]
    network_intent: NetworkIntent,
    #[serde(default)]
    principal_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    policy_revision: Option<String>,
    #[serde(default)]
    reviewer_config_id: Option<String>,
    captured_at_ms: i64,
}

impl ExecutionPolicySnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        approval_mode: ApprovalMode,
        sandbox_profile: SandboxProfile,
        principal_id: Option<String>,
        session_id: Option<String>,
        agent_id: Option<String>,
        policy_revision: Option<String>,
        reviewer_config_id: Option<String>,
    ) -> Self {
        Self {
            approval_mode,
            sandbox_profile,
            network_intent: NetworkIntent::LegacyUnrestricted,
            principal_id: bounded_opt(principal_id, 256),
            session_id: bounded_opt(session_id, 256),
            agent_id: bounded_opt(agent_id, 256),
            policy_revision: bounded_opt(policy_revision, 256),
            reviewer_config_id: bounded_opt(reviewer_config_id, 256),
            captured_at_ms: now_millis(),
        }
    }

    /// Default snapshot for harnesses and pool-less paths.
    pub fn default_snapshot() -> Self {
        Self::capture(
            ApprovalMode::Interactive,
            SandboxProfile::WorkspaceWrite,
            None,
            None,
            None,
            None,
            None,
        )
    }

    pub const fn approval_mode(&self) -> ApprovalMode {
        self.approval_mode
    }

    pub const fn sandbox_profile(&self) -> SandboxProfile {
        self.sandbox_profile
    }

    pub const fn network_intent(&self) -> NetworkIntent {
        self.network_intent
    }

    pub fn with_network_intent(
        mut self,
        network_intent: NetworkIntent,
    ) -> Result<Self, PreferenceError> {
        if self.network_intent == NetworkIntent::OfflineRequired
            && network_intent != NetworkIntent::OfflineRequired
        {
            return Err(PreferenceError::CeilingExceeded(
                "child network intent exceeds the OfflineRequired parent ceiling".into(),
            ));
        }
        self.network_intent = network_intent;
        Ok(self)
    }

    pub fn principal_id(&self) -> Option<&str> {
        self.principal_id.as_deref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }

    pub fn policy_revision(&self) -> Option<&str> {
        self.policy_revision.as_deref()
    }

    pub fn reviewer_config_id(&self) -> Option<&str> {
        self.reviewer_config_id.as_deref()
    }

    pub fn captured_at_ms(&self) -> i64 {
        self.captured_at_ms
    }

    pub const fn is_yolo(&self) -> bool {
        matches!(self.approval_mode, ApprovalMode::Yolo)
    }

    pub const fn is_automatic(&self) -> bool {
        matches!(self.approval_mode, ApprovalMode::Automatic)
    }

    /// Narrow this snapshot for a child loop. Fails when the child requests
    /// a broader mode or sandbox profile than the parent ceiling.
    pub fn narrow_for_child(
        &self,
        child_mode: ApprovalMode,
        child_sandbox: SandboxProfile,
    ) -> Result<Self, PreferenceError> {
        if !child_mode_allowed(self.approval_mode, child_mode) {
            return Err(PreferenceError::CeilingExceeded(format!(
                "child approval mode {} exceeds parent {}",
                child_mode.as_str(),
                self.approval_mode.as_str()
            )));
        }
        if !child_sandbox_allowed(self.sandbox_profile, child_sandbox) {
            return Err(PreferenceError::CeilingExceeded(format!(
                "child sandbox {} exceeds parent {}",
                child_sandbox.as_str(),
                self.sandbox_profile.as_str()
            )));
        }
        Ok(Self {
            approval_mode: child_mode,
            sandbox_profile: child_sandbox,
            network_intent: self.network_intent,
            principal_id: self.principal_id.clone(),
            session_id: self.session_id.clone(),
            agent_id: self.agent_id.clone(),
            policy_revision: self.policy_revision.clone(),
            reviewer_config_id: self.reviewer_config_id.clone(),
            captured_at_ms: now_millis(),
        })
    }
}

fn bounded_opt(value: Option<String>, max_len: usize) -> Option<String> {
    value.and_then(|v| {
        let t = v.trim().to_owned();
        if t.is_empty() || t.len() > max_len || t.contains('\0') {
            None
        } else {
            Some(t)
        }
    })
}

/// Daemon-owned principal-scoped runtime preference.
///
/// Secret-free by construction: only mode/profile names plus stable
/// connection/model identity strings. `revision` implements optimistic
/// concurrency between frontends; `updated_at_ms` is diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePreference {
    pub principal_id: String,
    pub approval_mode: Option<ApprovalMode>,
    pub sandbox_profile: Option<SandboxProfile>,
    /// Operator opt-in for child network isolation. Missing rows/values
    /// preserve the legacy unrestricted default.
    pub network_intent: Option<NetworkIntent>,
    /// Reserved for M004: last-used provider connection identity.
    pub last_provider_connection_id: Option<String>,
    /// Reserved for M004: last-used model identity.
    pub last_model_id: Option<String>,
    /// Project Work Orders M003: last Task-composer provider connection
    /// identity. Separately scoped from the ordinary session
    /// (`last_provider_connection_id`/`last_model_id`) convenience
    /// preference: Task-mode selection reads/writes only these fields and
    /// ordinary session selection never touches them.
    pub last_task_provider_connection_id: Option<String>,
    /// Project Work Orders M003: last Task-composer model identity. A
    /// convenience default snapshotted into `WorkOrderCreateRequest` at
    /// creation time; never execution authority.
    pub last_task_model_id: Option<String>,
    /// Provider reasoning control M002: the daemon-owned, principal-scoped
    /// extended-thinking selection.
    ///
    /// `None` means "no explicit selection": the turn then inherits the
    /// agent / model-profile default. An `Effort` selection carries an
    /// **open, provider-scoped** label verbatim — it is never folded onto
    /// one of the known convenience labels.
    pub reasoning_selection: Option<ReasoningOverride>,
    pub revision: u64,
    pub updated_at_ms: i64,
}

impl RuntimePreference {
    pub fn effective_approval_mode(&self) -> ApprovalMode {
        self.approval_mode.unwrap_or_default()
    }

    pub fn effective_sandbox_profile(&self) -> SandboxProfile {
        self.sandbox_profile.unwrap_or_default()
    }

    pub fn effective_network_intent(&self) -> NetworkIntent {
        self.network_intent.unwrap_or_default()
    }

    /// `true` when a last-used model preference is present (both
    /// connection and model identity). M004 convenience default for
    /// otherwise unselected sessions; never overrides an explicit
    /// session selection.
    pub fn has_model_preference(&self) -> bool {
        self.last_provider_connection_id
            .as_deref()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
            && self
                .last_model_id
                .as_deref()
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
    }

    /// Project Work Orders M003: `true` when a Task-composer model
    /// preference is present. Scoped independently of
    /// [`Self::has_model_preference`]: writing the ordinary session
    /// preference never sets this, and writing the Task preference never
    /// sets the ordinary one. A remembered Task model that no longer
    /// exists falls back to the normal current/default selection before
    /// creation (with a visible notice); an already-created WorkOrder
    /// never silently changes model.
    pub fn has_task_model_preference(&self) -> bool {
        self.last_task_provider_connection_id
            .as_deref()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
            && self
                .last_task_model_id
                .as_deref()
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
    }
}

/// M004: bounded outcome of applying a principal's last-used
/// connection/model preference to a session with no explicit durable
/// selection.
///
/// The preference stores no catalog revision as lasting authority;
/// the catalog is re-resolved at use. No variant ever authorizes a
/// silent switch to a different provider connection or model: invalid
/// preferences keep the session unselected and surface a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreferenceApplicationOutcome {
    /// Durable selection was applied with current revisions.
    Applied {
        connection_id: String,
        model_id: String,
    },
    /// The session already carries an explicit durable selection;
    /// the preference was not consulted.
    ExplicitSelectionPresent,
    /// No usable preference exists for this principal.
    NoPreference,
    /// The remembered connection is missing, not active, or not
    /// credential-ready. The session is left unselected.
    UnavailableConnection { reason: String },
    /// The remembered model is not in the connection's current
    /// bounded catalog. The session is left unselected.
    UnknownModel {
        connection_id: String,
        model_id: String,
    },
    /// The catalog revision moved during application. The session is
    /// left unchanged; the caller must reload and retry explicitly.
    StaleCatalog { detail: String },
}

impl PreferenceApplicationOutcome {
    /// Stable wire/diagnostic code for frontend-neutral reporting.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Applied { .. } => "preference_applied",
            Self::ExplicitSelectionPresent => "explicit_selection_present",
            Self::NoPreference => "no_preference",
            Self::UnavailableConnection { .. } => "preference_connection_unavailable",
            Self::UnknownModel { .. } => "preference_unknown_model",
            Self::StaleCatalog { .. } => "preference_catalog_stale",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Applied {
                connection_id,
                model_id,
            } => format!(
                "Applied last-used preference {connection_id}/{model_id} with current revisions."
            ),
            Self::ExplicitSelectionPresent => {
                "Session already has an explicit selection; preference not applied.".to_string()
            }
            Self::NoPreference => "No last-used model preference for this principal.".to_string(),
            Self::UnavailableConnection { reason } => format!(
                "Remembered provider connection is unavailable ({reason}); session left unselected."
            ),
            Self::UnknownModel {
                connection_id,
                model_id,
            } => format!(
                "Remembered model '{model_id}' is not in the current catalog of connection '{connection_id}'; session left unselected."
            ),
            Self::StaleCatalog { detail } => format!(
                "Catalog moved during preference application ({detail}); session left unchanged."
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PreferenceError {
    #[error("invalid preference: {0}")]
    Validation(String),
    #[error("preference revision conflict: expected {expected}, current {current}")]
    Conflict { expected: u64, current: u64 },
    #[error("approval ceiling exceeded: {0}")]
    CeilingExceeded(String),
    #[error("preference storage error: {0}")]
    Storage(String),
}

impl From<StorageError> for PreferenceError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value.to_string())
    }
}

/// Daemon-owned principal-scoped bounded preference store.
///
/// Backed by the `runtime_preferences` SQLite table (migration v58).
/// Additive and empty on upgrade; pre-preference databases open cleanly.
#[derive(Clone)]
pub struct RuntimePreferenceStore {
    pool: SqlitePool,
}

impl RuntimePreferenceStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Read the preference row for `principal_id`. Unknown mode/profile
    /// strings in a corrupt row degrade conservatively to `None` (the
    /// caller falls back to `Interactive`/`WorkspaceWrite`) rather than
    /// failing the whole read. A corrupt reasoning selection degrades the
    /// same way: it is not authority, so it reads as "no explicit
    /// selection" instead of failing the whole read.
    pub async fn get(
        &self,
        principal_id: &str,
    ) -> Result<Option<RuntimePreference>, PreferenceError> {
        validate_principal_id(principal_id)?;
        let row: Option<RuntimePreferenceRow> = sqlx::query_as(
            r#"
            SELECT principal_id, approval_mode, sandbox_profile, network_intent,
                   last_provider_connection_id, last_model_id,
                   last_task_provider_connection_id, last_task_model_id,
                   reasoning_selection,
                   revision, updated_at
            FROM runtime_preferences WHERE principal_id = ?1
            "#,
        )
        .bind(principal_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PreferenceError::Storage(e.to_string()))?;
        Ok(row.map(|r| r.into_preference()))
    }

    /// Persist the approval mode with optimistic concurrency. `None`
    /// revision means "create or overwrite"; `Some(expected)` requires the
    /// stored revision to match and returns [`PreferenceError::Conflict`]
    /// otherwise (stale frontend must reload, never last-write-wins).
    pub async fn set_approval_mode(
        &self,
        principal_id: &str,
        mode: ApprovalMode,
        expected_revision: Option<u64>,
    ) -> Result<RuntimePreference, PreferenceError> {
        validate_principal_id(principal_id)?;
        self.write_field(
            principal_id,
            "approval_mode",
            Some(mode.as_str()),
            expected_revision,
        )
        .await
    }

    pub async fn set_sandbox_profile(
        &self,
        principal_id: &str,
        profile: SandboxProfile,
        expected_revision: Option<u64>,
    ) -> Result<RuntimePreference, PreferenceError> {
        validate_principal_id(principal_id)?;
        self.write_field(
            principal_id,
            "sandbox_profile",
            Some(profile.as_str()),
            expected_revision,
        )
        .await
    }

    pub async fn set_network_intent(
        &self,
        principal_id: &str,
        network_intent: NetworkIntent,
        expected_revision: Option<u64>,
    ) -> Result<RuntimePreference, PreferenceError> {
        validate_principal_id(principal_id)?;
        self.write_field(
            principal_id,
            "network_intent",
            Some(network_intent.as_str()),
            expected_revision,
        )
        .await
    }

    /// M007: atomically persist approval mode and/or sandbox profile in a
    /// single revision bump.
    ///
    /// `None` leaves that dimension unchanged; at least one dimension must
    /// be `Some` (otherwise [`PreferenceError::Validation`]). Both values
    /// are validated before anything is written (fail closed), and the
    /// optimistic-concurrency check runs once against the current row, so
    /// two frontends racing a two-dimension change converge on one winner
    /// via [`PreferenceError::Conflict`] instead of interleaving separate
    /// single-field writes.
    pub async fn set_policy(
        &self,
        principal_id: &str,
        mode: Option<ApprovalMode>,
        profile: Option<SandboxProfile>,
        expected_revision: Option<u64>,
    ) -> Result<RuntimePreference, PreferenceError> {
        validate_principal_id(principal_id)?;
        if mode.is_none() && profile.is_none() {
            return Err(PreferenceError::Validation(
                "set_policy requires an approval mode and/or a sandbox profile".into(),
            ));
        }
        let current = self.get(principal_id).await?;
        let current_revision = current.as_ref().map(|p| p.revision).unwrap_or(0);
        if let Some(expected) = expected_revision {
            if expected != current_revision {
                return Err(PreferenceError::Conflict {
                    expected,
                    current: current_revision,
                });
            }
        }
        let next_revision = current_revision.saturating_add(1).max(1);
        let now = now_millis();
        let approval_mode = match (mode, current.as_ref()) {
            (Some(m), _) => Some(m.as_str().to_owned()),
            (None, Some(p)) => p.approval_mode.map(|m| m.as_str().to_owned()),
            (None, None) => None,
        };
        let sandbox_profile = match (profile, current.as_ref()) {
            (Some(s), _) => Some(s.as_str().to_owned()),
            (None, Some(p)) => p.sandbox_profile.map(|s| s.as_str().to_owned()),
            (None, None) => None,
        };
        let (connection_id, model_id) = match current.as_ref() {
            Some(p) => (
                p.last_provider_connection_id.clone(),
                p.last_model_id.clone(),
            ),
            None => (None, None),
        };
        // M003 Task-composer preference survives policy writes untouched.
        let (task_connection_id, task_model_id) = match current.as_ref() {
            Some(p) => (
                p.last_task_provider_connection_id.clone(),
                p.last_task_model_id.clone(),
            ),
            None => (None, None),
        };
        // M002 reasoning selection survives policy writes untouched.
        let reasoning_selection = current_reasoning_selection(current.as_ref());
        sqlx::query(
            r#"
            INSERT INTO runtime_preferences
                (principal_id, approval_mode, sandbox_profile,
                 last_provider_connection_id, last_model_id,
                 last_task_provider_connection_id, last_task_model_id,
                 reasoning_selection,
                 revision, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(principal_id) DO UPDATE SET
                approval_mode = excluded.approval_mode,
                sandbox_profile = excluded.sandbox_profile,
                revision = excluded.revision,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(principal_id)
        .bind(approval_mode)
        .bind(sandbox_profile)
        .bind(connection_id)
        .bind(model_id)
        .bind(task_connection_id)
        .bind(task_model_id)
        .bind(reasoning_selection)
        .bind(next_revision as i64)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| PreferenceError::Storage(e.to_string()))?;
        self.get(principal_id)
            .await?
            .ok_or_else(|| PreferenceError::Storage("preference write lost".into()))
    }

    /// Reserved M004 contract: persist last-used connection/model identity.
    /// Re-resolved against the provider catalog on use; a stale identity
    /// never authorizes a silent provider/model switch (that check lives in
    /// the selection service, not here).
    pub async fn set_model_preference(
        &self,
        principal_id: &str,
        connection_id: Option<&str>,
        model_id: Option<&str>,
        expected_revision: Option<u64>,
    ) -> Result<RuntimePreference, PreferenceError> {
        validate_principal_id(principal_id)?;
        if let Some(v) = connection_id {
            validate_identity_field("last_provider_connection_id", v)?;
        }
        if let Some(v) = model_id {
            validate_identity_field("last_model_id", v)?;
        }
        let current = self.get(principal_id).await?;
        let (current_revision, current_row) = match current {
            Some(p) => (p.revision, Some(p)),
            None => (0, None),
        };
        if let Some(expected) = expected_revision {
            if expected != current_revision {
                return Err(PreferenceError::Conflict {
                    expected,
                    current: current_revision,
                });
            }
        }
        let next_revision = current_revision.saturating_add(1).max(1);
        let now = now_millis();
        let approval_mode = current_row
            .as_ref()
            .and_then(|p| p.approval_mode.map(|m| m.as_str().to_owned()));
        let sandbox_profile = current_row
            .as_ref()
            .and_then(|p| p.sandbox_profile.map(|s| s.as_str().to_owned()));
        // M003 Task-composer preference survives ordinary model writes.
        let (task_connection_id, task_model_id) = match current_row.as_ref() {
            Some(p) => (
                p.last_task_provider_connection_id.clone(),
                p.last_task_model_id.clone(),
            ),
            None => (None, None),
        };
        // M002 reasoning selection survives ordinary model writes.
        let reasoning_selection = current_reasoning_selection(current_row.as_ref());
        sqlx::query(
            r#"
            INSERT INTO runtime_preferences
                (principal_id, approval_mode, sandbox_profile,
                 last_provider_connection_id, last_model_id,
                 last_task_provider_connection_id, last_task_model_id,
                 reasoning_selection,
                 revision, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(principal_id) DO UPDATE SET
                last_provider_connection_id = excluded.last_provider_connection_id,
                last_model_id = excluded.last_model_id,
                revision = excluded.revision,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(principal_id)
        .bind(approval_mode)
        .bind(sandbox_profile)
        .bind(connection_id)
        .bind(model_id)
        .bind(task_connection_id)
        .bind(task_model_id)
        .bind(reasoning_selection)
        .bind(next_revision as i64)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| PreferenceError::Storage(e.to_string()))?;
        self.get(principal_id)
            .await?
            .ok_or_else(|| PreferenceError::Storage("preference write lost".into()))
    }

    /// Project Work Orders M003: persist the Task-composer last-used
    /// connection/model identity in its own scope.
    ///
    /// Re-resolved against the provider catalog on use; a stale identity
    /// never authorizes a silent provider/model switch (that check lives in
    /// the composer, not here). Writing the Task preference never touches
    /// the ordinary session preference (and vice versa); approval/sandbox
    /// dimensions are preserved untouched.
    pub async fn set_task_model_preference(
        &self,
        principal_id: &str,
        connection_id: Option<&str>,
        model_id: Option<&str>,
        expected_revision: Option<u64>,
    ) -> Result<RuntimePreference, PreferenceError> {
        validate_principal_id(principal_id)?;
        if let Some(v) = connection_id {
            validate_identity_field("last_task_provider_connection_id", v)?;
        }
        if let Some(v) = model_id {
            validate_identity_field("last_task_model_id", v)?;
        }
        let current = self.get(principal_id).await?;
        let (current_revision, current_row) = match current {
            Some(p) => (p.revision, Some(p)),
            None => (0, None),
        };
        if let Some(expected) = expected_revision {
            if expected != current_revision {
                return Err(PreferenceError::Conflict {
                    expected,
                    current: current_revision,
                });
            }
        }
        let next_revision = current_revision.saturating_add(1).max(1);
        let now = now_millis();
        let approval_mode = current_row
            .as_ref()
            .and_then(|p| p.approval_mode.map(|m| m.as_str().to_owned()));
        let sandbox_profile = current_row
            .as_ref()
            .and_then(|p| p.sandbox_profile.map(|s| s.as_str().to_owned()));
        let (session_connection_id, session_model_id) = match current_row.as_ref() {
            Some(p) => (
                p.last_provider_connection_id.clone(),
                p.last_model_id.clone(),
            ),
            None => (None, None),
        };
        // M002 reasoning selection survives Task-composer writes.
        let reasoning_selection = current_reasoning_selection(current_row.as_ref());
        sqlx::query(
            r#"
            INSERT INTO runtime_preferences
                (principal_id, approval_mode, sandbox_profile,
                 last_provider_connection_id, last_model_id,
                 last_task_provider_connection_id, last_task_model_id,
                 reasoning_selection,
                 revision, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(principal_id) DO UPDATE SET
                last_task_provider_connection_id = excluded.last_task_provider_connection_id,
                last_task_model_id = excluded.last_task_model_id,
                revision = excluded.revision,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(principal_id)
        .bind(approval_mode)
        .bind(sandbox_profile)
        .bind(session_connection_id)
        .bind(session_model_id)
        .bind(connection_id)
        .bind(model_id)
        .bind(reasoning_selection)
        .bind(next_revision as i64)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| PreferenceError::Storage(e.to_string()))?;
        self.get(principal_id)
            .await?
            .ok_or_else(|| PreferenceError::Storage("preference write lost".into()))
    }

    async fn write_field(
        &self,
        principal_id: &str,
        field: &str,
        value: Option<&str>,
        expected_revision: Option<u64>,
    ) -> Result<RuntimePreference, PreferenceError> {
        let current = self.get(principal_id).await?;
        let current_revision = current.as_ref().map(|p| p.revision).unwrap_or(0);
        if let Some(expected) = expected_revision {
            if expected != current_revision {
                return Err(PreferenceError::Conflict {
                    expected,
                    current: current_revision,
                });
            }
        }
        let next_revision = current_revision.saturating_add(1).max(1);
        let now = now_millis();
        let (approval_mode, sandbox_profile, network_intent) = match current.as_ref() {
            Some(p) => (
                p.approval_mode.map(|m| m.as_str().to_owned()),
                p.sandbox_profile.map(|s| s.as_str().to_owned()),
                p.network_intent.map(|intent| intent.as_str().to_owned()),
            ),
            None => (None, None, None),
        };
        let (approval_mode, sandbox_profile, network_intent) = match field {
            "approval_mode" => (value.map(str::to_owned), sandbox_profile, network_intent),
            "sandbox_profile" => (approval_mode, value.map(str::to_owned), network_intent),
            "network_intent" => (approval_mode, sandbox_profile, value.map(str::to_owned)),
            _ => return Err(PreferenceError::Validation("unknown field".into())),
        };
        let (connection_id, model_id) = match current.as_ref() {
            Some(p) => (
                p.last_provider_connection_id.clone(),
                p.last_model_id.clone(),
            ),
            None => (None, None),
        };
        // M003 Task-composer preference survives single-field writes.
        let (task_connection_id, task_model_id) = match current.as_ref() {
            Some(p) => (
                p.last_task_provider_connection_id.clone(),
                p.last_task_model_id.clone(),
            ),
            None => (None, None),
        };
        // M002 reasoning selection survives single-field writes.
        let reasoning_selection = current_reasoning_selection(current.as_ref());
        sqlx::query(
            r#"
            INSERT INTO runtime_preferences
                (principal_id, approval_mode, sandbox_profile, network_intent,
                 last_provider_connection_id, last_model_id,
                 last_task_provider_connection_id, last_task_model_id,
                 reasoning_selection,
                 revision, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(principal_id) DO UPDATE SET
                approval_mode = excluded.approval_mode,
                sandbox_profile = excluded.sandbox_profile,
                network_intent = excluded.network_intent,
                revision = excluded.revision,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(principal_id)
        .bind(approval_mode)
        .bind(sandbox_profile)
        .bind(network_intent)
        .bind(connection_id)
        .bind(model_id)
        .bind(task_connection_id)
        .bind(task_model_id)
        .bind(reasoning_selection)
        .bind(next_revision as i64)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| PreferenceError::Storage(e.to_string()))?;
        self.get(principal_id)
            .await?
            .ok_or_else(|| PreferenceError::Storage("preference write lost".into()))
    }

    /// Provider reasoning control M002: persist the caller's durable,
    /// principal-scoped extended-thinking selection.
    ///
    /// The selection is **open**: an effort label is a provider-scoped
    /// string carried verbatim, never folded onto a closed level set. Its
    /// shape is validated here, at the store boundary, and is never
    /// trusted verbatim.
    ///
    /// `None` clears the selection, which puts the next turn back on the
    /// agent / model-profile default.
    ///
    /// The write is **revision-gated in the statement itself**: the upsert
    /// carries the revision this call observed as a predicate, so a writer
    /// that lands between this call's read and its write loses the
    /// compare-and-swap instead of overwriting the other writer's value.
    /// Two frontends racing a selection change therefore converge on one
    /// winner and one [`PreferenceError::Conflict`], never on a silent
    /// last-write-wins.
    ///
    /// `expected_revision` is the revision the *caller* believes is
    /// current. `Some(expected)` refuses up front when the stored revision
    /// has already moved past it. `None` means the caller does not assert a
    /// revision, but the write is still gated on the revision this call
    /// observed, so a concurrent write is a typed conflict rather than a
    /// lost update.
    ///
    /// Every other preference dimension is preserved untouched.
    pub async fn set_reasoning_selection(
        &self,
        principal_id: &str,
        selection: Option<&ReasoningOverride>,
        expected_revision: Option<u64>,
    ) -> Result<RuntimePreference, PreferenceError> {
        validate_principal_id(principal_id)?;
        let encoded = encode_reasoning_selection(selection)?;
        let current = self.get(principal_id).await?;
        let (current_revision, current_row) = match current {
            Some(p) => (p.revision, Some(p)),
            None => (0, None),
        };
        if let Some(expected) = expected_revision {
            if expected != current_revision {
                return Err(PreferenceError::Conflict {
                    expected,
                    current: current_revision,
                });
            }
        }
        let next_revision = current_revision.saturating_add(1).max(1);
        let now = now_millis();
        let approval_mode = current_row
            .as_ref()
            .and_then(|p| p.approval_mode.map(|m| m.as_str().to_owned()));
        let sandbox_profile = current_row
            .as_ref()
            .and_then(|p| p.sandbox_profile.map(|s| s.as_str().to_owned()));
        let (connection_id, model_id) = match current_row.as_ref() {
            Some(p) => (
                p.last_provider_connection_id.clone(),
                p.last_model_id.clone(),
            ),
            None => (None, None),
        };
        let (task_connection_id, task_model_id) = match current_row.as_ref() {
            Some(p) => (
                p.last_task_provider_connection_id.clone(),
                p.last_task_model_id.clone(),
            ),
            None => (None, None),
        };
        let write = RuntimePreferenceWrite {
            principal_id: principal_id.to_owned(),
            approval_mode,
            sandbox_profile,
            last_provider_connection_id: connection_id,
            last_model_id: model_id,
            last_task_provider_connection_id: task_connection_id,
            last_task_model_id: task_model_id,
            reasoning_selection: encoded,
        };
        if !self
            .upsert_reasoning_selection(&write, current_revision, next_revision, now)
            .await?
        {
            // The compare-and-swap rejected the write: another writer moved
            // the row's revision between this call's read and its write. The
            // write did **not** land, so the caller's choice is not lost —
            // it is reported as a typed conflict it can retry.
            let current = self
                .get(principal_id)
                .await
                .ok()
                .flatten()
                .map(|p| p.revision)
                .unwrap_or(current_revision);
            return Err(PreferenceError::Conflict {
                expected: current_revision,
                current,
            });
        }
        self.get(principal_id)
            .await?
            .ok_or_else(|| PreferenceError::Storage("preference write lost".into()))
    }

    /// The reasoning-selection write's compare-and-swap.
    ///
    /// `Ok(true)` means the row was written. `Ok(false)` means the row's
    /// revision was no longer `observed_revision` when the statement
    /// landed, so the statement's `DO UPDATE` was skipped and **nothing
    /// was written**.
    ///
    /// The revision predicate lives inside the statement on purpose. A
    /// revision check performed before the write is a check another writer
    /// can invalidate between the check and the write, which is how a
    /// read-then-write gate turns into a silent last-write-wins.
    async fn upsert_reasoning_selection(
        &self,
        write: &RuntimePreferenceWrite,
        observed_revision: u64,
        next_revision: u64,
        now: i64,
    ) -> Result<bool, PreferenceError> {
        let result = sqlx::query(
            r#"
            INSERT INTO runtime_preferences
                (principal_id, approval_mode, sandbox_profile,
                 last_provider_connection_id, last_model_id,
                 last_task_provider_connection_id, last_task_model_id,
                 reasoning_selection,
                 revision, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(principal_id) DO UPDATE SET
                reasoning_selection = excluded.reasoning_selection,
                revision = excluded.revision,
                updated_at = excluded.updated_at
            WHERE runtime_preferences.revision = ?11
            "#,
        )
        .bind(write.principal_id.as_str())
        .bind(write.approval_mode.clone())
        .bind(write.sandbox_profile.clone())
        .bind(write.last_provider_connection_id.clone())
        .bind(write.last_model_id.clone())
        .bind(write.last_task_provider_connection_id.clone())
        .bind(write.last_task_model_id.clone())
        .bind(write.reasoning_selection.clone())
        .bind(next_revision as i64)
        .bind(now)
        .bind(observed_revision as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| PreferenceError::Storage(e.to_string()))?;
        Ok(result.rows_affected() > 0)
    }
}

/// The values one reasoning-selection write carries.
///
/// A reasoning write only ever replaces the reasoning column. Every other
/// column is carried forward from the row that was read, so a reasoning
/// write never disturbs the approval, sandbox, or model-preference scopes.
#[derive(Debug, Clone, Default)]
struct RuntimePreferenceWrite {
    principal_id: String,
    approval_mode: Option<String>,
    sandbox_profile: Option<String>,
    last_provider_connection_id: Option<String>,
    last_model_id: Option<String>,
    last_task_provider_connection_id: Option<String>,
    last_task_model_id: Option<String>,
    reasoning_selection: Option<String>,
}

/// The stored reasoning selection of `current`, re-encoded for an
/// upsert that must preserve it.
///
/// A decoded selection already passed the shape check, so this cannot
/// fail; a corrupt stored value decodes to `None` and is written back as
/// `NULL` (a corrupt value is not authority).
fn current_reasoning_selection(current: Option<&RuntimePreference>) -> Option<String> {
    current
        .and_then(|p| p.reasoning_selection.as_ref())
        .and_then(|selection| encode_reasoning_selection(Some(selection)).ok())
        .flatten()
}

#[derive(Debug, sqlx::FromRow)]
struct RuntimePreferenceRow {
    principal_id: String,
    approval_mode: Option<String>,
    sandbox_profile: Option<String>,
    network_intent: Option<String>,
    last_provider_connection_id: Option<String>,
    last_model_id: Option<String>,
    last_task_provider_connection_id: Option<String>,
    last_task_model_id: Option<String>,
    reasoning_selection: Option<String>,
    revision: i64,
    updated_at: i64,
}

impl RuntimePreferenceRow {
    fn into_preference(self) -> RuntimePreference {
        RuntimePreference {
            principal_id: self.principal_id,
            approval_mode: self.approval_mode.as_deref().and_then(ApprovalMode::parse),
            sandbox_profile: self
                .sandbox_profile
                .as_deref()
                .and_then(SandboxProfile::parse),
            network_intent: self
                .network_intent
                .as_deref()
                .and_then(NetworkIntent::parse),
            last_provider_connection_id: sanitize_identity(self.last_provider_connection_id),
            last_model_id: sanitize_identity(self.last_model_id),
            last_task_provider_connection_id: sanitize_identity(
                self.last_task_provider_connection_id,
            ),
            last_task_model_id: sanitize_identity(self.last_task_model_id),
            reasoning_selection: decode_reasoning_selection(self.reasoning_selection),
            revision: self.revision.max(0) as u64,
            updated_at_ms: self.updated_at,
        }
    }
}

/// Decode a stored reasoning selection.
///
/// The durable value is the protocol's own serialized shape, so there is
/// no second vocabulary to keep in sync. A value that does not decode —
/// or that no longer passes the shape check — is **not authority**: it
/// degrades to "no explicit selection" rather than failing the read.
fn decode_reasoning_selection(value: Option<String>) -> Option<ReasoningOverride> {
    let raw = value?;
    let decoded: ReasoningOverride = serde_json::from_str(&raw).ok()?;
    validate_reasoning_selection(&decoded).ok()?;
    Some(decoded)
}

/// Encode a reasoning selection for durable storage.
///
/// `None` is stored as `NULL` ("no explicit selection").
fn encode_reasoning_selection(
    selection: Option<&ReasoningOverride>,
) -> Result<Option<String>, PreferenceError> {
    let Some(selection) = selection else {
        return Ok(None);
    };
    validate_reasoning_selection(selection)?;
    let encoded = serde_json::to_string(selection).map_err(|e| {
        PreferenceError::Validation(format!("reasoning selection is not encodable: {e}"))
    })?;
    Ok(Some(encoded))
}

/// Validate the shape of a reasoning selection at the store boundary.
///
/// The label is **open**: it is not matched against a closed set, and it
/// is preserved verbatim. What is checked is that it is a plausible
/// label — non-empty, bounded in length, and free of control characters
/// (including NUL), so a hostile or mistaken value cannot become an
/// unbounded durable value.
fn validate_reasoning_selection(selection: &ReasoningOverride) -> Result<(), PreferenceError> {
    match selection {
        ReasoningOverride::Effort { level } => {
            if level.trim().is_empty() {
                return Err(PreferenceError::Validation(
                    "reasoning label must not be empty".into(),
                ));
            }
            if level.len() > MAX_REASONING_LABEL_BYTES {
                return Err(PreferenceError::Validation(format!(
                    "reasoning label must be at most {MAX_REASONING_LABEL_BYTES} bytes"
                )));
            }
            if level.chars().any(|c| c.is_control()) {
                return Err(PreferenceError::Validation(
                    "reasoning label must not contain control characters".into(),
                ));
            }
            Ok(())
        }
        ReasoningOverride::Budget { tokens } => {
            if *tokens == 0 || *tokens > MAX_REASONING_BUDGET_TOKENS {
                return Err(PreferenceError::Validation(format!(
                    "reasoning budget must be 1..={MAX_REASONING_BUDGET_TOKENS} tokens"
                )));
            }
            Ok(())
        }
        ReasoningOverride::Off => Ok(()),
    }
}

fn sanitize_identity(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let t = v.trim().to_owned();
        if t.is_empty() || t.len() > 512 || t.contains('\0') {
            None
        } else {
            Some(t)
        }
    })
}

fn validate_principal_id(value: &str) -> Result<(), PreferenceError> {
    let t = value.trim();
    if t.is_empty() || t.len() > 256 || t.contains('\0') {
        return Err(PreferenceError::Validation(
            "principal_id must be 1..=256 chars without NUL".into(),
        ));
    }
    Ok(())
}

fn validate_identity_field(field: &str, value: &str) -> Result<(), PreferenceError> {
    let t = value.trim();
    if t.is_empty() || t.len() > 512 || t.contains('\0') {
        return Err(PreferenceError::Validation(format!(
            "{field} must be 1..=512 chars without NUL"
        )));
    }
    Ok(())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Additive M003 schema for `runtime_preferences`. Safe on existing
/// databases via `IF NOT EXISTS`. Fresh databases include the M003
/// Task-composer columns and the M002 reasoning-selection column;
/// pre-M003 databases gain the former via `migrate_v62` and pre-M002
/// databases gain the latter via `migrate_v84`.
pub const RUNTIME_PREFERENCE_SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS runtime_preferences (
        principal_id TEXT PRIMARY KEY,
        approval_mode TEXT CHECK (approval_mode IS NULL OR approval_mode IN ('interactive','automatic','yolo')),
        sandbox_profile TEXT CHECK (sandbox_profile IS NULL OR sandbox_profile IN ('read_only','workspace_write','full_host')),
        network_intent TEXT CHECK (network_intent IS NULL OR network_intent IN ('legacy_unrestricted','offline_required')),
        last_provider_connection_id TEXT CHECK (last_provider_connection_id IS NULL OR length(last_provider_connection_id) <= 512),
        last_model_id TEXT CHECK (last_model_id IS NULL OR length(last_model_id) <= 512),
        last_task_provider_connection_id TEXT CHECK (last_task_provider_connection_id IS NULL OR length(last_task_provider_connection_id) <= 512),
        last_task_model_id TEXT CHECK (last_task_model_id IS NULL OR length(last_task_model_id) <= 512),
        reasoning_selection TEXT CHECK (reasoning_selection IS NULL OR length(reasoning_selection) <= 512),
        revision INTEGER NOT NULL CHECK (revision >= 0),
        updated_at INTEGER NOT NULL
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_runtime_preferences_updated ON runtime_preferences(updated_at DESC)",
];

/// Additive M003 Work Orders migration statements for `runtime_preferences`.
///
/// Applied by `migrate_v62` on pre-M003 databases. Plain `ADD COLUMN`
/// keeps every existing row (ordinary session preference, approval,
/// sandbox untouched); new columns default to `NULL` ("no Task
/// preference"). Bounds are enforced in Rust (`validate_identity_field`
/// on write, `sanitize_identity` on read), matching the existing
/// nullable-identity convention.
pub const RUNTIME_PREFERENCE_TASK_MODEL_MIGRATION_STATEMENTS: &[&str] = &[
    "ALTER TABLE runtime_preferences ADD COLUMN last_task_provider_connection_id TEXT",
    "ALTER TABLE runtime_preferences ADD COLUMN last_task_model_id TEXT",
];

/// Additive M002 provider-reasoning migration statements for
/// `runtime_preferences`.
///
/// Applied by `migrate_v88` on pre-M002 databases. Plain `ADD COLUMN`
/// keeps every existing row untouched — approval, sandbox, model and
/// Task preferences are all preserved — and the new column defaults to
/// `NULL` ("no explicit reasoning selection"). The column holds the
/// protocol's own serialized selection shape, so there is no second
/// vocabulary; the label inside it is an open, provider-scoped string.
/// Bounds are enforced in Rust (`validate_reasoning_selection` on write,
/// `decode_reasoning_selection` on read).
///
/// The migration was `migrate_v84` on the provider-reasoning-control branch
/// and is **v88** in the merged tree: `main` had already published v84 (the
/// additive network-intent preference below) and v85 through v87, so the
/// branch yields the lower number to `main`.
pub const RUNTIME_PREFERENCE_REASONING_MIGRATION_STATEMENTS: &[&str] =
    &["ALTER TABLE runtime_preferences ADD COLUMN reasoning_selection TEXT"];

/// M005 offline networking opt-in. Existing principals remain unrestricted
/// because the nullable value resolves to `LegacyUnrestricted`.
pub const RUNTIME_PREFERENCE_NETWORK_INTENT_MIGRATION_STATEMENTS: &[&str] = &[
    "ALTER TABLE runtime_preferences ADD COLUMN network_intent TEXT CHECK (network_intent IS NULL OR network_intent IN ('legacy_unrestricted','offline_required'))",
];

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use std::time::Duration;

    async fn temp_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .expect("in-memory db");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    /// A shared in-memory pool with several connections.
    ///
    /// A single-connection pool cannot express a write race: the pool
    /// serializes the two writers, so the second one reads the first one's
    /// committed revision and the race never happens. The shared cache gives
    /// every connection the same database, so both writers really do read
    /// the same row.
    async fn shared_pool() -> SqlitePool {
        let name = format!("runtime_preference_{}", uuid::Uuid::new_v4().simple());
        let opts = SqliteConnectOptions::from_str(&format!("file:{name}?mode=memory&cache=shared"))
            .expect("sqlite options")
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(opts)
            .await
            .expect("shared in-memory db");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    #[test]
    fn approval_mode_parse_round_trips() {
        assert_eq!(
            ApprovalMode::parse("interactive"),
            Some(ApprovalMode::Interactive)
        );
        assert_eq!(ApprovalMode::parse("AUTO"), Some(ApprovalMode::Automatic));
        assert_eq!(ApprovalMode::parse("yolo"), Some(ApprovalMode::Yolo));
        assert_eq!(ApprovalMode::parse("bogus"), None);
        assert_eq!(ApprovalMode::Yolo.as_str(), "yolo");
    }

    #[test]
    fn sandbox_profile_parse_round_trips() {
        assert_eq!(
            SandboxProfile::parse("read_only"),
            Some(SandboxProfile::ReadOnly)
        );
        assert_eq!(
            SandboxProfile::parse("workspace_write"),
            Some(SandboxProfile::WorkspaceWrite)
        );
        assert_eq!(
            SandboxProfile::parse("full_host"),
            Some(SandboxProfile::FullHost)
        );
        assert_eq!(SandboxProfile::parse("bogus"), None);
    }

    #[test]
    fn child_ceiling_enforced() {
        assert!(child_mode_allowed(ApprovalMode::Yolo, ApprovalMode::Yolo));
        assert!(child_mode_allowed(
            ApprovalMode::Yolo,
            ApprovalMode::Interactive
        ));
        assert!(!child_mode_allowed(
            ApprovalMode::Interactive,
            ApprovalMode::Yolo
        ));
        assert!(!child_mode_allowed(
            ApprovalMode::Interactive,
            ApprovalMode::Automatic
        ));
        assert!(child_sandbox_allowed(
            SandboxProfile::FullHost,
            SandboxProfile::WorkspaceWrite
        ));
        assert!(!child_sandbox_allowed(
            SandboxProfile::WorkspaceWrite,
            SandboxProfile::FullHost
        ));
    }

    #[test]
    fn precedence_prefers_explicit_then_ceiling_then_persisted() {
        assert_eq!(
            resolve_effective_mode(
                Some(ApprovalMode::Yolo),
                Some(ApprovalMode::Interactive),
                Some(ApprovalMode::Yolo),
                ApprovalMode::Interactive,
            ),
            ApprovalMode::Interactive
        );
        assert_eq!(
            resolve_effective_mode(
                None,
                None,
                Some(ApprovalMode::Yolo),
                ApprovalMode::Interactive,
            ),
            ApprovalMode::Yolo
        );
        assert_eq!(
            resolve_effective_mode(None, None, None, ApprovalMode::Interactive,),
            ApprovalMode::Interactive
        );
    }

    #[test]
    fn snapshot_narrow_for_child_enforces_ceiling() {
        let parent = ExecutionPolicySnapshot::capture(
            ApprovalMode::Interactive,
            SandboxProfile::WorkspaceWrite,
            None,
            None,
            None,
            None,
            None,
        )
        .with_network_intent(NetworkIntent::OfflineRequired)
        .expect("network ceiling may narrow to offline");
        assert!(parent
            .narrow_for_child(ApprovalMode::Yolo, SandboxProfile::WorkspaceWrite)
            .is_err());
        let child = parent
            .narrow_for_child(ApprovalMode::Interactive, SandboxProfile::ReadOnly)
            .expect("child may narrow filesystem and retain network ceiling");
        assert_eq!(child.network_intent(), NetworkIntent::OfflineRequired);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_preference_round_trips_and_revises() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        assert!(store.get("local-owner").await.unwrap().is_none());
        let first = store
            .set_approval_mode("local-owner", ApprovalMode::Yolo, None)
            .await
            .unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(first.approval_mode, Some(ApprovalMode::Yolo));
        let second = store
            .set_sandbox_profile(
                "local-owner",
                SandboxProfile::ReadOnly,
                Some(first.revision),
            )
            .await
            .unwrap();
        assert_eq!(second.revision, 2);
        assert_eq!(second.sandbox_profile, Some(SandboxProfile::ReadOnly));
        // Approval mode is retained across sandbox writes.
        assert_eq!(second.approval_mode, Some(ApprovalMode::Yolo));
        assert_eq!(
            second.effective_network_intent(),
            NetworkIntent::LegacyUnrestricted,
            "existing preference writes do not opt into offline networking"
        );
        let third = store
            .set_network_intent(
                "local-owner",
                NetworkIntent::OfflineRequired,
                Some(second.revision),
            )
            .await
            .unwrap();
        assert_eq!(third.revision, 3);
        assert_eq!(third.network_intent, Some(NetworkIntent::OfflineRequired));
        assert_eq!(third.sandbox_profile, Some(SandboxProfile::ReadOnly));
        assert_eq!(third.approval_mode, Some(ApprovalMode::Yolo));
    }
    #[tokio::test(flavor = "current_thread")]
    async fn reasoning_selection_round_trips_an_open_label_verbatim() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        let selection = ReasoningOverride::Effort {
            level: "xhigh".to_string(),
        };
        let written = store
            .set_reasoning_selection("local-owner", Some(&selection), None)
            .await
            .unwrap();
        assert_eq!(written.reasoning_selection, Some(selection.clone()));
        let read = store.get("local-owner").await.unwrap().unwrap();
        assert_eq!(
            read.reasoning_selection,
            Some(selection),
            "an open vendor label must survive the store verbatim"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reasoning_selection_is_absent_until_it_is_written() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        assert!(store.get("local-owner").await.unwrap().is_none());
        let written = store
            .set_approval_mode("local-owner", ApprovalMode::Interactive, None)
            .await
            .unwrap();
        assert_eq!(
            written.reasoning_selection, None,
            "no explicit selection means the agent / model-profile default"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reasoning_selection_write_is_revision_gated() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        let first = store
            .set_reasoning_selection(
                "local-owner",
                Some(&ReasoningOverride::Effort {
                    level: "high".to_string(),
                }),
                None,
            )
            .await
            .unwrap();
        let err = store
            .set_reasoning_selection(
                "local-owner",
                Some(&ReasoningOverride::Off),
                Some(first.revision + 5),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, PreferenceError::Conflict { .. }),
            "a stale write must conflict, never last-write-wins"
        );
        let current = store.get("local-owner").await.unwrap().unwrap();
        assert_eq!(
            current.reasoning_selection,
            Some(ReasoningOverride::Effort {
                level: "high".to_string()
            }),
            "the losing write must not have landed"
        );
    }

    /// The revision gate must live **inside** the write, not beside it.
    ///
    /// A gate that reads the revision and then writes is a check another
    /// writer can invalidate between the check and the write. This test
    /// reproduces exactly that window deterministically: a writer that
    /// observed revision 0 lands its write after another writer has
    /// already moved the row to revision 1.
    #[tokio::test(flavor = "current_thread")]
    async fn a_stale_reasoning_write_is_rejected_by_the_statement_itself() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        let selection = ReasoningOverride::Effort {
            level: "high".to_string(),
        };
        let first = store
            .set_reasoning_selection("local-owner", Some(&selection), None)
            .await
            .unwrap();
        assert_eq!(first.revision, 1);

        let stale = RuntimePreferenceWrite {
            principal_id: "local-owner".to_string(),
            reasoning_selection: encode_reasoning_selection(Some(&ReasoningOverride::Off)).unwrap(),
            ..RuntimePreferenceWrite::default()
        };
        let landed = store
            .upsert_reasoning_selection(&stale, 0, 1, now_millis())
            .await
            .unwrap();
        assert!(
            !landed,
            "a write based on a revision that moved must not land"
        );
        let current = store.get("local-owner").await.unwrap().unwrap();
        assert_eq!(
            current.revision, 1,
            "the losing write must not move the row's revision"
        );
        assert_eq!(
            current.reasoning_selection,
            Some(selection),
            "the winning value must survive the rejected write"
        );
    }

    /// Two writers racing the same revision converge on one winner and one
    /// typed conflict. A silent last-write-wins would let the loser's value
    /// land and leave the caller believing its own write succeeded.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_concurrent_reasoning_writes_produce_one_winner_and_one_conflict() {
        let pool = shared_pool().await;
        let store = RuntimePreferenceStore::new(pool.clone());
        store
            .set_reasoning_selection(
                "local-owner",
                Some(&ReasoningOverride::Effort {
                    level: "low".to_string(),
                }),
                None,
            )
            .await
            .unwrap();

        for _round in 0..16 {
            let before = store.get("local-owner").await.unwrap().unwrap();
            let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
            let mut handles = Vec::new();
            for label in ["xhigh", "max"] {
                let store = RuntimePreferenceStore::new(pool.clone());
                let barrier = barrier.clone();
                let expected = before.revision;
                handles.push(tokio::spawn(async move {
                    barrier.wait().await;
                    let selection = ReasoningOverride::Effort {
                        level: label.to_string(),
                    };
                    store
                        .set_reasoning_selection("local-owner", Some(&selection), Some(expected))
                        .await
                }));
            }
            let mut winners = 0;
            let mut conflicts = 0;
            for handle in handles {
                match handle.await.expect("writer task") {
                    Ok(_) => winners += 1,
                    Err(PreferenceError::Conflict { .. }) => conflicts += 1,
                    Err(other) => panic!("unexpected write error: {other:?}"),
                }
            }
            assert_eq!(winners, 1, "exactly one writer may win the round");
            assert_eq!(conflicts, 1, "the other writer must see a typed conflict");
            let after = store.get("local-owner").await.unwrap().unwrap();
            assert_eq!(
                after.revision,
                before.revision + 1,
                "a racing round advances the revision exactly once"
            );
        }
    }

    /// The durable selection outlives the process that wrote it.
    ///
    /// A `sqlite::memory:` pool cannot show this: it is a different
    /// database on every open, so a read after a "restart" would prove
    /// nothing. This test writes to a file-backed database, closes it, and
    /// reopens it.
    #[tokio::test(flavor = "current_thread")]
    async fn reasoning_selection_survives_a_restart() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("runtime_preferences.sqlite");
        let url = format!("sqlite://{}", path.display());
        let options = || {
            SqliteConnectOptions::from_str(&url)
                .expect("sqlite options")
                .create_if_missing(true)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5))
        };
        let selection = ReasoningOverride::Effort {
            level: "xhigh".to_string(),
        };
        {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(5))
                .connect_with(options())
                .await
                .expect("connect file-backed db");
            crate::session::schema::migrate(&pool)
                .await
                .expect("migrate");
            let store = RuntimePreferenceStore::new(pool.clone());
            let written = store
                .set_reasoning_selection("local-owner", Some(&selection), None)
                .await
                .unwrap();
            assert_eq!(written.revision, 1);
            pool.close().await;
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options())
            .await
            .expect("reconnect file-backed db");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        let store = RuntimePreferenceStore::new(pool);
        let read = store.get("local-owner").await.unwrap().unwrap();
        assert_eq!(
            read.reasoning_selection,
            Some(selection),
            "the durable selection must survive the daemon restarting"
        );
        assert_eq!(read.revision, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reasoning_selection_survives_unrelated_preference_writes() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        let selection = ReasoningOverride::Effort {
            level: "max".to_string(),
        };
        store
            .set_reasoning_selection("local-owner", Some(&selection), None)
            .await
            .unwrap();
        store
            .set_approval_mode("local-owner", ApprovalMode::Automatic, None)
            .await
            .unwrap();
        store
            .set_model_preference("local-owner", Some("conn-1"), Some("model-1"), None)
            .await
            .unwrap();
        store
            .set_task_model_preference("local-owner", Some("conn-2"), Some("model-2"), None)
            .await
            .unwrap();
        let current = store.get("local-owner").await.unwrap().unwrap();
        assert_eq!(current.reasoning_selection, Some(selection));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reasoning_selection_can_be_cleared() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        store
            .set_reasoning_selection(
                "local-owner",
                Some(&ReasoningOverride::Effort {
                    level: "high".to_string(),
                }),
                None,
            )
            .await
            .unwrap();
        let cleared = store
            .set_reasoning_selection("local-owner", None, None)
            .await
            .unwrap();
        assert_eq!(cleared.reasoning_selection, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reasoning_selection_rejects_an_implausible_label() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        let too_long = ReasoningOverride::Effort {
            level: "x".repeat(MAX_REASONING_LABEL_BYTES + 1),
        };
        let err = store
            .set_reasoning_selection("local-owner", Some(&too_long), None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, PreferenceError::Validation(_)),
            "an unbounded label must be refused at the store boundary"
        );
        let control = ReasoningOverride::Effort {
            level: "xhigh\0y".to_string(),
        };
        let err = store
            .set_reasoning_selection("local-owner", Some(&control), None)
            .await
            .unwrap_err();
        assert!(matches!(err, PreferenceError::Validation(_)));
        let zero_budget = ReasoningOverride::Budget { tokens: 0 };
        let err = store
            .set_reasoning_selection("local-owner", Some(&zero_budget), None)
            .await
            .unwrap_err();
        assert!(matches!(err, PreferenceError::Validation(_)));
        assert!(
            store.get("local-owner").await.unwrap().is_none(),
            "a refused write must leave nothing behind"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_corrupt_reasoning_selection_degrades_to_no_selection() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        store
            .set_reasoning_selection(
                "local-owner",
                Some(&ReasoningOverride::Effort {
                    level: "high".to_string(),
                }),
                None,
            )
            .await
            .unwrap();
        sqlx::query(
            "UPDATE runtime_preferences SET reasoning_selection = 'not-json' WHERE principal_id = ?1",
        )
        .bind("local-owner")
        .execute(store.pool())
        .await
        .unwrap();
        let current = store.get("local-owner").await.unwrap().unwrap();
        assert_eq!(
            current.reasoning_selection, None,
            "a corrupt stored value is not authority"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reasoning_selection_migration_is_additive_and_preserves_rows() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .expect("in-memory db");
        // A pre-reasoning-migration `runtime_preferences` table: no reasoning
        // column. It **does** carry `network_intent`, because in the merged
        // tree the reasoning migration is v88 and runs *after* main's v84
        // network-intent migration — a database in the state this migration
        // expects already has that column. The fixture states the real
        // migration order rather than an obsolete one.
        sqlx::query(
            r#"
            CREATE TABLE runtime_preferences (
                principal_id TEXT PRIMARY KEY,
                approval_mode TEXT,
                sandbox_profile TEXT,
                network_intent TEXT,
                last_provider_connection_id TEXT,
                last_model_id TEXT,
                last_task_provider_connection_id TEXT,
                last_task_model_id TEXT,
                revision INTEGER NOT NULL CHECK (revision >= 0),
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO runtime_preferences
             (principal_id, approval_mode, sandbox_profile, network_intent,
              last_provider_connection_id, last_model_id,
              last_task_provider_connection_id, last_task_model_id,
              revision, updated_at)
             VALUES ('erin', 'yolo', 'workspace_write', 'offline_required',
                     'conn-s', 'model-s', 'conn-t', 'model-t', 3, 7)",
        )
        .execute(&pool)
        .await
        .unwrap();
        for statement in RUNTIME_PREFERENCE_REASONING_MIGRATION_STATEMENTS {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        let column_exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('runtime_preferences') WHERE name = 'reasoning_selection'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(column_exists, 1, "migrate_v88 must add the column");
        let store = RuntimePreferenceStore::new(pool);
        let pref = store.get("erin").await.unwrap().unwrap();
        // Every pre-existing dimension survives the additive migration.
        assert_eq!(pref.revision, 3);
        assert_eq!(pref.approval_mode, Some(ApprovalMode::Yolo));
        assert_eq!(pref.sandbox_profile, Some(SandboxProfile::WorkspaceWrite));
        assert_eq!(
            pref.network_intent,
            Some(NetworkIntent::OfflineRequired),
            "the reasoning migration must not disturb main's network-intent column"
        );
        assert_eq!(pref.last_provider_connection_id.as_deref(), Some("conn-s"));
        assert_eq!(pref.last_model_id.as_deref(), Some("model-s"));
        assert_eq!(
            pref.last_task_provider_connection_id.as_deref(),
            Some("conn-t")
        );
        assert_eq!(pref.last_task_model_id.as_deref(), Some("model-t"));
        // The new column defaults to "no explicit selection".
        assert_eq!(pref.reasoning_selection, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_preference_cas_conflicts_instead_of_last_write_wins() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        let first = store
            .set_approval_mode("alice", ApprovalMode::Interactive, None)
            .await
            .unwrap();
        let err = store
            .set_approval_mode("alice", ApprovalMode::Yolo, Some(first.revision + 5))
            .await
            .unwrap_err();
        assert!(matches!(err, PreferenceError::Conflict { .. }));
        let current = store.get("alice").await.unwrap().unwrap();
        assert_eq!(current.approval_mode, Some(ApprovalMode::Interactive));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn set_policy_writes_both_dimensions_in_one_revision() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        let pref = store
            .set_policy(
                "dave",
                Some(ApprovalMode::Yolo),
                Some(SandboxProfile::FullHost),
                None,
            )
            .await
            .unwrap();
        assert_eq!(pref.revision, 1);
        assert_eq!(pref.approval_mode, Some(ApprovalMode::Yolo));
        assert_eq!(pref.sandbox_profile, Some(SandboxProfile::FullHost));

        // Partial update preserves the untouched dimension and bumps once.
        let pref = store
            .set_policy("dave", Some(ApprovalMode::Automatic), None, Some(1))
            .await
            .unwrap();
        assert_eq!(pref.revision, 2);
        assert_eq!(pref.approval_mode, Some(ApprovalMode::Automatic));
        assert_eq!(pref.sandbox_profile, Some(SandboxProfile::FullHost));

        // Stale CAS fails without touching the row.
        let err = store
            .set_policy("dave", Some(ApprovalMode::Yolo), None, Some(1))
            .await
            .unwrap_err();
        assert!(matches!(err, PreferenceError::Conflict { .. }));
        let current = store.get("dave").await.unwrap().unwrap();
        assert_eq!(current.revision, 2);
        assert_eq!(current.approval_mode, Some(ApprovalMode::Automatic));

        // Empty update is rejected.
        let err = store
            .set_policy("dave", None, None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, PreferenceError::Validation(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn task_model_migration_preserves_existing_rows() {
        use sqlx::sqlite::SqlitePoolOptions;
        use std::time::Duration;
        // Simulate a pre-M003 database: v58-shape table without the Task
        // columns, holding an ordinary session preference row.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .expect("in-memory db");
        sqlx::query(
            r#"
            CREATE TABLE runtime_preferences (
                principal_id TEXT PRIMARY KEY,
                approval_mode TEXT,
                sandbox_profile TEXT,
                last_provider_connection_id TEXT,
                last_model_id TEXT,
                revision INTEGER NOT NULL CHECK (revision >= 0),
                updated_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO runtime_preferences
             (principal_id, approval_mode, sandbox_profile,
              last_provider_connection_id, last_model_id, revision, updated_at)
             VALUES ('erin', 'yolo', 'workspace_write', 'conn-s', 'model-s', 3, 7)",
        )
        .execute(&pool)
        .await
        .unwrap();
        for statement in RUNTIME_PREFERENCE_TASK_MODEL_MIGRATION_STATEMENTS {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        // A real legacy database also received `migrate_v88`, which adds the
        // M002 reasoning column. The read path selects that column, so a
        // database that never received it cannot be read at all — the
        // migration chain is what makes the row readable, not the row.
        for statement in RUNTIME_PREFERENCE_REASONING_MIGRATION_STATEMENTS {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        // And `migrate_v84`, which adds the M005 network-intent column; the
        // read path selects that one too.
        for statement in RUNTIME_PREFERENCE_NETWORK_INTENT_MIGRATION_STATEMENTS {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        // Re-running is idempotent via the caller's duplicate-tolerant
        // helper; here the statements themselves must at least apply once.
        let store = RuntimePreferenceStore::new(pool);
        let pref = store.get("erin").await.unwrap().unwrap();
        assert_eq!(pref.revision, 3);
        assert_eq!(pref.last_provider_connection_id.as_deref(), Some("conn-s"));
        assert_eq!(pref.last_model_id.as_deref(), Some("model-s"));
        assert_eq!(pref.last_task_provider_connection_id, None);
        assert_eq!(pref.last_task_model_id, None);
        assert!(pref.has_model_preference());
        assert!(!pref.has_task_model_preference());
        // Task write after migration preserves the carried row.
        let pref = store
            .set_task_model_preference("erin", Some("conn-t"), Some("model-t"), Some(3))
            .await
            .unwrap();
        assert_eq!(pref.revision, 4);
        assert_eq!(pref.last_provider_connection_id.as_deref(), Some("conn-s"));
        assert_eq!(pref.last_task_model_id.as_deref(), Some("model-t"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_preference_bounds_reject_oversize_principal() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        let big = "p".repeat(300);
        let err = store.get(&big).await.unwrap_err();
        assert!(matches!(err, PreferenceError::Validation(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_preference_model_fields_reserved_for_m004() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        let pref = store
            .set_model_preference("bob", Some("conn-1"), Some("model-1"), None)
            .await
            .unwrap();
        assert_eq!(pref.last_provider_connection_id.as_deref(), Some("conn-1"));
        assert_eq!(pref.last_model_id.as_deref(), Some("model-1"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn task_model_preference_is_scoped_apart_from_session_preference() {
        let store = RuntimePreferenceStore::new(temp_pool().await);
        // Ordinary session preference first.
        let pref = store
            .set_model_preference("bob", Some("conn-session"), Some("model-session"), None)
            .await
            .unwrap();
        assert!(pref.has_model_preference());
        assert!(!pref.has_task_model_preference());

        // Task preference write leaves the session preference untouched.
        let pref = store
            .set_task_model_preference("bob", Some("conn-task"), Some("model-task"), Some(1))
            .await
            .unwrap();
        assert_eq!(pref.revision, 2);
        assert_eq!(
            pref.last_provider_connection_id.as_deref(),
            Some("conn-session")
        );
        assert_eq!(pref.last_model_id.as_deref(), Some("model-session"));
        assert_eq!(
            pref.last_task_provider_connection_id.as_deref(),
            Some("conn-task")
        );
        assert_eq!(pref.last_task_model_id.as_deref(), Some("model-task"));
        assert!(pref.has_model_preference());
        assert!(pref.has_task_model_preference());

        // Ordinary session write leaves the Task preference untouched.
        let pref = store
            .set_model_preference("bob", Some("conn-session-2"), Some("model-s2"), Some(2))
            .await
            .unwrap();
        assert_eq!(pref.revision, 3);
        assert_eq!(
            pref.last_task_provider_connection_id.as_deref(),
            Some("conn-task")
        );
        assert_eq!(pref.last_task_model_id.as_deref(), Some("model-task"));

        // Approval/sandbox writes preserve both model scopes.
        let pref = store
            .set_policy(
                "bob",
                Some(ApprovalMode::Yolo),
                Some(SandboxProfile::ReadOnly),
                Some(3),
            )
            .await
            .unwrap();
        assert_eq!(pref.revision, 4);
        assert_eq!(
            pref.last_provider_connection_id.as_deref(),
            Some("conn-session-2")
        );
        assert_eq!(
            pref.last_task_provider_connection_id.as_deref(),
            Some("conn-task")
        );

        // Stale CAS on the Task scope conflicts without touching the row.
        let err = store
            .set_task_model_preference("bob", Some("conn-x"), Some("model-x"), Some(2))
            .await
            .unwrap_err();
        assert!(matches!(err, PreferenceError::Conflict { .. }));
        let current = store.get("bob").await.unwrap().unwrap();
        assert_eq!(current.revision, 4);

        // Oversize Task identities fail closed.
        let big = "m".repeat(600);
        let err = store
            .set_task_model_preference("bob", Some(&big), Some("model-x"), None)
            .await
            .unwrap_err();
        assert!(matches!(err, PreferenceError::Validation(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_preference_corrupt_mode_degrades_conservatively() {
        let pool = temp_pool().await;
        sqlx::query(
            "INSERT INTO runtime_preferences
             (principal_id, approval_mode, sandbox_profile, revision, updated_at)
             VALUES ('carol', 'interactive', 'workspace_write', 1, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Bypass the CHECK constraint to simulate a corrupt row from an
        // older writer; direct UPDATE without constraint enforcement.
        let _ = sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&pool)
            .await;
        let store = RuntimePreferenceStore::new(pool);
        let pref = store.get("carol").await.unwrap().unwrap();
        assert_eq!(pref.effective_approval_mode(), ApprovalMode::Interactive);
    }

    #[test]
    fn preference_application_outcome_codes_are_stable() {
        let applied = PreferenceApplicationOutcome::Applied {
            connection_id: "conn-1".into(),
            model_id: "model-1".into(),
        };
        assert_eq!(applied.code(), "preference_applied");
        assert!(applied.message().contains("conn-1"));
        assert_eq!(
            PreferenceApplicationOutcome::ExplicitSelectionPresent.code(),
            "explicit_selection_present"
        );
        assert_eq!(
            PreferenceApplicationOutcome::NoPreference.code(),
            "no_preference"
        );
        assert_eq!(
            PreferenceApplicationOutcome::UnavailableConnection {
                reason: "missing".into()
            }
            .code(),
            "preference_connection_unavailable"
        );
        assert_eq!(
            PreferenceApplicationOutcome::UnknownModel {
                connection_id: "c".into(),
                model_id: "m".into()
            }
            .code(),
            "preference_unknown_model"
        );
        assert_eq!(
            PreferenceApplicationOutcome::StaleCatalog {
                detail: "rev".into()
            }
            .code(),
            "preference_catalog_stale"
        );
    }

    #[test]
    fn runtime_preference_has_model_preference_gate() {
        let mut pref = RuntimePreference {
            principal_id: "p".into(),
            approval_mode: None,
            sandbox_profile: None,
            network_intent: None,
            last_provider_connection_id: Some("conn-1".into()),
            last_model_id: Some("model-1".into()),
            last_task_provider_connection_id: None,
            last_task_model_id: None,
            reasoning_selection: None,
            revision: 1,
            updated_at_ms: 0,
        };
        assert!(pref.has_model_preference());
        assert!(!pref.has_task_model_preference());
        pref.last_model_id = None;
        assert!(!pref.has_model_preference());
        // Task scope is independent: setting it does not imply a session
        // preference and vice versa.
        pref.last_task_provider_connection_id = Some("conn-t".into());
        pref.last_task_model_id = Some("model-t".into());
        assert!(!pref.has_model_preference());
        assert!(pref.has_task_model_preference());
    }

    #[test]
    fn sandbox_enforcement_matrix_is_truthful() {
        // FullHost is explicit no-containment, never "enforced".
        let full = SandboxEnforcement::for_full_host();
        assert!(full.is_full_host());
        assert!(!full.is_enforced());
        assert!(matches!(full.filesystem, FilesystemEnforcement::FullHost));
        assert!(matches!(full.network, NetworkEnforcement::Unrestricted));

        // Constrained enforced reports filesystem + unrestricted network.
        let enforced = SandboxEnforcement::for_constrained_enforced(
            SandboxProfile::WorkspaceWrite,
            "landlock",
            Some(1),
        );
        assert!(enforced.is_enforced());
        assert!(!enforced.is_full_host());
        assert!(matches!(enforced.network, NetworkEnforcement::Unrestricted));
        let text = enforced.describe();
        assert!(text.contains("workspace_write"));
        assert!(text.contains("filesystem enforced"));
        assert!(text.contains("network unrestricted"));

        // Constrained unavailable is fail-closed, never FullHost.
        let missing = SandboxEnforcement::for_constrained_unavailable(
            SandboxProfile::WorkspaceWrite,
            "Landlock unavailable: test",
        );
        assert!(!missing.is_enforced());
        assert!(!missing.is_full_host());
        assert!(matches!(
            missing.filesystem,
            FilesystemEnforcement::Unavailable { .. }
        ));

        // ReadOnly/WorkspaceWrite require containment; FullHost does not.
        assert!(SandboxProfile::ReadOnly.requires_filesystem_containment());
        assert!(SandboxProfile::WorkspaceWrite.requires_filesystem_containment());
        assert!(!SandboxProfile::FullHost.requires_filesystem_containment());
        assert_ne!(SandboxProfile::WorkspaceWrite, SandboxProfile::FullHost);
    }

    #[test]
    fn child_sandbox_resolution_enforces_ceiling() {
        assert_eq!(
            resolve_child_sandbox(SandboxProfile::WorkspaceWrite, SandboxProfile::ReadOnly)
                .unwrap(),
            SandboxProfile::ReadOnly
        );
        assert_eq!(
            resolve_child_sandbox(
                SandboxProfile::WorkspaceWrite,
                SandboxProfile::WorkspaceWrite
            )
            .unwrap(),
            SandboxProfile::WorkspaceWrite
        );
        assert!(
            resolve_child_sandbox(SandboxProfile::WorkspaceWrite, SandboxProfile::FullHost)
                .is_err()
        );
        assert!(
            resolve_child_sandbox(SandboxProfile::ReadOnly, SandboxProfile::WorkspaceWrite)
                .is_err()
        );
    }

    #[test]
    fn sandbox_escalation_is_bounded_and_explicit() {
        let req = SandboxEscalationRequest::new(
            "/etc/passwd",
            SandboxProfile::WorkspaceWrite,
            "outside workspace roots",
        );
        assert!(req.describe().contains("/etc/passwd"));
        assert!(req.describe().contains("workspace_write"));
        let long = "x".repeat(5000);
        let bounded = SandboxEscalationRequest::new(long.clone(), SandboxProfile::ReadOnly, long);
        assert!(bounded.requested_path.len() <= 1024);
        assert!(bounded.reason.len() <= 512);
    }

    #[test]
    fn runtime_preference_serialization_is_secret_free() {
        let pref = RuntimePreference {
            principal_id: "local-owner".into(),
            approval_mode: Some(ApprovalMode::Interactive),
            sandbox_profile: Some(SandboxProfile::WorkspaceWrite),
            network_intent: Some(NetworkIntent::OfflineRequired),
            last_provider_connection_id: Some("conn-1".into()),
            last_model_id: Some("model-1".into()),
            last_task_provider_connection_id: Some("conn-t".into()),
            last_task_model_id: Some("model-t".into()),
            reasoning_selection: None,
            revision: 1,
            updated_at_ms: 0,
        };
        let value = serde_json::to_value(&pref).expect("serialize preference");
        let object = value.as_object().expect("preference is an object");
        // Secret-free allowlist: identifiers + mode/profile + metadata only.
        for key in object.keys() {
            assert!(
                matches!(
                    key.as_str(),
                    "principal_id"
                        | "approval_mode"
                        | "sandbox_profile"
                        | "network_intent"
                        | "last_provider_connection_id"
                        | "last_model_id"
                        | "last_task_provider_connection_id"
                        | "last_task_model_id"
                        | "reasoning_selection"
                        | "revision"
                        | "updated_at_ms"
                ),
                "unexpected preference field: {key}"
            );
        }
        let raw = value.to_string().to_lowercase();
        for forbidden in ["token", "secret", "api_key", "apikey", "bearer", "password"] {
            assert!(!raw.contains(forbidden), "preference leaks {forbidden}");
        }
    }
}
