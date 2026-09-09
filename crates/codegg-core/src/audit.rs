//! Append-only structural audit foundation (M004).
//!
//! M003 owns the server-side authorization decision and the immutable
//! originating-principal attribution. This module owns the M004 durable
//! record: a coordinator-owned append-only structural event log with typed
//! attribution, bounded redacted metadata, deterministic ordering and
//! idempotency, authorized bounded query/pagination/filter/export, and
//! content-retention separation.
//!
//! ## Design notes
//!
//! - The coordinator (daemon) is the single sequence authority. Sequence
//!   numbers are assigned by SQLite `AUTOINCREMENT` inside one transactional
//!   `INSERT`, so concurrent appends receive deterministic unique sequences
//!   and restart continues the sequence safely via `sqlite_sequence`.
//! - Events are append-only. Production code performs `INSERT` and `SELECT`
//!   on `audit_event` only; there is no `UPDATE` or `DELETE` path for
//!   structural rows. Content bodies live in the separate `audit_body`
//!   table with their own expiry and may be deleted without touching the
//!   structural record or its digests.
//! - The caller cannot rewrite actor/decision. [`AuditEventBuilder`] takes
//!   the transport-bound [`AuthenticatedPrincipal`] plus an
//!   [`AuditDecisionProvenance`] copied from the gate-enforced M003
//!   decision (see `codegg_core::authorization::audit_provenance`); there
//!   are no setters for actor, decision, policy, or sequence fields.
//!   Request DTOs supply locators but never authority.
//! - Secrets never enter metadata. Metadata keys must match a narrow
//!   structural charset and must not contain secret-bearing substrings;
//!   values are scanned for secret-bearing substrings and oversized or
//!   secret-bearing metadata is rejected with [`AuditError`] before any
//!   write. Bodies are opaque bytes referenced by SHA-256 digest; the
//!   builder still rejects bodies that scan as credential text so a
//!   misclassified secret cannot hide behind the body seam.
//! - Duplicate event IDs are idempotent. `event_id` is `UNIQUE`; an append
//!   with a known ID returns the stored event unchanged and never assigns
//!   a second sequence number.
//! - Audit reads are authorized at the daemon boundary. `audit_query` and
//!   `audit_export` are `DirectProject` + `audit.read` operations in the
//!   M003 operation matrix, so the gate enforces the grant before the
//!   store is touched (LocalOwner broad policy observes everything; team
//!   principals must hold the grant on the queried project). Unknown
//!   action/principal filters degrade to empty pages.
//! - Writes never block forever. [`AuditWriter`] bounds in-flight appends
//!   with a semaphore and a write timeout; exhaustion surfaces typed
//!   [`AuditError::Backpressure`] and observable counters instead of an
//!   unbounded queue or a fabricated success.
//!
//! ## Transport contract (M005 consumes this store)
//!
//! Instrumentation call sites (M005) append through the typed builder at
//! the canonical state-transition owner. This module never invents
//! attribution: every event carries the decision id and correlation that
//! M003 produced at the daemon boundary.

use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::error::StorageError;
use crate::identity::{AuditEventId, PrincipalId, ProjectId};
use crate::team::PrincipalKind;
use crate::transport_auth::{AuthMethod, AuthenticatedPrincipal, TransportClass};

/// Maximum structural metadata entries per event.
pub const MAX_METADATA_ENTRIES: usize = 16;
/// Maximum UTF-8 bytes per metadata key.
pub const MAX_METADATA_KEY_LENGTH: usize = 64;
/// Maximum UTF-8 bytes per metadata value.
pub const MAX_METADATA_VALUE_LENGTH: usize = 512;
/// Maximum total UTF-8 bytes across all metadata keys and values.
pub const MAX_METADATA_TOTAL_BYTES: usize = 4096;
/// Maximum opaque body bytes referenced by one event (64 KiB window).
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// Default page size for audit queries.
pub const DEFAULT_QUERY_LIMIT: u32 = 50;
/// Maximum page size for audit queries.
pub const MAX_QUERY_LIMIT: u32 = 100;
/// Maximum events per export envelope.
pub const MAX_EXPORT_EVENTS: u32 = 200;
/// Maximum UTF-8 bytes for a correlation/causation/scope locator.
pub const MAX_CORRELATION_LENGTH: usize = 128;

/// Canonical audit actions owned by this foundation.
///
/// Writers MUST use [`AuditAction::parse_known`]; unknown input fails
/// closed at build time. Readers MUST use [`AuditAction::parse_lenient`]
/// so future actions degrade to [`AuditAction::Unknown`] instead of
/// failing a page decode.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Authentication,
    AuthorizationDecision,
    MembershipChange,
    NodeEnrollment,
    SessionCreate,
    SessionAttach,
    PromptSubmit,
    ProviderSelect,
    ModelSelect,
    AgentDelegate,
    PermissionDecision,
    ToolInvoke,
    CommandExecute,
    FileMutate,
    GitOperation,
    WorktreeLifecycle,
    JobSubmit,
    JobCancel,
    JobComplete,
    RemoteExecute,
    ChatTriggeredAction,
    ConfigChange,
    AssetRefresh,
    AuditExport,
    AuditQuery,
    #[serde(other)]
    Unknown,
}

impl AuditAction {
    /// Every known action in canonical order.
    pub const ALL: [&'static str; 25] = [
        "authentication",
        "authorization_decision",
        "membership_change",
        "node_enrollment",
        "session_create",
        "session_attach",
        "prompt_submit",
        "provider_select",
        "model_select",
        "agent_delegate",
        "permission_decision",
        "tool_invoke",
        "command_execute",
        "file_mutate",
        "git_operation",
        "worktree_lifecycle",
        "job_submit",
        "job_cancel",
        "job_complete",
        "remote_execute",
        "chat_triggered_action",
        "config_change",
        "asset_refresh",
        "audit_export",
        "audit_query",
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::AuthorizationDecision => "authorization_decision",
            Self::MembershipChange => "membership_change",
            Self::NodeEnrollment => "node_enrollment",
            Self::SessionCreate => "session_create",
            Self::SessionAttach => "session_attach",
            Self::PromptSubmit => "prompt_submit",
            Self::ProviderSelect => "provider_select",
            Self::ModelSelect => "model_select",
            Self::AgentDelegate => "agent_delegate",
            Self::PermissionDecision => "permission_decision",
            Self::ToolInvoke => "tool_invoke",
            Self::CommandExecute => "command_execute",
            Self::FileMutate => "file_mutate",
            Self::GitOperation => "git_operation",
            Self::WorktreeLifecycle => "worktree_lifecycle",
            Self::JobSubmit => "job_submit",
            Self::JobCancel => "job_cancel",
            Self::JobComplete => "job_complete",
            Self::RemoteExecute => "remote_execute",
            Self::ChatTriggeredAction => "chat_triggered_action",
            Self::ConfigChange => "config_change",
            Self::AssetRefresh => "asset_refresh",
            Self::AuditExport => "audit_export",
            Self::AuditQuery => "audit_query",
            Self::Unknown => "unknown",
        }
    }

    /// Strict parse for writers: unknown actions fail closed.
    pub fn parse_known(value: &str) -> Result<Self, AuditError> {
        Ok(match value {
            "authentication" => Self::Authentication,
            "authorization_decision" => Self::AuthorizationDecision,
            "membership_change" => Self::MembershipChange,
            "node_enrollment" => Self::NodeEnrollment,
            "session_create" => Self::SessionCreate,
            "session_attach" => Self::SessionAttach,
            "prompt_submit" => Self::PromptSubmit,
            "provider_select" => Self::ProviderSelect,
            "model_select" => Self::ModelSelect,
            "agent_delegate" => Self::AgentDelegate,
            "permission_decision" => Self::PermissionDecision,
            "tool_invoke" => Self::ToolInvoke,
            "command_execute" => Self::CommandExecute,
            "file_mutate" => Self::FileMutate,
            "git_operation" => Self::GitOperation,
            "worktree_lifecycle" => Self::WorktreeLifecycle,
            "job_submit" => Self::JobSubmit,
            "job_cancel" => Self::JobCancel,
            "job_complete" => Self::JobComplete,
            "remote_execute" => Self::RemoteExecute,
            "chat_triggered_action" => Self::ChatTriggeredAction,
            "config_change" => Self::ConfigChange,
            "asset_refresh" => Self::AssetRefresh,
            "audit_export" => Self::AuditExport,
            "audit_query" => Self::AuditQuery,
            _ => {
                return Err(AuditError::UnknownAction(value.to_owned()));
            }
        })
    }

    /// Lenient parse for readers: unknown actions degrade to `Unknown`.
    pub fn parse_lenient(value: &str) -> Self {
        Self::parse_known(value).unwrap_or(Self::Unknown)
    }
}

/// Visibility classification for one audit event.
///
/// Mirrors the long-term observability contract: project members observe
/// project events; session-participant and actor-only material stays
/// narrow; administrator material requires elevated read grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditVisibility {
    Project,
    SessionParticipants,
    ActorOnly,
    Administrators,
}

impl AuditVisibility {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::SessionParticipants => "session_participants",
            Self::ActorOnly => "actor_only",
            Self::Administrators => "administrators",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AuditError> {
        Ok(match value {
            "project" => Self::Project,
            "session_participants" => Self::SessionParticipants,
            "actor_only" => Self::ActorOnly,
            "administrators" => Self::Administrators,
            _ => return Err(AuditError::UnknownVisibility(value.to_owned())),
        })
    }
}

/// Typed audit failure.
#[derive(Debug, Error)]
pub enum AuditError {
    #[error("unknown audit action {0:?}")]
    UnknownAction(String),
    #[error("unknown audit visibility {0:?}")]
    UnknownVisibility(String),
    #[error("invalid audit field {field}: {reason}")]
    InvalidField { field: &'static str, reason: String },
    #[error("audit metadata too large: {0}")]
    MetadataTooLarge(String),
    #[error("audit body too large: {0} bytes (max {1})")]
    BodyTooLarge(usize, usize),
    #[error("audit metadata carries secret-bearing material (key or value rejected)")]
    SecretDetected,
    #[error("audit writer backpressure: {0}")]
    Backpressure(String),
    #[error("audit write timed out after {0}ms")]
    Timeout(u64),
    #[error("audit store unavailable: {0}")]
    Unavailable(String),
    #[error("audit storage error: {0}")]
    Storage(#[from] StorageError),
}

impl AuditError {
    /// Stable wire code for `CoreResponse::Error`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownAction(_) => "audit_unknown_action",
            Self::UnknownVisibility(_) => "audit_unknown_visibility",
            Self::InvalidField { .. } => "audit_invalid_field",
            Self::MetadataTooLarge(_) => "audit_metadata_too_large",
            Self::BodyTooLarge(_, _) => "audit_body_too_large",
            Self::SecretDetected => "audit_secret_detected",
            Self::Backpressure(_) => "audit_backpressure",
            Self::Timeout(_) => "audit_write_timeout",
            Self::Unavailable(_) => "audit_unavailable",
            Self::Storage(_) => "audit_storage_error",
        }
    }
}

/// Lowercase substrings that mark a metadata key as secret-bearing.
/// Keys are structural (`session.id`, `job.id`); anything naming a
/// credential is rejected before the write.
const SECRET_KEY_SUBSTRINGS: [&str; 14] = [
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "bearer",
    "credential",
    "private_key",
    "privatekey",
    "cookie",
    "authorization",
    "session_key",
    "client_secret",
];

/// Lowercase substrings that mark a metadata or body value as
/// credential text. Structural values are IDs, digests, and bounded
/// labels; anything scanning as credential text is rejected.
const SECRET_VALUE_SUBSTRINGS: [&str; 16] = [
    "password",
    "passwd",
    "secret",
    "bearer ",
    "private_key",
    "-----begin",
    "sk-live",
    "sk-test",
    "ghp_",
    "gho_",
    "github_token",
    "akia",
    "aws_secret",
    "client_secret",
    "api_key=",
    "token=",
];

fn is_secret_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    SECRET_KEY_SUBSTRINGS
        .iter()
        .any(|needle| lowered.contains(needle))
}

fn value_scans_as_secret(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    SECRET_VALUE_SUBSTRINGS
        .iter()
        .any(|needle| lowered.contains(needle))
}

fn validate_metadata_key(key: &str) -> Result<(), AuditError> {
    if key.is_empty() || key.len() > MAX_METADATA_KEY_LENGTH {
        return Err(AuditError::InvalidField {
            field: "metadata_key",
            reason: format!("key must be 1..={MAX_METADATA_KEY_LENGTH} bytes"),
        });
    }
    let mut chars = key.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => {
            return Err(AuditError::InvalidField {
                field: "metadata_key",
                reason: "key must start with [a-z0-9]".to_owned(),
            });
        }
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.' || c == '-')
    {
        return Err(AuditError::InvalidField {
            field: "metadata_key",
            reason: "key must match [a-z0-9][a-z0-9_.-]*".to_owned(),
        });
    }
    if is_secret_key(key) {
        return Err(AuditError::SecretDetected);
    }
    Ok(())
}

fn validate_metadata(map: &BTreeMap<String, String>) -> Result<(), AuditError> {
    if map.len() > MAX_METADATA_ENTRIES {
        return Err(AuditError::MetadataTooLarge(format!(
            "{} entries (max {MAX_METADATA_ENTRIES})",
            map.len()
        )));
    }
    let mut total = 0usize;
    for (key, value) in map {
        validate_metadata_key(key)?;
        if value.len() > MAX_METADATA_VALUE_LENGTH {
            return Err(AuditError::MetadataTooLarge(format!(
                "value for {key:?} is {} bytes (max {MAX_METADATA_VALUE_LENGTH})",
                value.len()
            )));
        }
        if value_scans_as_secret(value) {
            return Err(AuditError::SecretDetected);
        }
        total += key.len() + value.len();
    }
    if total > MAX_METADATA_TOTAL_BYTES {
        return Err(AuditError::MetadataTooLarge(format!(
            "{total} bytes (max {MAX_METADATA_TOTAL_BYTES})"
        )));
    }
    Ok(())
}

fn validate_bounded_locator(field: &'static str, value: &str) -> Result<(), AuditError> {
    if value.is_empty() || value.len() > MAX_CORRELATION_LENGTH {
        return Err(AuditError::InvalidField {
            field,
            reason: format!("must be 1..={MAX_CORRELATION_LENGTH} bytes"),
        });
    }
    if value.bytes().any(|b| b == 0) || value.chars().any(char::is_control) {
        return Err(AuditError::InvalidField {
            field,
            reason: "must not contain NUL or control characters".to_owned(),
        });
    }
    Ok(())
}

/// SHA-256 hex digest helper.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_bytes(&hasher.finalize())
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Canonical metadata digest: SHA-256 over sorted `key=value\n` lines.
pub fn metadata_digest(metadata: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (key, value) in metadata {
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
        hasher.update(b"\n");
    }
    hex_bytes(&hasher.finalize())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().try_into().unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn auth_method_str(method: AuthMethod) -> &'static str {
    method.as_str()
}

fn transport_class_str(class: TransportClass) -> &'static str {
    class.as_str()
}

/// Decision provenance copied from a gate-enforced M003 authorization
/// decision.
///
/// Production callers obtain this via
/// `codegg_core::authorization::audit_provenance(&decision)` at the daemon
/// boundary, where the gate has already enforced the operation. The struct
/// carries only the decision linkage (ids, policy marker, project); the
/// actor itself always comes from the transport-bound principal, so
/// request payloads can never supply authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditDecisionProvenance {
    decision_id: String,
    correlation_id: String,
    policy: String,
    project: Option<ProjectId>,
}

impl AuditDecisionProvenance {
    pub fn new(
        decision_id: impl Into<String>,
        correlation_id: impl Into<String>,
        policy: impl Into<String>,
        project: Option<ProjectId>,
    ) -> Self {
        Self {
            decision_id: decision_id.into(),
            correlation_id: correlation_id.into(),
            policy: policy.into(),
            project,
        }
    }

    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    pub fn policy(&self) -> &str {
        &self.policy
    }

    pub fn project(&self) -> Option<&ProjectId> {
        self.project.as_ref()
    }
}

/// Trusted builder for one structural audit event.
///
/// The actor, decision, policy, and correlation context come from the
/// transport-bound principal plus the decision provenance copied from the
/// gate-enforced M003 authorization decision. There are deliberately no
/// setters for actor, decision, policy, or sequence: the caller cannot
/// rewrite attribution.
pub struct AuditEventBuilder {
    event_id: AuditEventId,
    action: AuditAction,
    visibility: AuditVisibility,
    principal: AuthenticatedPrincipal,
    provenance: AuditDecisionProvenance,
    correlation_id: String,
    causation_parent: Option<String>,
    project_id: Option<ProjectId>,
    session_id: Option<String>,
    turn_id: Option<String>,
    run_id: Option<String>,
    job_id: Option<String>,
    worktree_id: Option<String>,
    provider_connection_id: Option<String>,
    metadata: BTreeMap<String, String>,
    body: Option<Vec<u8>>,
    body_expires_at: Option<i64>,
}

impl AuditEventBuilder {
    /// Build an event for `action` from trusted authority context.
    ///
    /// `principal` is the transport-bound principal; `provenance` is the
    /// decision linkage copied from the M003 decision that authorized the
    /// operation being audited. Both are copied into the event; the caller
    /// cannot substitute payload-supplied identity.
    pub fn new(
        action: AuditAction,
        principal: &AuthenticatedPrincipal,
        provenance: &AuditDecisionProvenance,
    ) -> Self {
        Self {
            event_id: AuditEventId::new(),
            action,
            visibility: AuditVisibility::Project,
            principal: principal.clone(),
            provenance: provenance.clone(),
            correlation_id: provenance.correlation_id.clone(),
            causation_parent: None,
            project_id: provenance.project.clone(),
            session_id: None,
            turn_id: None,
            run_id: None,
            job_id: None,
            worktree_id: None,
            provider_connection_id: None,
            metadata: BTreeMap::new(),
            body: None,
            body_expires_at: None,
        }
    }

    /// Fixed event id for idempotent retries. Retransmitted appends with
    /// the same id return the stored event instead of duplicating it.
    pub fn with_event_id(mut self, event_id: AuditEventId) -> Self {
        self.event_id = event_id;
        self
    }

    pub fn with_visibility(mut self, visibility: AuditVisibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn with_correlation(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = correlation_id.into();
        self
    }

    pub fn with_causation_parent(mut self, parent_event_id: impl Into<String>) -> Self {
        self.causation_parent = Some(parent_event_id.into());
        self
    }

    pub fn with_project(mut self, project: ProjectId) -> Self {
        self.project_id = Some(project);
        self
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_turn(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }

    pub fn with_run(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    pub fn with_job(mut self, job_id: impl Into<String>) -> Self {
        self.job_id = Some(job_id.into());
        self
    }

    pub fn with_worktree(mut self, worktree_id: impl Into<String>) -> Self {
        self.worktree_id = Some(worktree_id.into());
        self
    }

    pub fn with_provider_connection(mut self, connection_id: impl Into<String>) -> Self {
        self.provider_connection_id = Some(connection_id.into());
        self
    }

    /// Insert one structural metadata entry. Keys and values are
    /// validated at build time; secret-bearing material is rejected.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Attach an opaque body with an independent expiry. Bodies are
    /// referenced by digest; the structural record outlives them.
    pub fn with_body(mut self, body: Vec<u8>, expires_at_ms: Option<i64>) -> Self {
        self.body = Some(body);
        self.body_expires_at = expires_at_ms;
        self
    }

    fn validate(&self) -> Result<(), AuditError> {
        validate_metadata(&self.metadata)?;
        validate_bounded_locator("correlation_id", &self.correlation_id)?;
        validate_bounded_locator("decision_id", self.provenance.decision_id())?;
        validate_bounded_locator("policy", self.provenance.policy())?;
        if let Some(parent) = &self.causation_parent {
            validate_bounded_locator("causation_parent", parent)?;
        }
        for (field, value) in [
            ("session_id", self.session_id.as_ref()),
            ("turn_id", self.turn_id.as_ref()),
            ("run_id", self.run_id.as_ref()),
            ("job_id", self.job_id.as_ref()),
            ("worktree_id", self.worktree_id.as_ref()),
            (
                "provider_connection_id",
                self.provider_connection_id.as_ref(),
            ),
        ]
        .into_iter()
        .filter_map(|(field, value)| value.map(|value| (field, value)))
        {
            validate_bounded_locator(field, value)?;
        }
        if let Some(body) = &self.body {
            if body.len() > MAX_BODY_BYTES {
                return Err(AuditError::BodyTooLarge(body.len(), MAX_BODY_BYTES));
            }
            if !body.is_empty() {
                let lossy = String::from_utf8_lossy(body);
                if value_scans_as_secret(&lossy) {
                    return Err(AuditError::SecretDetected);
                }
            }
        }
        Ok(())
    }

    fn build_parts(self) -> Result<UnsequencedAuditEvent, AuditError> {
        self.validate()?;
        let metadata_digest = metadata_digest(&self.metadata);
        let content_digest = self.body.as_ref().map(|body| sha256_hex(body));
        let body_ref = self
            .body
            .as_ref()
            .map(|_| format!("audit-body-{}", self.event_id));
        Ok(UnsequencedAuditEvent {
            event_id: self.event_id,
            action: self.action,
            visibility: self.visibility,
            principal: self.principal,
            provenance: self.provenance,
            correlation_id: self.correlation_id,
            causation_parent: self.causation_parent,
            project_id: self.project_id,
            session_id: self.session_id,
            turn_id: self.turn_id,
            run_id: self.run_id,
            job_id: self.job_id,
            worktree_id: self.worktree_id,
            provider_connection_id: self.provider_connection_id,
            metadata: self.metadata,
            metadata_digest,
            content_digest,
            body_ref,
            body_expires_at: self.body_expires_at,
            body: self.body,
            time_created_ms: now_millis(),
        })
    }
}

struct UnsequencedAuditEvent {
    event_id: AuditEventId,
    action: AuditAction,
    visibility: AuditVisibility,
    principal: AuthenticatedPrincipal,
    provenance: AuditDecisionProvenance,
    correlation_id: String,
    causation_parent: Option<String>,
    project_id: Option<ProjectId>,
    session_id: Option<String>,
    turn_id: Option<String>,
    run_id: Option<String>,
    job_id: Option<String>,
    worktree_id: Option<String>,
    provider_connection_id: Option<String>,
    metadata: BTreeMap<String, String>,
    metadata_digest: String,
    content_digest: Option<String>,
    body_ref: Option<String>,
    body_expires_at: Option<i64>,
    body: Option<Vec<u8>>,
    time_created_ms: i64,
}

/// One durable structural audit event with its coordinator sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub seq: u64,
    pub event_id: AuditEventId,
    pub action: String,
    pub visibility: String,
    pub actor_principal: PrincipalId,
    pub actor_kind: PrincipalKind,
    pub auth_method: AuthMethod,
    pub transport_class: TransportClass,
    pub policy: String,
    pub decision_id: String,
    pub correlation_id: String,
    pub causation_parent: Option<String>,
    pub project_id: Option<ProjectId>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub run_id: Option<String>,
    pub job_id: Option<String>,
    pub worktree_id: Option<String>,
    pub provider_connection_id: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub metadata_digest: String,
    pub content_digest: Option<String>,
    pub body_ref: Option<String>,
    pub body_expires_at: Option<i64>,
    pub time_created_ms: i64,
}

impl AuditEvent {
    /// Wire DTO for query/page/export responses.
    pub fn to_dto(&self) -> codegg_protocol::core::AuditEventDto {
        codegg_protocol::core::AuditEventDto {
            seq: self.seq,
            event_id: self.event_id.as_str().to_owned(),
            action: self.action.clone(),
            visibility: self.visibility.clone(),
            actor_principal: self.actor_principal.as_str().to_owned(),
            auth_method: self.auth_method.as_str().to_owned(),
            transport_class: self.transport_class.as_str().to_owned(),
            policy: self.policy.clone(),
            decision_id: self.decision_id.clone(),
            correlation_id: self.correlation_id.clone(),
            causation_parent: self.causation_parent.clone(),
            project_id: self.project_id.as_ref().map(|id| id.as_str().to_owned()),
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
            run_id: self.run_id.clone(),
            job_id: self.job_id.clone(),
            worktree_id: self.worktree_id.clone(),
            provider_connection_id: self.provider_connection_id.clone(),
            metadata: self.metadata.clone(),
            metadata_digest: self.metadata_digest.clone(),
            content_digest: self.content_digest.clone(),
            body_ref: self.body_ref.clone(),
            body_expires_at: self.body_expires_at,
            time_created: self.time_created_ms,
        }
    }
}

/// Bounded query filter. `limit` is clamped to [`MAX_QUERY_LIMIT`];
/// unknown action/principal filters degrade to empty pages downstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditQueryFilter {
    pub project_id: Option<ProjectId>,
    pub action: Option<String>,
    pub principal: Option<PrincipalId>,
    pub from_seq: Option<u64>,
    pub limit: u32,
}

impl AuditQueryFilter {
    pub fn new(project: Option<ProjectId>) -> Self {
        Self {
            project_id: project,
            action: None,
            principal: None,
            from_seq: None,
            limit: DEFAULT_QUERY_LIMIT,
        }
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    pub fn with_principal(mut self, principal: PrincipalId) -> Self {
        self.principal = Some(principal);
        self
    }

    pub fn with_from_seq(mut self, from_seq: u64) -> Self {
        self.from_seq = Some(from_seq);
        self
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }

    /// Clamp the limit into `1..=MAX_QUERY_LIMIT`.
    pub fn normalized_limit(&self) -> i64 {
        self.limit.clamp(1, MAX_QUERY_LIMIT) as i64
    }

    pub fn normalized_export_limit(&self) -> i64 {
        self.limit.clamp(1, MAX_EXPORT_EVENTS) as i64
    }
}

/// Bounded ordered page. `next_cursor` is the `seq` to resume from;
/// `None` means the log is exhausted at read time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditPage {
    pub events: Vec<AuditEvent>,
    pub next_cursor: Option<u64>,
    pub truncated: bool,
}

/// Bounded export envelope with an integrity digest over the canonical
/// event ordering (`seq:event_id:action:actor:decision:metadata:content`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditExport {
    pub events: Vec<AuditEvent>,
    pub digest: String,
    pub count: usize,
    pub exported_at_ms: i64,
}

/// Compute the export integrity digest for `events` in page order.
pub fn export_digest(events: &[AuditEvent]) -> String {
    let mut hasher = Sha256::new();
    for event in events {
        hasher.update(event.seq.to_le_bytes());
        hasher.update(event.event_id.as_str().as_bytes());
        hasher.update(b"|");
        hasher.update(event.action.as_bytes());
        hasher.update(b"|");
        hasher.update(event.actor_principal.as_str().as_bytes());
        hasher.update(b"|");
        hasher.update(event.decision_id.as_bytes());
        hasher.update(b"|");
        hasher.update(event.metadata_digest.as_bytes());
        hasher.update(b"|");
        hasher.update(event.content_digest.as_deref().unwrap_or("-").as_bytes());
        hasher.update(b"\n");
    }
    hex_bytes(&hasher.finalize())
}

/// Coordinator-owned append-only audit store over the daemon catalog.
#[derive(Clone)]
pub struct AuditStore {
    pool: SqlitePool,
}

impl AuditStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Transactional idempotent append.
    ///
    /// The coordinator assigns `seq` via `AUTOINCREMENT` inside one
    /// statement. A duplicate `event_id` returns the stored event
    /// unchanged and never assigns a second sequence number.
    pub async fn append(&self, builder: AuditEventBuilder) -> Result<AuditEvent, AuditError> {
        let parts = builder.build_parts()?;
        let metadata_json =
            serde_json::to_string(&parts.metadata).map_err(|e| AuditError::InvalidField {
                field: "metadata",
                reason: e.to_string(),
            })?;
        sqlx::query(
            "INSERT INTO audit_event (event_id, action, visibility, actor_principal, actor_kind, \
             auth_method, transport_class, policy, decision_id, correlation_id, causation_parent, \
             project_id, session_id, turn_id, run_id, job_id, worktree_id, provider_connection_id, \
             metadata_json, metadata_digest, content_digest, body_ref, body_expires_at, time_created) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(event_id) DO NOTHING",
        )
        .bind(parts.event_id.as_str())
        .bind(parts.action.as_str())
        .bind(parts.visibility.as_str())
        .bind(parts.principal.principal_id().as_str())
        .bind(parts.principal.kind().as_str())
        .bind(auth_method_str(parts.principal.auth_method()))
        .bind(transport_class_str(parts.principal.transport_class()))
        .bind(parts.provenance.policy().to_owned())
        .bind(parts.provenance.decision_id().to_owned())
        .bind(&parts.correlation_id)
        .bind(&parts.causation_parent)
        .bind(parts.project_id.as_ref().map(|id| id.as_str()))
        .bind(&parts.session_id)
        .bind(&parts.turn_id)
        .bind(&parts.run_id)
        .bind(&parts.job_id)
        .bind(&parts.worktree_id)
        .bind(&parts.provider_connection_id)
        .bind(&metadata_json)
        .bind(&parts.metadata_digest)
        .bind(&parts.content_digest)
        .bind(&parts.body_ref)
        .bind(parts.body_expires_at)
        .bind(parts.time_created_ms)
        .execute(&self.pool)
        .await
        .map_err(|e| AuditError::Storage(StorageError::Database(e.to_string())))?;
        if let (Some(body), Some(body_ref)) = (parts.body, parts.body_ref.clone()) {
            let digest = parts
                .content_digest
                .clone()
                .unwrap_or_else(|| sha256_hex(&body));
            sqlx::query(
                "INSERT INTO audit_body (body_ref, event_id, content_digest, body, byte_length, \
                 expires_at, time_created) VALUES (?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(body_ref) DO NOTHING",
            )
            .bind(&body_ref)
            .bind(parts.event_id.as_str())
            .bind(&digest)
            .bind(&body)
            .bind(body.len() as i64)
            .bind(parts.body_expires_at)
            .bind(parts.time_created_ms)
            .execute(&self.pool)
            .await
            .map_err(|e| AuditError::Storage(StorageError::Database(e.to_string())))?;
        }
        self.get_by_id(&parts.event_id)
            .await?
            .ok_or_else(|| AuditError::Unavailable("audit append lost".to_owned()))
    }

    /// Fetch one event by id, if present.
    pub async fn get_by_id(
        &self,
        event_id: &AuditEventId,
    ) -> Result<Option<AuditEvent>, AuditError> {
        let row: Option<AuditRow> = sqlx::query_as(
            "SELECT seq, event_id, action, visibility, actor_principal, actor_kind, auth_method, \
             transport_class, policy, decision_id, correlation_id, causation_parent, project_id, \
             session_id, turn_id, run_id, job_id, worktree_id, provider_connection_id, \
             metadata_json, metadata_digest, content_digest, body_ref, body_expires_at, \
             time_created FROM audit_event WHERE event_id = ?",
        )
        .bind(event_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AuditError::Storage(StorageError::Database(e.to_string())))?;
        row.map(decode_row).transpose()
    }

    /// Raw bounded query. The daemon gate enforces `audit.read` before
    /// this is reached (`audit_query` is `DirectProject` + `audit.read` in
    /// the operation matrix); direct callers own their own authorization.
    pub async fn query(&self, filter: &AuditQueryFilter) -> Result<AuditPage, AuditError> {
        let limit = filter.normalized_limit();
        let from_seq = filter.from_seq.unwrap_or(0) as i64;
        let rows: Vec<AuditRow> = sqlx::query_as(
            "SELECT seq, event_id, action, visibility, actor_principal, actor_kind, auth_method, \
             transport_class, policy, decision_id, correlation_id, causation_parent, project_id, \
             session_id, turn_id, run_id, job_id, worktree_id, provider_connection_id, \
             metadata_json, metadata_digest, content_digest, body_ref, body_expires_at, \
             time_created FROM audit_event \
             WHERE seq >= ? AND (? IS NULL OR project_id = ?) AND (? IS NULL OR action = ?) AND \
             (? IS NULL OR actor_principal = ?) ORDER BY seq ASC LIMIT ?",
        )
        .bind(from_seq)
        .bind(filter.project_id.as_ref().map(|id| id.as_str()))
        .bind(filter.project_id.as_ref().map(|id| id.as_str()))
        .bind(filter.action.as_deref())
        .bind(filter.action.as_deref())
        .bind(filter.principal.as_ref().map(|id| id.as_str()))
        .bind(filter.principal.as_ref().map(|id| id.as_str()))
        .bind(limit + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AuditError::Storage(StorageError::Database(e.to_string())))?;
        let truncated = rows.len() as i64 > limit;
        let mut events = Vec::with_capacity(rows.len().min(limit as usize));
        for row in rows.into_iter().take(limit as usize) {
            events.push(decode_row(row)?);
        }
        let next_cursor = if truncated {
            events.last().map(|event| event.seq + 1)
        } else {
            None
        };
        Ok(AuditPage {
            events,
            next_cursor,
            truncated,
        })
    }

    /// Bounded export with an integrity digest over the canonical event
    /// ordering. Like [`Self::query`], authorization is enforced by the
    /// daemon gate before this is reached.
    pub async fn export(&self, filter: &AuditQueryFilter) -> Result<AuditExport, AuditError> {
        let page = self.query(filter).await?;
        let bounded: Vec<AuditEvent> = page
            .events
            .into_iter()
            .take(filter.normalized_export_limit() as usize)
            .collect();
        let digest = export_digest(&bounded);
        Ok(AuditExport {
            count: bounded.len(),
            digest,
            events: bounded,
            exported_at_ms: now_millis(),
        })
    }

    /// Read one retained body by reference. Returns `None` when the body
    /// expired or was never stored; the structural event is unaffected.
    pub async fn read_body(&self, body_ref: &str) -> Result<Option<Vec<u8>>, AuditError> {
        validate_bounded_locator("body_ref", body_ref)?;
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT body FROM audit_body WHERE body_ref = ?")
                .bind(body_ref)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AuditError::Storage(StorageError::Database(e.to_string())))?;
        Ok(row.map(|(body,)| body))
    }

    /// Expire bodies at or before `now_ms`. Structural events and their
    /// digests are preserved; only `audit_body` rows are deleted.
    /// Returns the number of body rows removed.
    pub async fn expire_bodies(&self, now_ms: i64) -> Result<u64, AuditError> {
        let result =
            sqlx::query("DELETE FROM audit_body WHERE expires_at IS NOT NULL AND expires_at <= ?")
                .bind(now_ms)
                .execute(&self.pool)
                .await
                .map_err(|e| AuditError::Storage(StorageError::Database(e.to_string())))?;
        Ok(result.rows_affected())
    }

    /// Highest assigned coordinator sequence, if any event exists.
    pub async fn max_seq(&self) -> Result<Option<u64>, AuditError> {
        let row: (Option<i64>,) = sqlx::query_as("SELECT MAX(seq) FROM audit_event")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AuditError::Storage(StorageError::Database(e.to_string())))?;
        Ok(row.0.map(|seq| seq as u64))
    }

    /// Number of structural events stored.
    pub async fn count(&self) -> Result<u64, AuditError> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_event")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AuditError::Storage(StorageError::Database(e.to_string())))?;
        Ok(row.0 as u64)
    }
}

#[derive(Debug, sqlx::FromRow)]
struct AuditRow {
    seq: i64,
    event_id: String,
    action: String,
    visibility: String,
    actor_principal: String,
    actor_kind: String,
    auth_method: String,
    transport_class: String,
    policy: String,
    decision_id: String,
    correlation_id: String,
    causation_parent: Option<String>,
    project_id: Option<String>,
    session_id: Option<String>,
    turn_id: Option<String>,
    run_id: Option<String>,
    job_id: Option<String>,
    worktree_id: Option<String>,
    provider_connection_id: Option<String>,
    metadata_json: String,
    metadata_digest: String,
    content_digest: Option<String>,
    body_ref: Option<String>,
    body_expires_at: Option<i64>,
    time_created: i64,
}

fn decode_row(row: AuditRow) -> Result<AuditEvent, AuditError> {
    let event_id = AuditEventId::parse(&row.event_id).map_err(|e| AuditError::InvalidField {
        field: "event_id",
        reason: e.to_string(),
    })?;
    let actor_principal =
        PrincipalId::parse(&row.actor_principal).map_err(|e| AuditError::InvalidField {
            field: "actor_principal",
            reason: e.to_string(),
        })?;
    let project_id = row
        .project_id
        .map(|raw| ProjectId::parse(&raw))
        .transpose()
        .map_err(|e| AuditError::InvalidField {
            field: "project_id",
            reason: e.to_string(),
        })?;
    let metadata: BTreeMap<String, String> =
        serde_json::from_str(&row.metadata_json).map_err(|e| AuditError::InvalidField {
            field: "metadata",
            reason: e.to_string(),
        })?;
    // Unknown actions/visibilities degrade safely: the stored string is
    // preserved verbatim so export digests stay stable.
    Ok(AuditEvent {
        seq: row.seq as u64,
        event_id,
        action: row.action,
        visibility: row.visibility,
        actor_principal,
        actor_kind: crate::team::PrincipalKind::parse_for_audit(&row.actor_kind)
            .unwrap_or(crate::team::PrincipalKind::LocalOwner),
        auth_method: crate::transport_auth::AuthMethod::parse_for_audit(&row.auth_method)
            .unwrap_or(crate::transport_auth::AuthMethod::LocalOwner),
        transport_class: crate::transport_auth::TransportClass::parse_for_audit(
            &row.transport_class,
        )
        .unwrap_or(crate::transport_auth::TransportClass::Local),
        policy: row.policy,
        decision_id: row.decision_id,
        correlation_id: row.correlation_id,
        causation_parent: row.causation_parent,
        project_id,
        session_id: row.session_id,
        turn_id: row.turn_id,
        run_id: row.run_id,
        job_id: row.job_id,
        worktree_id: row.worktree_id,
        provider_connection_id: row.provider_connection_id,
        metadata,
        metadata_digest: row.metadata_digest,
        content_digest: row.content_digest,
        body_ref: row.body_ref,
        body_expires_at: row.body_expires_at,
        time_created_ms: row.time_created,
    })
}

/// When audit writes must fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditFailurePolicy {
    /// Surface the storage error to the caller; never fabricate success.
    FailClosed,
    /// Record the failure in metrics and return it as an error without
    /// retrying forever. The operation is still reported as failed, never
    /// as appended.
    FailVisible,
}

/// Bounded writer configuration. No unbounded queue exists: at most
/// `max_inflight` appends proceed concurrently and each append is
/// bounded by `write_timeout`.
#[derive(Debug, Clone, Copy)]
pub struct AuditWriterConfig {
    pub max_inflight: usize,
    pub write_timeout_ms: u64,
    pub failure_policy: AuditFailurePolicy,
}

impl Default for AuditWriterConfig {
    fn default() -> Self {
        Self {
            max_inflight: 32,
            write_timeout_ms: 2000,
            failure_policy: AuditFailurePolicy::FailClosed,
        }
    }
}

/// Observable writer counters. All values are monotonic within the
/// process; `queue_depth` is the current semaphore utilization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditWriterMetrics {
    pub appended: u64,
    pub duplicates: u64,
    pub failed: u64,
    pub dropped_backpressure: u64,
    pub timeouts: u64,
    pub queue_depth: u64,
}

/// Bounded coordinator writer with explicit backpressure.
///
/// The writer never blocks forever: [`Self::try_append`] fails fast when
/// no permit is available, and [`Self::append`] bounds both permit
/// acquisition and the store write with [`AuditWriterConfig::write_timeout_ms`].
/// Failures are counted and surfaced; success is never fabricated.
#[derive(Clone)]
pub struct AuditWriter {
    store: AuditStore,
    config: AuditWriterConfig,
    semaphore: Arc<Semaphore>,
    appended: Arc<AtomicU64>,
    duplicates: Arc<AtomicU64>,
    failed: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    timeouts: Arc<AtomicU64>,
    inflight: Arc<AtomicU64>,
}

impl AuditWriter {
    pub fn new(store: AuditStore, config: AuditWriterConfig) -> Self {
        let max = config.max_inflight.max(1);
        Self {
            store,
            config,
            semaphore: Arc::new(Semaphore::new(max)),
            appended: Arc::new(AtomicU64::new(0)),
            duplicates: Arc::new(AtomicU64::new(0)),
            failed: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            timeouts: Arc::new(AtomicU64::new(0)),
            inflight: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn store(&self) -> &AuditStore {
        &self.store
    }

    pub fn metrics(&self) -> AuditWriterMetrics {
        AuditWriterMetrics {
            appended: self.appended.load(Ordering::Relaxed),
            duplicates: self.duplicates.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            dropped_backpressure: self.dropped.load(Ordering::Relaxed),
            timeouts: self.timeouts.load(Ordering::Relaxed),
            queue_depth: self.inflight.load(Ordering::Relaxed),
        }
    }

    /// Fail-fast append: returns [`AuditError::Backpressure`] without
    /// waiting when all writer permits are in use.
    pub async fn try_append(&self, builder: AuditEventBuilder) -> Result<AuditEvent, AuditError> {
        let Ok(_permit) = self.semaphore.try_acquire() else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            self.failed.fetch_add(1, Ordering::Relaxed);
            return Err(AuditError::Backpressure(format!(
                "audit writer saturated (max_inflight={})",
                self.config.max_inflight
            )));
        };
        self.append_inner(builder).await
    }

    /// Bounded append: permit acquisition and the store write each
    /// respect the configured timeout. Never blocks forever.
    pub async fn append(&self, builder: AuditEventBuilder) -> Result<AuditEvent, AuditError> {
        let write_timeout = Duration::from_millis(self.config.write_timeout_ms);
        let permit = timeout(write_timeout, self.semaphore.acquire())
            .await
            .map_err(|_| {
                self.timeouts.fetch_add(1, Ordering::Relaxed);
                self.failed.fetch_add(1, Ordering::Relaxed);
                AuditError::Timeout(self.config.write_timeout_ms)
            })?
            .map_err(|_| AuditError::Backpressure("audit writer closed".to_owned()))?;
        let _held = permit;
        self.append_inner_with_timeout(builder, write_timeout).await
    }

    async fn append_inner(&self, builder: AuditEventBuilder) -> Result<AuditEvent, AuditError> {
        self.append_inner_with_timeout(builder, Duration::from_millis(self.config.write_timeout_ms))
            .await
    }

    async fn append_inner_with_timeout(
        &self,
        builder: AuditEventBuilder,
        write_timeout: Duration,
    ) -> Result<AuditEvent, AuditError> {
        // Idempotency pre-check: a retransmitted event id returns the
        // stored event without consuming backpressure or a new sequence.
        // The event id is not known until the builder is consumed, so the
        // authoritative duplicate check stays inside the transactional
        // append below; this counter records post-write duplicates.
        self.inflight.fetch_add(1, Ordering::Relaxed);
        let before = self.store.count().await.unwrap_or(0);
        let outcome = timeout(write_timeout, self.store.append(builder)).await;
        self.inflight.fetch_sub(1, Ordering::Relaxed);
        match outcome {
            Err(_) => {
                self.timeouts.fetch_add(1, Ordering::Relaxed);
                self.failed.fetch_add(1, Ordering::Relaxed);
                Err(AuditError::Timeout(self.config.write_timeout_ms))
            }
            Ok(Err(error)) => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                let _ = self.config.failure_policy;
                Err(error)
            }
            Ok(Ok(event)) => {
                let after = self.store.count().await.unwrap_or(before + 1);
                if after == before {
                    self.duplicates.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.appended.fetch_add(1, Ordering::Relaxed);
                }
                Ok(event)
            }
        }
    }
}

/// Capability negotiation response for the audit surface.
pub fn audit_capabilities_dto() -> codegg_protocol::core::AuditCapabilitiesDto {
    codegg_protocol::core::AuditCapabilitiesDto {
        supported: true,
        max_query_limit: MAX_QUERY_LIMIT,
        max_export_events: MAX_EXPORT_EVENTS,
        max_metadata_entries: MAX_METADATA_ENTRIES as u32,
        max_metadata_total_bytes: MAX_METADATA_TOTAL_BYTES as u32,
        max_body_bytes: MAX_BODY_BYTES as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate test store");
        pool
    }

    fn local_context(correlation: &str) -> (AuthenticatedPrincipal, AuditDecisionProvenance) {
        let principal = AuthenticatedPrincipal::local_owner("client-test");
        let provenance =
            AuditDecisionProvenance::new("decision-test", correlation, "local_owner_broad", None);
        (principal, provenance)
    }

    fn test_event(
        principal: &AuthenticatedPrincipal,
        provenance: &AuditDecisionProvenance,
    ) -> AuditEventBuilder {
        AuditEventBuilder::new(AuditAction::SessionCreate, principal, provenance)
            .with_metadata("session.id", "session-1")
            .with_metadata("project.id", "project-1")
    }

    #[test]
    fn action_taxonomy_round_trips_and_rejects_unknown_writes() {
        for known in AuditAction::ALL {
            let parsed = AuditAction::parse_known(known).expect("known action");
            assert_eq!(parsed.as_str(), known);
        }
        assert!(AuditAction::parse_known("drop_table").is_err());
        assert_eq!(
            AuditAction::parse_lenient("future_action"),
            AuditAction::Unknown
        );
        assert_eq!(
            AuditAction::parse_lenient("session_create").as_str(),
            "session_create"
        );
    }

    #[test]
    fn visibility_taxonomy_rejects_unknown() {
        assert_eq!(
            AuditVisibility::parse("project").unwrap(),
            AuditVisibility::Project
        );
        assert!(AuditVisibility::parse("everyone").is_err());
    }

    #[test]
    fn metadata_digest_is_deterministic_and_order_independent() {
        let mut first = BTreeMap::new();
        first.insert("b".to_owned(), "2".to_owned());
        first.insert("a".to_owned(), "1".to_owned());
        let mut second = BTreeMap::new();
        second.insert("a".to_owned(), "1".to_owned());
        second.insert("b".to_owned(), "2".to_owned());
        assert_eq!(metadata_digest(&first), metadata_digest(&second));
        second.insert("c".to_owned(), "3".to_owned());
        assert_ne!(metadata_digest(&first), metadata_digest(&second));
    }

    #[test]
    fn secret_keys_and_values_are_rejected() {
        assert!(is_secret_key("api_token"));
        assert!(is_secret_key("db_PASSWORD"));
        assert!(!is_secret_key("session.id"));
        assert!(value_scans_as_secret("value ghp_abc123"));
        assert!(value_scans_as_secret("-----BEGIN PRIVATE KEY-----"));
        assert!(!value_scans_as_secret("session-1"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn append_assigns_sequence_and_returns_stored_event() {
        let pool = migrated_pool().await;
        let store = AuditStore::new(pool);
        let (principal, decision) = local_context("corr-1");
        let event = store
            .append(test_event(&principal, &decision))
            .await
            .expect("append");
        assert!(event.seq >= 1);
        assert_eq!(event.action, "session_create");
        assert_eq!(event.actor_principal.as_str(), "local-owner");
        assert_eq!(event.decision_id, "decision-test");
        assert_eq!(
            event.metadata.get("session.id").map(String::as_str),
            Some("session-1")
        );
        assert_eq!(event.metadata_digest, metadata_digest(&event.metadata));
        let fetched = store
            .get_by_id(&event.event_id)
            .await
            .unwrap()
            .expect("row");
        assert_eq!(fetched, event);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn duplicate_event_id_is_idempotent() {
        let pool = migrated_pool().await;
        let store = AuditStore::new(pool);
        let (principal, decision) = local_context("corr-dup");
        let event_id = AuditEventId::new();
        let first = store
            .append(
                AuditEventBuilder::new(AuditAction::ToolInvoke, &principal, &decision)
                    .with_event_id(event_id.clone())
                    .with_metadata("tool.name", "read"),
            )
            .await
            .expect("first append");
        let second = store
            .append(
                AuditEventBuilder::new(AuditAction::ToolInvoke, &principal, &decision)
                    .with_event_id(event_id)
                    .with_metadata("tool.name", "read"),
            )
            .await
            .expect("duplicate append");
        assert_eq!(first.seq, second.seq);
        assert_eq!(first.event_id, second.event_id);
        assert_eq!(store.count().await.unwrap(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn secret_metadata_never_reaches_storage() {
        let pool = migrated_pool().await;
        let store = AuditStore::new(pool);
        let (principal, decision) = local_context("corr-secret");
        let error = store
            .append(
                AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision)
                    .with_metadata("api_token", "ghp_sentinel"),
            )
            .await
            .expect_err("secret key must be rejected");
        assert!(matches!(error, AuditError::SecretDetected));
        let error = store
            .append(
                AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision)
                    .with_metadata("note", "contains password=hunter2"),
            )
            .await
            .expect_err("secret value must be rejected");
        assert!(matches!(error, AuditError::SecretDetected));
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn metadata_and_body_bounds_are_enforced() {
        let pool = migrated_pool().await;
        let store = AuditStore::new(pool);
        let (principal, decision) = local_context("corr-bounds");
        let mut oversized =
            AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision);
        for index in 0..(MAX_METADATA_ENTRIES + 1) {
            oversized = oversized.with_metadata(format!("key.{index:02}"), "v");
        }
        assert!(matches!(
            store.append(oversized).await.expect_err("entry bound"),
            AuditError::MetadataTooLarge(_)
        ));
        let oversized_value = "x".repeat(MAX_METADATA_VALUE_LENGTH + 1);
        let error = store
            .append(
                AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision)
                    .with_metadata("note", oversized_value),
            )
            .await
            .expect_err("value bound");
        assert!(matches!(error, AuditError::MetadataTooLarge(_)));
        let oversized_body = vec![b'a'; MAX_BODY_BYTES + 1];
        let error = store
            .append(
                AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &decision)
                    .with_body(oversized_body, None),
            )
            .await
            .expect_err("body bound");
        assert!(matches!(error, AuditError::BodyTooLarge(_, _)));
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn body_retention_expiry_preserves_structure() {
        let pool = migrated_pool().await;
        let store = AuditStore::new(pool);
        let (principal, decision) = local_context("corr-retention");
        let event = store
            .append(
                AuditEventBuilder::new(AuditAction::FileMutate, &principal, &decision)
                    .with_metadata("file.path_hash", "abc123")
                    .with_body(b"structural-body".to_vec(), Some(100)),
            )
            .await
            .expect("append with body");
        let body_ref = event.body_ref.clone().expect("body ref");
        assert!(event.content_digest.is_some());
        assert!(store.read_body(&body_ref).await.unwrap().is_some());
        assert_eq!(store.expire_bodies(99).await.unwrap(), 0);
        assert!(store.read_body(&body_ref).await.unwrap().is_some());
        assert_eq!(store.expire_bodies(100).await.unwrap(), 1);
        assert!(store.read_body(&body_ref).await.unwrap().is_none());
        let structural = store
            .get_by_id(&event.event_id)
            .await
            .unwrap()
            .expect("kept");
        assert_eq!(structural.seq, event.seq);
        assert_eq!(structural.content_digest, event.content_digest);
        assert_eq!(store.count().await.unwrap(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pagination_and_filters_are_stable() {
        let pool = migrated_pool().await;
        let store = AuditStore::new(pool);
        let (principal, decision) = local_context("corr-page");
        for index in 0..5 {
            let action = if index % 2 == 0 {
                AuditAction::SessionCreate
            } else {
                AuditAction::ToolInvoke
            };
            store
                .append(
                    AuditEventBuilder::new(action, &principal, &decision)
                        .with_metadata("index", index.to_string()),
                )
                .await
                .expect("append");
        }
        let first = store
            .query(&AuditQueryFilter::new(None).with_limit(2))
            .await
            .expect("page one");
        assert_eq!(first.events.len(), 2);
        assert!(first.truncated);
        let cursor = first.next_cursor.expect("cursor");
        let second = store
            .query(
                &AuditQueryFilter::new(None)
                    .with_limit(10)
                    .with_from_seq(cursor),
            )
            .await
            .expect("page two");
        assert_eq!(second.events.len(), 3);
        assert!(!second.truncated);
        assert!(second.events.iter().all(|event| event.seq >= cursor));
        let filtered = store
            .query(
                &AuditQueryFilter::new(None)
                    .with_limit(10)
                    .with_action("tool_invoke"),
            )
            .await
            .expect("filtered");
        assert_eq!(filtered.events.len(), 2);
        assert!(filtered
            .events
            .iter()
            .all(|event| event.action == "tool_invoke"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decision_provenance_is_copied_without_authority_setters() {
        let pool = migrated_pool().await;
        let store = AuditStore::new(pool);
        let (principal, _provenance) = local_context("corr-provenance");
        let project = ProjectId::new();
        let scoped = AuditDecisionProvenance::new(
            "decision-scoped",
            "corr-scoped",
            "team_membership",
            Some(project.clone()),
        );
        let event = store
            .append(
                AuditEventBuilder::new(AuditAction::SessionCreate, &principal, &scoped)
                    .with_metadata("session.id", "session-9"),
            )
            .await
            .expect("append");
        // Actor comes from the transport-bound principal; decision linkage
        // comes from the provenance — neither is settable from a payload.
        assert_eq!(event.actor_principal.as_str(), "local-owner");
        assert_eq!(event.decision_id, "decision-scoped");
        assert_eq!(event.correlation_id, "corr-scoped");
        assert_eq!(event.policy, "team_membership");
        assert_eq!(event.project_id, Some(project));
        // Unknown stored actions degrade safely on read: insert one row
        // with a future action through the append path shape and confirm
        // the reader preserves it verbatim while classifying it Unknown.
        let future_id = AuditEventId::new();
        sqlx::query(
            "INSERT INTO audit_event (event_id, action, visibility, actor_principal, actor_kind, \
             auth_method, transport_class, policy, decision_id, correlation_id, metadata_json, \
             metadata_digest, time_created) VALUES (?, 'future_action', 'project', 'local-owner', \
             'local_owner', 'local_owner', 'local', 'local_owner_broad', 'decision-future', \
             'corr-future', '{}', 'digest-future', 1)",
        )
        .bind(future_id.as_str())
        .execute(store.pool())
        .await
        .expect("test-only insert");
        let reread = store.get_by_id(&future_id).await.unwrap().expect("row");
        assert_eq!(reread.action, "future_action");
        assert_eq!(
            AuditAction::parse_lenient(&reread.action),
            AuditAction::Unknown
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn export_digest_is_stable_and_covers_ordering() {
        let pool = migrated_pool().await;
        let store = AuditStore::new(pool);
        let (principal, decision) = local_context("corr-export");
        for index in 0..3 {
            store
                .append(
                    AuditEventBuilder::new(AuditAction::GitOperation, &principal, &decision)
                        .with_metadata("index", index.to_string()),
                )
                .await
                .expect("append");
        }
        let page = store
            .query(&AuditQueryFilter::new(None).with_limit(10))
            .await
            .expect("query");
        let first_digest = export_digest(&page.events);
        let second_digest = export_digest(&page.events);
        assert_eq!(first_digest, second_digest);
        assert_eq!(first_digest.len(), 64);
        let mut reordered = page.events.clone();
        reordered.reverse();
        assert_ne!(export_digest(&reordered), first_digest);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn writer_backpressure_is_bounded_and_observable() {
        let pool = migrated_pool().await;
        let store = AuditStore::new(pool);
        let writer = AuditWriter::new(
            store,
            AuditWriterConfig {
                max_inflight: 1,
                write_timeout_ms: 2000,
                failure_policy: AuditFailurePolicy::FailVisible,
            },
        );
        let (principal, decision) = local_context("corr-writer");
        let _guard = writer.semaphore.acquire().await.expect("permit");
        let error = writer
            .try_append(test_event(&principal, &decision))
            .await
            .expect_err("saturated writer");
        assert!(matches!(error, AuditError::Backpressure(_)));
        assert_eq!(writer.metrics().dropped_backpressure, 1);
        drop(_guard);
        let event = writer
            .try_append(test_event(&principal, &decision))
            .await
            .expect("writer recovers");
        assert!(event.seq >= 1);
        assert_eq!(writer.metrics().appended, 1);
    }
}
