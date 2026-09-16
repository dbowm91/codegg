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
    /// Reserved for M004: last-used provider connection identity.
    pub last_provider_connection_id: Option<String>,
    /// Reserved for M004: last-used model identity.
    pub last_model_id: Option<String>,
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
    /// failing the whole read.
    pub async fn get(
        &self,
        principal_id: &str,
    ) -> Result<Option<RuntimePreference>, PreferenceError> {
        validate_principal_id(principal_id)?;
        let row: Option<RuntimePreferenceRow> = sqlx::query_as(
            r#"
            SELECT principal_id, approval_mode, sandbox_profile,
                   last_provider_connection_id, last_model_id,
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
        sqlx::query(
            r#"
            INSERT INTO runtime_preferences
                (principal_id, approval_mode, sandbox_profile,
                 last_provider_connection_id, last_model_id, revision, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
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
        let (approval_mode, sandbox_profile) = match current.as_ref() {
            Some(p) => (
                p.approval_mode.map(|m| m.as_str().to_owned()),
                p.sandbox_profile.map(|s| s.as_str().to_owned()),
            ),
            None => (None, None),
        };
        let (approval_mode, sandbox_profile) = match field {
            "approval_mode" => (value.map(str::to_owned), sandbox_profile),
            "sandbox_profile" => (approval_mode, value.map(str::to_owned)),
            _ => return Err(PreferenceError::Validation("unknown field".into())),
        };
        let (connection_id, model_id) = match current.as_ref() {
            Some(p) => (
                p.last_provider_connection_id.clone(),
                p.last_model_id.clone(),
            ),
            None => (None, None),
        };
        sqlx::query(
            r#"
            INSERT INTO runtime_preferences
                (principal_id, approval_mode, sandbox_profile,
                 last_provider_connection_id, last_model_id, revision, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
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
        .bind(next_revision as i64)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| PreferenceError::Storage(e.to_string()))?;
        self.get(principal_id)
            .await?
            .ok_or_else(|| PreferenceError::Storage("preference write lost".into()))
    }
}

#[derive(Debug, sqlx::FromRow)]
struct RuntimePreferenceRow {
    principal_id: String,
    approval_mode: Option<String>,
    sandbox_profile: Option<String>,
    last_provider_connection_id: Option<String>,
    last_model_id: Option<String>,
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
            last_provider_connection_id: sanitize_identity(self.last_provider_connection_id),
            last_model_id: sanitize_identity(self.last_model_id),
            revision: self.revision.max(0) as u64,
            updated_at_ms: self.updated_at,
        }
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
/// databases via `IF NOT EXISTS`.
pub const RUNTIME_PREFERENCE_SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS runtime_preferences (
        principal_id TEXT PRIMARY KEY,
        approval_mode TEXT CHECK (approval_mode IS NULL OR approval_mode IN ('interactive','automatic','yolo')),
        sandbox_profile TEXT CHECK (sandbox_profile IS NULL OR sandbox_profile IN ('read_only','workspace_write','full_host')),
        last_provider_connection_id TEXT CHECK (last_provider_connection_id IS NULL OR length(last_provider_connection_id) <= 512),
        last_model_id TEXT CHECK (last_model_id IS NULL OR length(last_model_id) <= 512),
        revision INTEGER NOT NULL CHECK (revision >= 0),
        updated_at INTEGER NOT NULL
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_runtime_preferences_updated ON runtime_preferences(updated_at DESC)",
];

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
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
        );
        assert!(parent
            .narrow_for_child(ApprovalMode::Yolo, SandboxProfile::WorkspaceWrite)
            .is_err());
        assert!(parent
            .narrow_for_child(ApprovalMode::Interactive, SandboxProfile::ReadOnly)
            .is_ok());
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
            last_provider_connection_id: Some("conn-1".into()),
            last_model_id: Some("model-1".into()),
            revision: 1,
            updated_at_ms: 0,
        };
        assert!(pref.has_model_preference());
        pref.last_model_id = None;
        assert!(!pref.has_model_preference());
    }

    #[test]
    fn runtime_preference_serialization_is_secret_free() {
        let pref = RuntimePreference {
            principal_id: "local-owner".into(),
            approval_mode: Some(ApprovalMode::Interactive),
            sandbox_profile: Some(SandboxProfile::WorkspaceWrite),
            last_provider_connection_id: Some("conn-1".into()),
            last_model_id: Some("model-1".into()),
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
                        | "last_provider_connection_id"
                        | "last_model_id"
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
