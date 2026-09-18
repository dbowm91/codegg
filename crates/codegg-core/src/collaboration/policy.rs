//! Team collaboration corrective M002: revisioned project/channel chat
//! access policy (ADR-0006).
//!
//! Role defaults stay the compatibility baseline (`Contributor` and above
//! allow chat, `Viewer` denies). A daemon-owned overlay adds project
//! principal overrides plus per-channel mode (`inherit_project` |
//! `restricted`) and channel principal overrides. Active membership is
//! always required first; a chat grant never implies execution authority.
//!
//! This module owns the pure precedence resolver plus the durable
//! revisioned store. It performs no transport authentication itself: the
//! daemon supplies typed `ProjectId`/`ChannelId`/`PrincipalId` plus the
//! caller's active role, and maps storage failures to fail-closed
//! denials.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;

use super::CollaborationError;
use crate::error::StorageError;
use crate::identity::{ChannelId, PrincipalId, ProjectId};
use crate::team::{ProjectRole, TeamStore};

/// One principal allow/deny override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPolicyDecision {
    Allow,
    Deny,
}

impl ChatPolicyDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "allow" => Some(Self::Allow),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }

    pub fn to_dto(self) -> codegg_protocol::core::ChatPolicyDecisionDto {
        match self {
            Self::Allow => codegg_protocol::core::ChatPolicyDecisionDto::Allow,
            Self::Deny => codegg_protocol::core::ChatPolicyDecisionDto::Deny,
        }
    }

    pub fn from_dto(value: codegg_protocol::core::ChatPolicyDecisionDto) -> Self {
        match value {
            codegg_protocol::core::ChatPolicyDecisionDto::Allow => Self::Allow,
            codegg_protocol::core::ChatPolicyDecisionDto::Deny => Self::Deny,
        }
    }
}

/// Per-channel default when no principal override matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannelMode {
    InheritProject,
    Restricted,
}

impl ChatChannelMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InheritProject => "inherit_project",
            Self::Restricted => "restricted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "inherit_project" => Some(Self::InheritProject),
            "restricted" => Some(Self::Restricted),
            _ => None,
        }
    }

    pub fn to_dto(self) -> codegg_protocol::core::ChatChannelModeDto {
        match self {
            Self::InheritProject => codegg_protocol::core::ChatChannelModeDto::InheritProject,
            Self::Restricted => codegg_protocol::core::ChatChannelModeDto::Restricted,
        }
    }

    pub fn from_dto(value: codegg_protocol::core::ChatChannelModeDto) -> Self {
        match value {
            codegg_protocol::core::ChatChannelModeDto::InheritProject => Self::InheritProject,
            codegg_protocol::core::ChatChannelModeDto::Restricted => Self::Restricted,
        }
    }
}

/// One principal override row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPolicyOverride {
    pub principal_id: PrincipalId,
    pub decision: ChatPolicyDecision,
}

impl ChatPolicyOverride {
    pub fn to_dto(&self) -> codegg_protocol::core::ChatPolicyOverrideDto {
        codegg_protocol::core::ChatPolicyOverrideDto {
            principal_id: self.principal_id.as_str().to_owned(),
            decision: self.decision.to_dto(),
        }
    }
}

/// Durable project-scoped chat policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatProjectPolicy {
    pub project_id: ProjectId,
    pub revision: u64,
    pub overrides: Vec<ChatPolicyOverride>,
}

impl ChatProjectPolicy {
    pub fn empty(project_id: ProjectId) -> Self {
        Self {
            project_id,
            revision: 0,
            overrides: Vec::new(),
        }
    }

    pub fn override_for(&self, principal: &PrincipalId) -> Option<ChatPolicyDecision> {
        self.overrides
            .iter()
            .find(|row| row.principal_id == *principal)
            .map(|row| row.decision)
    }

    pub fn to_dto(&self) -> codegg_protocol::core::ChatProjectPolicyDto {
        codegg_protocol::core::ChatProjectPolicyDto {
            project_id: self.project_id.as_str().to_owned(),
            revision: self.revision,
            overrides: self.overrides.iter().map(|row| row.to_dto()).collect(),
        }
    }
}

/// Durable channel-scoped chat policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatChannelPolicy {
    pub channel_id: ChannelId,
    pub project_id: ProjectId,
    pub mode: ChatChannelMode,
    pub revision: u64,
    pub overrides: Vec<ChatPolicyOverride>,
}

impl ChatChannelPolicy {
    pub fn override_for(&self, principal: &PrincipalId) -> Option<ChatPolicyDecision> {
        self.overrides
            .iter()
            .find(|row| row.principal_id == *principal)
            .map(|row| row.decision)
    }

    pub fn to_dto(&self) -> codegg_protocol::core::ChatChannelPolicyDto {
        codegg_protocol::core::ChatChannelPolicyDto {
            channel_id: self.channel_id.as_str().to_owned(),
            project_id: self.project_id.as_str().to_owned(),
            mode: self.mode.to_dto(),
            revision: self.revision,
            overrides: self.overrides.iter().map(|row| row.to_dto()).collect(),
        }
    }
}

/// Pure ADR-0006 precedence resolver.
///
/// Order: active membership required; role baseline (Contributor+
/// allow, Viewer deny); project principal override replaces the
/// baseline; `restricted` channel mode resets the default to deny;
/// channel principal override is final.
pub fn effective_chat_access(
    membership_active: bool,
    role: ProjectRole,
    project_override: Option<ChatPolicyDecision>,
    channel_mode: Option<ChatChannelMode>,
    channel_override: Option<ChatPolicyDecision>,
) -> bool {
    if !membership_active {
        return false;
    }
    let mut allowed = matches!(
        role,
        ProjectRole::Contributor | ProjectRole::Maintainer | ProjectRole::Owner
    );
    if let Some(decision) = project_override {
        allowed = decision == ChatPolicyDecision::Allow;
    }
    let mut channel_allowed = match channel_mode {
        Some(ChatChannelMode::Restricted) => false,
        Some(ChatChannelMode::InheritProject) | None => allowed,
    };
    if let Some(decision) = channel_override {
        channel_allowed = decision == ChatPolicyDecision::Allow;
    }
    channel_allowed
}

/// Typed failure for chat-policy administration.
#[derive(Debug, Error)]
pub enum ChatPolicyError {
    #[error("stale chat policy revision: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error("chat policy references an unknown channel: {0}")]
    UnknownChannel(String),
    #[error("chat policy channel does not belong to the project")]
    ChannelProjectMismatch,
    #[error("chat policy store unavailable: {0}")]
    Unavailable(String),
    #[error("chat policy storage error: {0}")]
    Storage(#[from] StorageError),
}

impl ChatPolicyError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::RevisionConflict { .. } => "chat_policy_conflict",
            Self::UnknownChannel(_) => "chat_channel_not_found",
            Self::ChannelProjectMismatch => "chat_channel_not_found",
            Self::Unavailable(_) => "chat_unavailable",
            Self::Storage(_) => "chat_storage_error",
        }
    }
}

impl From<sqlx::Error> for ChatPolicyError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(StorageError::Database(error.to_string()))
    }
}

/// Canonical `CREATE TABLE` statements for the M002 chat-policy domain.
///
/// Executed by the session-schema migration (v64); this helper keeps
/// tests and pool-less guards on the identical shape. All additive
/// `IF NOT EXISTS`, safe on existing databases: absence of rows
/// preserves Viewer-denied / Contributor+-allowed role defaults.
pub const CHAT_POLICY_SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS chat_project_policy (
        project_id TEXT PRIMARY KEY,
        revision INTEGER NOT NULL DEFAULT 1,
        updated_at INTEGER NOT NULL,
        updated_by TEXT NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS chat_project_chat_override (
        project_id TEXT NOT NULL,
        principal_id TEXT NOT NULL,
        decision TEXT NOT NULL CHECK (decision IN ('allow', 'deny')),
        updated_at INTEGER NOT NULL,
        updated_by TEXT NOT NULL,
        PRIMARY KEY (project_id, principal_id)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS chat_channel_policy (
        channel_id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        mode TEXT NOT NULL CHECK (mode IN ('inherit_project', 'restricted')) DEFAULT 'inherit_project',
        revision INTEGER NOT NULL DEFAULT 1,
        updated_at INTEGER NOT NULL,
        updated_by TEXT NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS chat_channel_chat_override (
        channel_id TEXT NOT NULL,
        principal_id TEXT NOT NULL,
        decision TEXT NOT NULL CHECK (decision IN ('allow', 'deny')),
        updated_at INTEGER NOT NULL,
        updated_by TEXT NOT NULL,
        PRIMARY KEY (channel_id, principal_id)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_chat_project_override_project ON chat_project_chat_override(project_id)",
    "CREATE INDEX IF NOT EXISTS idx_chat_channel_policy_project ON chat_channel_policy(project_id)",
    "CREATE INDEX IF NOT EXISTS idx_chat_channel_override_channel ON chat_channel_chat_override(channel_id)",
];

/// Ensure the chat-policy tables exist. Idempotent.
pub async fn ensure_chat_policy_tables(pool: &SqlitePool) -> Result<(), ChatPolicyError> {
    for statement in CHAT_POLICY_SCHEMA_STATEMENTS {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| ChatPolicyError::Storage(StorageError::Migration(e.to_string())))?;
    }
    Ok(())
}

/// Load one project policy. Absent rows decode as revision 0 with no
/// overrides, preserving role-default behavior.
pub async fn get_project_policy(
    pool: &SqlitePool,
    project: &ProjectId,
) -> Result<ChatProjectPolicy, ChatPolicyError> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT revision FROM chat_project_policy WHERE project_id = ?")
            .bind(project.as_str())
            .fetch_optional(pool)
            .await?;
    let revision = match row {
        Some((rev,)) => u64::try_from(rev).unwrap_or(0),
        None => 0,
    };
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT principal_id, decision FROM chat_project_chat_override WHERE project_id = ?",
    )
    .bind(project.as_str())
    .fetch_all(pool)
    .await?;
    let mut overrides = Vec::with_capacity(rows.len());
    for (principal_raw, decision_raw) in rows {
        let Ok(principal) = PrincipalId::parse(&principal_raw) else {
            continue;
        };
        let Some(decision) = ChatPolicyDecision::parse(&decision_raw) else {
            continue;
        };
        overrides.push(ChatPolicyOverride {
            principal_id: principal,
            decision,
        });
    }
    overrides.sort_by(|a, b| a.principal_id.as_str().cmp(b.principal_id.as_str()));
    Ok(ChatProjectPolicy {
        project_id: project.clone(),
        revision,
        overrides,
    })
}

/// Load one channel policy. Returns `None` when no policy row exists
/// (callers treat absence as `inherit_project` with no overrides).
/// Returns `None` for foreign channels: the stored `project_id` must
/// match `project`, otherwise the lookup fails closed.
pub async fn get_channel_policy(
    pool: &SqlitePool,
    project: &ProjectId,
    channel: &ChannelId,
) -> Result<Option<ChatChannelPolicy>, ChatPolicyError> {
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT project_id, mode, revision FROM chat_channel_policy WHERE channel_id = ?",
    )
    .bind(channel.as_str())
    .fetch_optional(pool)
    .await?;
    let Some((stored_project, mode_raw, rev)) = row else {
        return Ok(None);
    };
    if stored_project != project.as_str() {
        return Ok(None);
    }
    let mode = ChatChannelMode::parse(&mode_raw).unwrap_or(ChatChannelMode::InheritProject);
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT principal_id, decision FROM chat_channel_chat_override WHERE channel_id = ?",
    )
    .bind(channel.as_str())
    .fetch_all(pool)
    .await?;
    let mut overrides = Vec::with_capacity(rows.len());
    for (principal_raw, decision_raw) in rows {
        let Ok(principal) = PrincipalId::parse(&principal_raw) else {
            continue;
        };
        let Some(decision) = ChatPolicyDecision::parse(&decision_raw) else {
            continue;
        };
        overrides.push(ChatPolicyOverride {
            principal_id: principal,
            decision,
        });
    }
    overrides.sort_by(|a, b| a.principal_id.as_str().cmp(b.principal_id.as_str()));
    Ok(Some(ChatChannelPolicy {
        channel_id: channel.clone(),
        project_id: project.clone(),
        mode,
        revision: u64::try_from(rev).unwrap_or(0),
        overrides,
    }))
}

/// List every channel policy for one project (bounded by the caller's
/// channel budget; rows are structural only).
pub async fn list_channel_policies(
    pool: &SqlitePool,
    project: &ProjectId,
) -> Result<Vec<ChatChannelPolicy>, ChatPolicyError> {
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT channel_id, mode, revision FROM chat_channel_policy WHERE project_id = ? \
         ORDER BY channel_id ASC",
    )
    .bind(project.as_str())
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for (channel_raw, mode_raw, rev) in rows {
        let Ok(channel) = ChannelId::parse(&channel_raw) else {
            continue;
        };
        let mode = ChatChannelMode::parse(&mode_raw).unwrap_or(ChatChannelMode::InheritProject);
        let orows: Vec<(String, String)> = sqlx::query_as(
            "SELECT principal_id, decision FROM chat_channel_chat_override WHERE channel_id = ?",
        )
        .bind(channel.as_str())
        .fetch_all(pool)
        .await?;
        let mut overrides = Vec::with_capacity(orows.len());
        for (principal_raw, decision_raw) in orows {
            let Ok(principal) = PrincipalId::parse(&principal_raw) else {
                continue;
            };
            let Some(decision) = ChatPolicyDecision::parse(&decision_raw) else {
                continue;
            };
            overrides.push(ChatPolicyOverride {
                principal_id: principal,
                decision,
            });
        }
        overrides.sort_by(|a, b| a.principal_id.as_str().cmp(b.principal_id.as_str()));
        out.push(ChatChannelPolicy {
            channel_id: channel,
            project_id: project.clone(),
            mode,
            revision: u64::try_from(rev).unwrap_or(0),
            overrides,
        });
    }
    Ok(out)
}

fn check_revision(current: u64, expected: Option<u64>) -> Result<(), ChatPolicyError> {
    if let Some(expected) = expected {
        if expected != current {
            return Err(ChatPolicyError::RevisionConflict { expected, current });
        }
    }
    Ok(())
}

/// Set or clear one project principal override.
///
/// `decision: None` clears the row. Duplicate identical sets and
/// clears of absent rows converge without a revision bump. Otherwise
/// the project revision bumps by one. Stale `expected_revision` fails
/// with `RevisionConflict` and changes nothing.
pub async fn set_project_override(
    pool: &SqlitePool,
    project: &ProjectId,
    principal: &PrincipalId,
    decision: Option<ChatPolicyDecision>,
    expected_revision: Option<u64>,
    updated_by: &PrincipalId,
    now_ms: i64,
) -> Result<ChatProjectPolicy, ChatPolicyError> {
    let current = get_project_policy(pool, project).await?;
    check_revision(current.revision, expected_revision)?;
    let existing = current.override_for(principal);
    if existing == decision {
        return Ok(current);
    }
    let mut tx = pool.begin().await?;
    let next_revision = current.revision + 1;
    sqlx::query(
        "INSERT INTO chat_project_policy (project_id, revision, updated_at, updated_by) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(project_id) DO UPDATE SET revision = excluded.revision, \
         updated_at = excluded.updated_at, updated_by = excluded.updated_by",
    )
    .bind(project.as_str())
    .bind(next_revision as i64)
    .bind(now_ms)
    .bind(updated_by.as_str())
    .execute(&mut *tx)
    .await?;
    match decision {
        Some(decision) => {
            sqlx::query(
                "INSERT INTO chat_project_chat_override \
                 (project_id, principal_id, decision, updated_at, updated_by) \
                 VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(project_id, principal_id) DO UPDATE SET decision = excluded.decision, \
                 updated_at = excluded.updated_at, updated_by = excluded.updated_by",
            )
            .bind(project.as_str())
            .bind(principal.as_str())
            .bind(decision.as_str())
            .bind(now_ms)
            .bind(updated_by.as_str())
            .execute(&mut *tx)
            .await?;
        }
        None => {
            sqlx::query(
                "DELETE FROM chat_project_chat_override WHERE project_id = ? AND principal_id = ?",
            )
            .bind(project.as_str())
            .bind(principal.as_str())
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    get_project_policy(pool, project).await
}

/// Set channel mode and/or one channel principal override.
///
/// At least one of `mode` / `(principal, decision-or-clear)` must be
/// present. Duplicate identical writes converge without a revision
/// bump. The channel must exist in `chat_channel` and belong to
/// `project`; foreign or unknown channels fail closed with
/// `UnknownChannel`. Stale revisions fail with `RevisionConflict`.
#[allow(clippy::too_many_arguments)]
pub async fn set_channel_policy(
    pool: &SqlitePool,
    project: &ProjectId,
    channel: &ChannelId,
    mode: Option<ChatChannelMode>,
    principal: Option<&PrincipalId>,
    decision: Option<ChatPolicyDecision>,
    expected_revision: Option<u64>,
    updated_by: &PrincipalId,
    now_ms: i64,
) -> Result<ChatChannelPolicy, ChatPolicyError> {
    // Foreign/stale channel ids fail closed before any policy read.
    let owner: Option<(String,)> =
        sqlx::query_as("SELECT project_id FROM chat_channel WHERE id = ?")
            .bind(channel.as_str())
            .fetch_optional(pool)
            .await?;
    let Some((stored_project,)) = owner else {
        return Err(ChatPolicyError::UnknownChannel(channel.as_str().to_owned()));
    };
    if stored_project != project.as_str() {
        return Err(ChatPolicyError::UnknownChannel(channel.as_str().to_owned()));
    }
    let current = get_channel_policy(pool, project, channel).await?;
    let current_revision = current.as_ref().map(|p| p.revision).unwrap_or(0);
    check_revision(current_revision, expected_revision)?;
    let current_mode = current
        .as_ref()
        .map(|p| p.mode)
        .unwrap_or(ChatChannelMode::InheritProject);
    let existing_override = current
        .as_ref()
        .and_then(|p| principal.map(|id| p.override_for(id)).unwrap_or(None));
    let mode_unchanged = mode.map(|m| m == current_mode).unwrap_or(true);
    let override_unchanged = match principal {
        None => true,
        Some(_) => existing_override == decision,
    };
    if mode_unchanged && override_unchanged {
        if let Some(policy) = current {
            return Ok(policy);
        }
        return Ok(ChatChannelPolicy {
            channel_id: channel.clone(),
            project_id: project.clone(),
            mode: ChatChannelMode::InheritProject,
            revision: 0,
            overrides: Vec::new(),
        });
    }
    let next_revision = current_revision + 1;
    let next_mode = mode.unwrap_or(current_mode);
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO chat_channel_policy \
         (channel_id, project_id, mode, revision, updated_at, updated_by) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(channel_id) DO UPDATE SET mode = excluded.mode, \
         revision = excluded.revision, updated_at = excluded.updated_at, \
         updated_by = excluded.updated_by",
    )
    .bind(channel.as_str())
    .bind(project.as_str())
    .bind(next_mode.as_str())
    .bind(next_revision as i64)
    .bind(now_ms)
    .bind(updated_by.as_str())
    .execute(&mut *tx)
    .await?;
    if let Some(target) = principal {
        match decision {
            Some(decision) => {
                sqlx::query(
                    "INSERT INTO chat_channel_chat_override \
                     (channel_id, principal_id, decision, updated_at, updated_by) \
                     VALUES (?, ?, ?, ?, ?) \
                     ON CONFLICT(channel_id, principal_id) DO UPDATE SET \
                     decision = excluded.decision, updated_at = excluded.updated_at, \
                     updated_by = excluded.updated_by",
                )
                .bind(channel.as_str())
                .bind(target.as_str())
                .bind(decision.as_str())
                .bind(now_ms)
                .bind(updated_by.as_str())
                .execute(&mut *tx)
                .await?;
            }
            None => {
                sqlx::query(
                    "DELETE FROM chat_channel_chat_override \
                     WHERE channel_id = ? AND principal_id = ?",
                )
                .bind(channel.as_str())
                .bind(target.as_str())
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    tx.commit().await?;
    let policy = get_channel_policy(pool, project, channel)
        .await?
        .unwrap_or(ChatChannelPolicy {
            channel_id: channel.clone(),
            project_id: project.clone(),
            mode: next_mode,
            revision: next_revision,
            overrides: Vec::new(),
        });
    Ok(policy)
}

/// Request-time effective chat access for one principal.
///
/// Active membership (plus an active principal record) is required
/// before any policy row is consulted; revoked, suspended, absent, or
/// disabled principals are denied. Unknown channels and cross-project
/// channels fail closed. When no policy rows exist the role baseline
/// decides (Viewer denied, Contributor+ allowed). Storage failures
/// fail closed: the caller must deny rather than fall back to role
/// defaults.
pub async fn effective_chat_access_for(
    pool: &SqlitePool,
    team: &TeamStore,
    project: &ProjectId,
    channel: Option<&ChannelId>,
    principal: &PrincipalId,
) -> Result<bool, ChatPolicyError> {
    let membership = team.get_membership(project, principal).await.map_err(|e| {
        ChatPolicyError::Storage(crate::error::StorageError::Database(e.to_string()))
    })?;
    let Some(membership) = membership else {
        return Ok(false);
    };
    if membership.state != crate::team::MembershipState::Active {
        return Ok(false);
    }
    let record = team.get_principal(principal).await.map_err(|e| {
        ChatPolicyError::Storage(crate::error::StorageError::Database(e.to_string()))
    })?;
    let Some(record) = record else {
        return Ok(false);
    };
    if record.status != crate::team::PrincipalStatus::Active {
        return Ok(false);
    }
    let project_policy = get_project_policy(pool, project).await?;
    let project_override = project_policy.override_for(principal);
    let (channel_mode, channel_override) = match channel {
        None => (None, None),
        Some(channel_id) => {
            // Unknown or cross-project channels fail closed.
            let owner: Option<(String,)> =
                sqlx::query_as("SELECT project_id FROM chat_channel WHERE id = ?")
                    .bind(channel_id.as_str())
                    .fetch_optional(pool)
                    .await?;
            let Some((stored_project,)) = owner else {
                return Ok(false);
            };
            if stored_project != project.as_str() {
                return Ok(false);
            }
            match get_channel_policy(pool, project, channel_id).await? {
                None => (None, None),
                Some(policy) => (Some(policy.mode), policy.override_for(principal)),
            }
        }
    };
    Ok(effective_chat_access(
        true,
        membership.role,
        project_override,
        channel_mode,
        channel_override,
    ))
}

/// Structural audit metadata for a chat-policy change. Carries ids and
/// revisions only — never override content beyond the decision word,
/// never message content or secrets.
pub fn audit_metadata_for_policy_change(
    project: &ProjectId,
    channel: Option<&ChannelId>,
    revision: u64,
    actor: &PrincipalId,
) -> std::collections::BTreeMap<String, String> {
    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("chat.project".to_owned(), project.as_str().to_owned());
    if let Some(channel) = channel {
        metadata.insert("chat.channel".to_owned(), channel.as_str().to_owned());
    }
    metadata.insert("chat.policy_revision".to_owned(), revision.to_string());
    metadata.insert("chat.actor".to_owned(), actor.as_str().to_owned());
    metadata
}

impl From<ChatPolicyError> for CollaborationError {
    fn from(error: ChatPolicyError) -> Self {
        match error {
            ChatPolicyError::RevisionConflict {
                expected, current, ..
            } => CollaborationError::RevisionConflict {
                message: "chat-policy".to_owned(),
                expected,
                current,
            },
            ChatPolicyError::UnknownChannel(id) => CollaborationError::ChannelNotFound(id),
            ChatPolicyError::ChannelProjectMismatch => {
                CollaborationError::ProjectMismatch("chat policy channel project".to_owned())
            }
            ChatPolicyError::Unavailable(message) => CollaborationError::Unavailable(message),
            ChatPolicyError::Storage(inner) => CollaborationError::Storage(inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::ProjectRole;

    #[test]
    fn resolver_truth_table_matches_adr0006() {
        use ChatChannelMode as Mode;
        use ChatPolicyDecision as D;
        use ProjectRole as R;
        // Role baselines with no overrides.
        assert!(!effective_chat_access(true, R::Viewer, None, None, None));
        assert!(effective_chat_access(
            true,
            R::Contributor,
            None,
            None,
            None
        ));
        assert!(effective_chat_access(true, R::Maintainer, None, None, None));
        assert!(effective_chat_access(true, R::Owner, None, None, None));
        // Inactive membership denies before policy lookup.
        assert!(!effective_chat_access(
            false,
            R::Owner,
            Some(D::Allow),
            None,
            None
        ));
        assert!(!effective_chat_access(
            false,
            R::Contributor,
            None,
            None,
            Some(D::Allow)
        ));
        // Project overrides replace the baseline.
        assert!(effective_chat_access(
            true,
            R::Viewer,
            Some(D::Allow),
            None,
            None
        ));
        assert!(!effective_chat_access(
            true,
            R::Contributor,
            Some(D::Deny),
            None,
            None
        ));
        // Channel deny over project allow.
        assert!(!effective_chat_access(
            true,
            R::Viewer,
            Some(D::Allow),
            None,
            Some(D::Deny)
        ));
        assert!(!effective_chat_access(
            true,
            R::Contributor,
            None,
            None,
            Some(D::Deny)
        ));
        // Viewer one-channel allow.
        assert!(effective_chat_access(
            true,
            R::Viewer,
            None,
            None,
            Some(D::Allow)
        ));
        // Restricted channel explicit allowlist.
        assert!(!effective_chat_access(
            true,
            R::Contributor,
            None,
            Some(Mode::Restricted),
            None
        ));
        assert!(effective_chat_access(
            true,
            R::Contributor,
            None,
            Some(Mode::Restricted),
            Some(D::Allow)
        ));
        assert!(!effective_chat_access(
            true,
            R::Viewer,
            None,
            Some(Mode::Restricted),
            None
        ));
        assert!(effective_chat_access(
            true,
            R::Viewer,
            None,
            Some(Mode::Restricted),
            Some(D::Allow)
        ));
        // Inherit keeps the project decision.
        assert!(effective_chat_access(
            true,
            R::Viewer,
            Some(D::Allow),
            Some(Mode::InheritProject),
            None
        ));
        assert!(!effective_chat_access(
            true,
            R::Contributor,
            Some(D::Deny),
            Some(Mode::InheritProject),
            None
        ));
        // Channel allow over project deny.
        assert!(effective_chat_access(
            true,
            R::Contributor,
            Some(D::Deny),
            None,
            Some(D::Allow)
        ));
    }

    #[test]
    fn decision_and_mode_parse_fails_closed() {
        assert_eq!(
            ChatPolicyDecision::parse("allow"),
            Some(ChatPolicyDecision::Allow)
        );
        assert_eq!(
            ChatPolicyDecision::parse("deny"),
            Some(ChatPolicyDecision::Deny)
        );
        assert_eq!(ChatPolicyDecision::parse("ALLOW"), None);
        assert_eq!(ChatPolicyDecision::parse(""), None);
        assert_eq!(
            ChatChannelMode::parse("inherit_project"),
            Some(ChatChannelMode::InheritProject)
        );
        assert_eq!(
            ChatChannelMode::parse("restricted"),
            Some(ChatChannelMode::Restricted)
        );
        assert_eq!(ChatChannelMode::parse("open"), None);
    }
}
