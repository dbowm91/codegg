//! Canonical principal, project-membership, role, and capability domain.
//!
//! Identity, authorization, and audit M001 owns the durable team domain that
//! later transport authentication (M002) and daemon authorization (M003) bind
//! to. This module is intentionally infrastructure: it stores who belongs to
//! which project and what each role may do, but it does not authenticate
//! network callers and does not enforce request-time authorization.
//!
//! ## Design notes
//!
//! - Paths never define identity. Every API takes typed [`PrincipalId`] and
//!   [`ProjectId`]; filesystem locators are rejected by the identity lexical
//!   contract before they can become owned values.
//! - [`PrincipalKind::LocalOwner`] is an explicit principal composition, not
//!   an authorization bypass. Personal-local daemons resolve the OS owner to
//!   the deterministic [`LOCAL_OWNER_PRINCIPAL_ID`] record via
//!   [`TeamStore::ensure_local_owner`]; authorization still receives an
//!   explicit principal.
//! - Roles expand to capabilities centrally through
//!   [`ProjectRole::capabilities`]. Callers never supply authority: unknown
//!   role or capability input fails closed.
//! - Membership rows are never physically deleted. Revocation sets
//!   [`MembershipState::Revoked`] with a revision bump, and every mutation
//!   requires the current revision so a stale writer cannot silently restore
//!   revoked authority.
//! - Records contain no credential secret, token, or key material. The
//!   `no_secret_material` test asserts the serialized shape stays free of
//!   secret-bearing field names.
//!
//! ## Transport contract (M002, implemented in `crate::transport_auth`)
//!
//! M002 authentication adapters resolve transport evidence to a canonical
//! [`PrincipalId`] plus authentication context and carry that immutable
//! principal through client/request context. Request DTOs remain locators and
//! MUST NOT name a principal, role, or capability. The daemon transport
//! constructs the request authority context; [`CapabilitySet`] values defined
//! here are the data that the M003 authorization service will evaluate. The
//! legacy global bearer remains a bootstrap/compatibility seam owned by the
//! transport layer (maps to `LocalOwner`, never to distinct identities); see
//! [`crate::transport_auth`] and [`adapt_principal_to_projection_id`].

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;

use crate::error::StorageError;
use crate::identity::{IdentityParseError, PrincipalId, ProjectId};

/// Deterministic principal identity for the personal-local owner.
///
/// The value satisfies the shared identity lexical contract so it can be
/// owned as a [`PrincipalId`] without a path or UUID round-trip. Bootstrap is
/// idempotent: [`TeamStore::ensure_local_owner`] inserts this row with
/// `INSERT OR IGNORE`, so restart and concurrent first-use converge on the
/// same record.
pub const LOCAL_OWNER_PRINCIPAL_ID: &str = "local-owner";

/// Maximum display-name length for a principal record.
pub const MAX_PRINCIPAL_DISPLAY_NAME_LENGTH: usize = 200;

/// Kind of authorization principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    ServiceAccount,
    Node,
    LocalOwner,
}

impl PrincipalKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::ServiceAccount => "service_account",
            Self::Node => "node",
            Self::LocalOwner => "local_owner",
        }
    }

    fn parse(value: &str) -> Result<Self, TeamError> {
        match value {
            "human" => Ok(Self::Human),
            "service_account" => Ok(Self::ServiceAccount),
            "node" => Ok(Self::Node),
            "local_owner" => Ok(Self::LocalOwner),
            _ => Err(TeamError::Invalid(format!(
                "unknown principal kind {value:?}"
            ))),
        }
    }

    /// Lenient parse for audit readers. Unknown stored kinds degrade to
    /// `None` so a page decode never fails on forward-compatible data.
    pub fn parse_for_audit(value: &str) -> Option<Self> {
        Self::parse(value).ok()
    }
}

/// Lifecycle state for a principal record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalStatus {
    Active,
    Disabled,
}

impl PrincipalStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    fn parse(value: &str) -> Result<Self, TeamError> {
        match value {
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            _ => Err(TeamError::Invalid(format!(
                "unknown principal status {value:?}"
            ))),
        }
    }
}

/// Durable principal record. Contains identity metadata only; never secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalRecord {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub display_name: String,
    pub status: PrincipalStatus,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Named bundle of project capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    Viewer,
    Contributor,
    Maintainer,
    Owner,
}

impl ProjectRole {
    /// All roles in ascending authority order.
    pub const ALL: [Self; 4] = [
        Self::Viewer,
        Self::Contributor,
        Self::Maintainer,
        Self::Owner,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Contributor => "contributor",
            Self::Maintainer => "maintainer",
            Self::Owner => "owner",
        }
    }

    /// Parse a role name, failing closed on unknown input.
    pub fn parse(value: &str) -> Result<Self, TeamError> {
        match value {
            "viewer" => Ok(Self::Viewer),
            "contributor" => Ok(Self::Contributor),
            "maintainer" => Ok(Self::Maintainer),
            "owner" => Ok(Self::Owner),
            _ => Err(TeamError::UnknownRole(value.to_owned())),
        }
    }

    /// Deterministic expansion of this role into semantic capabilities.
    ///
    /// Expansion is strictly monotonic: every role contains all capabilities
    /// of the roles below it. The mapping is the single central authority;
    /// no handler may hand-roll its own role interpretation.
    pub fn capabilities(self) -> CapabilitySet {
        match self {
            Self::Viewer => CapabilitySet::from_caps([
                Capability::ProjectRead,
                Capability::ProjectObserve,
                Capability::SessionRead,
                Capability::SessionObserve,
                Capability::FileRead,
                Capability::GitRead,
            ]),
            Self::Contributor => CapabilitySet::from_caps([
                Capability::ProjectRead,
                Capability::ProjectObserve,
                Capability::SessionRead,
                Capability::SessionObserve,
                Capability::FileRead,
                Capability::GitRead,
                Capability::ProjectChat,
                Capability::SessionCreate,
                Capability::AgentInvoke,
                Capability::FileModify,
                Capability::CommandExecute,
                Capability::JobSubmit,
                Capability::GitWrite,
                Capability::WorktreeCreate,
            ]),
            Self::Maintainer => CapabilitySet::from_caps([
                Capability::ProjectRead,
                Capability::ProjectObserve,
                Capability::SessionRead,
                Capability::SessionObserve,
                Capability::FileRead,
                Capability::GitRead,
                Capability::ProjectChat,
                Capability::SessionCreate,
                Capability::AgentInvoke,
                Capability::FileModify,
                Capability::CommandExecute,
                Capability::JobSubmit,
                Capability::GitWrite,
                Capability::WorktreeCreate,
                Capability::AgentDelegate,
                Capability::JobCancel,
                Capability::WorktreeRemove,
                Capability::ProjectConfigure,
                Capability::AuditRead,
            ]),
            Self::Owner => CapabilitySet::from_caps(Capability::ALL),
        }
    }

    /// `true` when this role's expansion contains `capability`.
    pub fn contains(self, capability: Capability) -> bool {
        self.capabilities().has(capability)
    }
}

/// Semantic, operation-oriented project capability.
///
/// Names describe operations (`session.create`, `git.write`), never handler
/// or frontend role names. Adding a variant is additive; unknown wire values
/// fail closed via [`Capability::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    #[serde(rename = "project.read")]
    ProjectRead,
    #[serde(rename = "project.observe")]
    ProjectObserve,
    #[serde(rename = "project.chat")]
    ProjectChat,
    #[serde(rename = "session.create")]
    SessionCreate,
    #[serde(rename = "session.read")]
    SessionRead,
    #[serde(rename = "session.observe")]
    SessionObserve,
    #[serde(rename = "agent.invoke")]
    AgentInvoke,
    #[serde(rename = "agent.delegate")]
    AgentDelegate,
    #[serde(rename = "file.read")]
    FileRead,
    #[serde(rename = "file.modify")]
    FileModify,
    #[serde(rename = "command.execute")]
    CommandExecute,
    #[serde(rename = "job.submit")]
    JobSubmit,
    #[serde(rename = "job.cancel")]
    JobCancel,
    #[serde(rename = "git.read")]
    GitRead,
    #[serde(rename = "git.write")]
    GitWrite,
    #[serde(rename = "worktree.create")]
    WorktreeCreate,
    #[serde(rename = "worktree.remove")]
    WorktreeRemove,
    #[serde(rename = "project.configure")]
    ProjectConfigure,
    #[serde(rename = "member.manage")]
    MemberManage,
    #[serde(rename = "audit.read")]
    AuditRead,
    #[serde(rename = "node.target")]
    NodeTarget,
}

impl Capability {
    /// Every capability in canonical order.
    pub const ALL: [Self; 21] = [
        Self::ProjectRead,
        Self::ProjectObserve,
        Self::ProjectChat,
        Self::SessionCreate,
        Self::SessionRead,
        Self::SessionObserve,
        Self::AgentInvoke,
        Self::AgentDelegate,
        Self::FileRead,
        Self::FileModify,
        Self::CommandExecute,
        Self::JobSubmit,
        Self::JobCancel,
        Self::GitRead,
        Self::GitWrite,
        Self::WorktreeCreate,
        Self::WorktreeRemove,
        Self::ProjectConfigure,
        Self::MemberManage,
        Self::AuditRead,
        Self::NodeTarget,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectRead => "project.read",
            Self::ProjectObserve => "project.observe",
            Self::ProjectChat => "project.chat",
            Self::SessionCreate => "session.create",
            Self::SessionRead => "session.read",
            Self::SessionObserve => "session.observe",
            Self::AgentInvoke => "agent.invoke",
            Self::AgentDelegate => "agent.delegate",
            Self::FileRead => "file.read",
            Self::FileModify => "file.modify",
            Self::CommandExecute => "command.execute",
            Self::JobSubmit => "job.submit",
            Self::JobCancel => "job.cancel",
            Self::GitRead => "git.read",
            Self::GitWrite => "git.write",
            Self::WorktreeCreate => "worktree.create",
            Self::WorktreeRemove => "worktree.remove",
            Self::ProjectConfigure => "project.configure",
            Self::MemberManage => "member.manage",
            Self::AuditRead => "audit.read",
            Self::NodeTarget => "node.target",
        }
    }

    /// Parse a wire/storage capability name, failing closed on unknown input.
    pub fn parse(value: &str) -> Result<Self, TeamError> {
        Ok(match value {
            "project.read" => Self::ProjectRead,
            "project.observe" => Self::ProjectObserve,
            "project.chat" => Self::ProjectChat,
            "session.create" => Self::SessionCreate,
            "session.read" => Self::SessionRead,
            "session.observe" => Self::SessionObserve,
            "agent.invoke" => Self::AgentInvoke,
            "agent.delegate" => Self::AgentDelegate,
            "file.read" => Self::FileRead,
            "file.modify" => Self::FileModify,
            "command.execute" => Self::CommandExecute,
            "job.submit" => Self::JobSubmit,
            "job.cancel" => Self::JobCancel,
            "git.read" => Self::GitRead,
            "git.write" => Self::GitWrite,
            "worktree.create" => Self::WorktreeCreate,
            "worktree.remove" => Self::WorktreeRemove,
            "project.configure" => Self::ProjectConfigure,
            "member.manage" => Self::MemberManage,
            "audit.read" => Self::AuditRead,
            "node.target" => Self::NodeTarget,
            _ => return Err(TeamError::UnknownCapability(value.to_owned())),
        })
    }
}

/// Deterministic set of semantic capabilities.
///
/// The set is data, never authority: it is produced by role expansion or by
/// parsing validated input, never trusted from a network caller. Iterating
/// yields capabilities in canonical order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    capabilities: Vec<Capability>,
}

impl CapabilitySet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a set from an iterator, deduplicating and sorting into canonical
    /// order so hashing, comparison, and snapshots are deterministic.
    pub fn from_caps<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Capability>,
    {
        let mut capabilities: Vec<Capability> = iter.into_iter().collect();
        capabilities.sort_by_key(|capability| *capability as u8);
        capabilities.dedup();
        Self { capabilities }
    }

    /// Parse a list of capability names, failing closed on the first unknown
    /// value. An empty list is a valid empty set.
    pub fn parse_list<I, S>(iter: I) -> Result<Self, TeamError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut capabilities = Vec::new();
        for value in iter {
            capabilities.push(Capability::parse(value.as_ref())?);
        }
        Ok(Self::from_caps(capabilities))
    }

    pub fn has(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.capabilities.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }

    /// Canonical storage/wire keys in sorted order.
    pub fn to_sorted_keys(&self) -> Vec<&'static str> {
        self.iter().map(Capability::as_str).collect()
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        Self::from_caps(iter)
    }
}

/// Lifecycle state for a project membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipState {
    Active,
    Suspended,
    Revoked,
}

impl MembershipState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Revoked => "revoked",
        }
    }

    fn parse(value: &str) -> Result<Self, TeamError> {
        match value {
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "revoked" => Ok(Self::Revoked),
            _ => Err(TeamError::Invalid(format!(
                "unknown membership state {value:?}"
            ))),
        }
    }
}

/// Durable project-scoped membership of one principal.
///
/// The primary key is `(project_id, principal_id)`: membership is always
/// project scoped and never global. `revision` implements optimistic
/// concurrency; every mutation requires the current revision and bumps it, so
/// a stale writer holding a pre-revocation snapshot cannot silently restore
/// authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipRecord {
    pub project_id: ProjectId,
    pub principal_id: PrincipalId,
    pub role: ProjectRole,
    pub state: MembershipState,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl MembershipRecord {
    /// Effective capabilities for this membership.
    ///
    /// Only [`MembershipState::Active`] memberships grant capabilities.
    /// Suspended and revoked memberships expand to the empty set without
    /// error so authorization fails closed.
    pub fn effective_capabilities(&self) -> CapabilitySet {
        if self.state != MembershipState::Active {
            return CapabilitySet::new();
        }
        self.role.capabilities()
    }

    /// `true` when this membership currently grants `capability`.
    pub fn has_capability(&self, capability: Capability) -> bool {
        self.effective_capabilities().has(capability)
    }
}

/// Domain errors for the principal/membership store.
#[derive(Debug, Error)]
pub enum TeamError {
    #[error("invalid team value: {0}")]
    Invalid(String),
    #[error("unknown project role: {0}")]
    UnknownRole(String),
    #[error("unknown capability: {0}")]
    UnknownCapability(String),
    #[error("invalid principal identity: {0}")]
    Identity(#[from] IdentityParseError),
    #[error("principal not found: {0}")]
    PrincipalNotFound(PrincipalId),
    #[error("membership not found for principal {principal} in project {project}")]
    MembershipNotFound {
        project: ProjectId,
        principal: PrincipalId,
    },
    #[error("principal {0} already exists")]
    PrincipalConflict(PrincipalId),
    #[error("membership for principal {principal} in project {project} already exists")]
    MembershipConflict {
        project: ProjectId,
        principal: PrincipalId,
    },
    #[error("stale membership revision for principal {principal} in project {project}: expected {expected}, current {current}")]
    RevisionConflict {
        project: ProjectId,
        principal: PrincipalId,
        expected: u64,
        current: u64,
    },
    #[error("stale principal revision for {id}: expected {expected}, current {current}")]
    PrincipalRevisionConflict {
        id: PrincipalId,
        expected: u64,
        current: u64,
    },
    #[error("team storage error: {0}")]
    Storage(#[from] StorageError),
}

/// Daemon-owned durable store for principals and project memberships.
///
/// The store supports concurrent read/update through optimistic revision
/// checks. All write methods are transactional and idempotent where retried:
/// bootstrap uses `INSERT OR IGNORE`, and mutations fail with
/// [`TeamError::RevisionConflict`] instead of overwriting newer state.
#[derive(Clone)]
pub struct TeamStore {
    pool: SqlitePool,
}

impl TeamStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Create a principal with a fresh [`PrincipalId`].
    pub async fn create_principal(
        &self,
        kind: PrincipalKind,
        display_name: &str,
    ) -> Result<PrincipalRecord, TeamError> {
        let display_name = validate_display_name(display_name)?;
        let id = PrincipalId::new();
        self.insert_principal(&id, kind, &display_name).await
    }

    /// Create a principal with an explicit identity.
    ///
    /// Used by tests and by deterministic bootstrap seams. Production callers
    /// other than [`Self::ensure_local_owner`] should prefer
    /// [`Self::create_principal`].
    pub async fn create_principal_with_id(
        &self,
        id: &PrincipalId,
        kind: PrincipalKind,
        display_name: &str,
    ) -> Result<PrincipalRecord, TeamError> {
        let display_name = validate_display_name(display_name)?;
        self.insert_principal(id, kind, &display_name).await
    }

    async fn insert_principal(
        &self,
        id: &PrincipalId,
        kind: PrincipalKind,
        display_name: &str,
    ) -> Result<PrincipalRecord, TeamError> {
        let now = now_millis();
        let result = sqlx::query(
            "INSERT INTO principal (id, kind, display_name, status, revision, \
             time_created, time_updated) VALUES (?, ?, ?, 'active', 1, ?, ?)",
        )
        .bind(id.as_str())
        .bind(kind.as_str())
        .bind(display_name)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await;
        match result {
            Ok(_) => self
                .get_principal(id)
                .await?
                .ok_or_else(|| TeamError::PrincipalNotFound(id.clone())),
            Err(error) if is_unique_violation(&error) => {
                Err(TeamError::PrincipalConflict(id.clone()))
            }
            Err(error) => Err(TeamError::Storage(StorageError::Database(
                error.to_string(),
            ))),
        }
    }

    /// Deterministic LocalOwner bootstrap.
    ///
    /// Inserts the well-known [`LOCAL_OWNER_PRINCIPAL_ID`] record when absent
    /// and returns the current row otherwise. Safe to call on every daemon
    /// start and from concurrent first-use paths: the insert is
    /// `OR IGNORE` and the subsequent read observes the winning row.
    pub async fn ensure_local_owner(&self) -> Result<PrincipalRecord, TeamError> {
        let now = now_millis();
        sqlx::query(
            "INSERT OR IGNORE INTO principal (id, kind, display_name, status, \
             revision, time_created, time_updated) VALUES (?, 'local_owner', \
             'Local Owner', 'active', 1, ?, ?)",
        )
        .bind(LOCAL_OWNER_PRINCIPAL_ID)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| TeamError::Storage(StorageError::Database(e.to_string())))?;
        let id = PrincipalId::parse(LOCAL_OWNER_PRINCIPAL_ID)?;
        self.get_principal(&id)
            .await?
            .ok_or(TeamError::PrincipalNotFound(id))
    }

    pub async fn get_principal(
        &self,
        id: &PrincipalId,
    ) -> Result<Option<PrincipalRecord>, TeamError> {
        let row = sqlx::query_as::<_, (String, String, String, String, i64, i64, i64)>(
            "SELECT id, kind, display_name, status, revision, time_created, \
             time_updated FROM principal WHERE id = ?",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TeamError::Storage(StorageError::Database(e.to_string())))?;
        row.map(principal_row).transpose()
    }

    pub async fn list_principals(&self) -> Result<Vec<PrincipalRecord>, TeamError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, i64, i64, i64)>(
            "SELECT id, kind, display_name, status, revision, time_created, \
             time_updated FROM principal ORDER BY time_created, id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| TeamError::Storage(StorageError::Database(e.to_string())))?;
        rows.into_iter().map(principal_row).collect()
    }

    /// Set a principal's lifecycle status, requiring the current revision.
    pub async fn set_principal_status(
        &self,
        id: &PrincipalId,
        expected_revision: u64,
        status: PrincipalStatus,
    ) -> Result<PrincipalRecord, TeamError> {
        let now = now_millis();
        let result = sqlx::query(
            "UPDATE principal SET status = ?, revision = revision + 1, \
             time_updated = ? WHERE id = ? AND revision = ?",
        )
        .bind(status.as_str())
        .bind(now)
        .bind(id.as_str())
        .bind(expected_revision as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| TeamError::Storage(StorageError::Database(e.to_string())))?;
        if result.rows_affected() == 1 {
            return self
                .get_principal(id)
                .await?
                .ok_or_else(|| TeamError::PrincipalNotFound(id.clone()));
        }
        match self.get_principal(id).await? {
            Some(current) => Err(TeamError::PrincipalRevisionConflict {
                id: id.clone(),
                expected: expected_revision,
                current: current.revision,
            }),
            None => Err(TeamError::PrincipalNotFound(id.clone())),
        }
    }

    /// Create a project-scoped membership.
    ///
    /// Fails with [`TeamError::MembershipConflict`] when the
    /// `(project, principal)` pair already exists -- including revoked rows --
    /// so revocation cannot be bypassed by re-creation. Re-grants must go
    /// through [`Self::update_membership`] with the current revision.
    pub async fn create_membership(
        &self,
        project_id: &ProjectId,
        principal_id: &PrincipalId,
        role: ProjectRole,
    ) -> Result<MembershipRecord, TeamError> {
        if self.get_principal(principal_id).await?.is_none() {
            return Err(TeamError::PrincipalNotFound(principal_id.clone()));
        }
        let now = now_millis();
        let result = sqlx::query(
            "INSERT INTO project_membership (project_id, principal_id, role, \
             state, revision, time_created, time_updated) VALUES (?, ?, ?, \
             'active', 1, ?, ?)",
        )
        .bind(project_id.as_str())
        .bind(principal_id.as_str())
        .bind(role.as_str())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await;
        match result {
            Ok(_) => self
                .get_membership(project_id, principal_id)
                .await?
                .ok_or_else(|| TeamError::MembershipNotFound {
                    project: project_id.clone(),
                    principal: principal_id.clone(),
                }),
            Err(error) if is_unique_violation(&error) => Err(TeamError::MembershipConflict {
                project: project_id.clone(),
                principal: principal_id.clone(),
            }),
            Err(error) => Err(TeamError::Storage(StorageError::Database(
                error.to_string(),
            ))),
        }
    }

    pub async fn get_membership(
        &self,
        project_id: &ProjectId,
        principal_id: &PrincipalId,
    ) -> Result<Option<MembershipRecord>, TeamError> {
        let row = sqlx::query_as::<_, (String, String, String, String, i64, i64, i64)>(
            "SELECT project_id, principal_id, role, state, revision, \
             time_created, time_updated FROM project_membership WHERE project_id = ? \
             AND principal_id = ?",
        )
        .bind(project_id.as_str())
        .bind(principal_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| TeamError::Storage(StorageError::Database(e.to_string())))?;
        row.map(membership_row).transpose()
    }

    /// List memberships for one project. Membership is always project scoped;
    /// there is no cross-project expansion in this query.
    pub async fn list_memberships_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<MembershipRecord>, TeamError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, i64, i64, i64)>(
            "SELECT project_id, principal_id, role, state, revision, \
             time_created, time_updated FROM project_membership WHERE project_id = ? \
             ORDER BY time_created, principal_id",
        )
        .bind(project_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| TeamError::Storage(StorageError::Database(e.to_string())))?;
        rows.into_iter().map(membership_row).collect()
    }

    pub async fn list_memberships_for_principal(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<Vec<MembershipRecord>, TeamError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, i64, i64, i64)>(
            "SELECT project_id, principal_id, role, state, revision, \
             time_created, time_updated FROM project_membership WHERE principal_id = ? \
             ORDER BY time_created, project_id",
        )
        .bind(principal_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| TeamError::Storage(StorageError::Database(e.to_string())))?;
        rows.into_iter().map(membership_row).collect()
    }

    /// Update a membership's role and/or state, requiring the current
    /// revision. A stale writer holding a pre-revocation snapshot receives
    /// [`TeamError::RevisionConflict`] and its write is dropped.
    pub async fn update_membership(
        &self,
        project_id: &ProjectId,
        principal_id: &PrincipalId,
        expected_revision: u64,
        role: Option<ProjectRole>,
        state: Option<MembershipState>,
    ) -> Result<MembershipRecord, TeamError> {
        let current = self
            .get_membership(project_id, principal_id)
            .await?
            .ok_or_else(|| TeamError::MembershipNotFound {
                project: project_id.clone(),
                principal: principal_id.clone(),
            })?;
        if current.revision != expected_revision {
            return Err(TeamError::RevisionConflict {
                project: project_id.clone(),
                principal: principal_id.clone(),
                expected: expected_revision,
                current: current.revision,
            });
        }
        let next_role = role.unwrap_or(current.role);
        let next_state = state.unwrap_or(current.state);
        if next_role == current.role && next_state == current.state {
            return Ok(current);
        }
        let now = now_millis();
        let result = sqlx::query(
            "UPDATE project_membership SET role = ?, state = ?, \
             revision = revision + 1, time_updated = ? WHERE project_id = ? AND \
             principal_id = ? AND revision = ?",
        )
        .bind(next_role.as_str())
        .bind(next_state.as_str())
        .bind(now)
        .bind(project_id.as_str())
        .bind(principal_id.as_str())
        .bind(expected_revision as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| TeamError::Storage(StorageError::Database(e.to_string())))?;
        if result.rows_affected() == 1 {
            return self
                .get_membership(project_id, principal_id)
                .await?
                .ok_or_else(|| TeamError::MembershipNotFound {
                    project: project_id.clone(),
                    principal: principal_id.clone(),
                });
        }
        let current = self
            .get_membership(project_id, principal_id)
            .await?
            .ok_or_else(|| TeamError::MembershipNotFound {
                project: project_id.clone(),
                principal: principal_id.clone(),
            })?;
        Err(TeamError::RevisionConflict {
            project: project_id.clone(),
            principal: principal_id.clone(),
            expected: expected_revision,
            current: current.revision,
        })
    }

    /// Revoke a membership without deleting its row. Revoked rows retain their
    /// revision history so stale re-grants fail closed.
    pub async fn revoke_membership(
        &self,
        project_id: &ProjectId,
        principal_id: &PrincipalId,
        expected_revision: u64,
    ) -> Result<MembershipRecord, TeamError> {
        self.update_membership(
            project_id,
            principal_id,
            expected_revision,
            None,
            Some(MembershipState::Revoked),
        )
        .await
    }

    /// Effective capabilities for one `(project, principal)` pair.
    ///
    /// Returns the empty set for unknown, non-active, or disabled-principal
    /// memberships so authorization fails closed. Unknown principals and
    /// missing memberships are indistinguishable to the caller.
    pub async fn effective_capabilities(
        &self,
        project_id: &ProjectId,
        principal_id: &PrincipalId,
    ) -> Result<CapabilitySet, TeamError> {
        let membership = self.get_membership(project_id, principal_id).await?;
        let Some(membership) = membership else {
            return Ok(CapabilitySet::new());
        };
        if membership.state != MembershipState::Active {
            return Ok(CapabilitySet::new());
        }
        if let Some(principal) = self.get_principal(principal_id).await? {
            if principal.status != PrincipalStatus::Active {
                return Ok(CapabilitySet::new());
            }
        }
        Ok(membership.effective_capabilities())
    }

    /// `true` when the `(project, principal)` pair currently grants
    /// `capability`.
    pub async fn has_capability(
        &self,
        project_id: &ProjectId,
        principal_id: &PrincipalId,
        capability: Capability,
    ) -> Result<bool, TeamError> {
        Ok(self
            .effective_capabilities(project_id, principal_id)
            .await?
            .has(capability))
    }
}

/// Executable role-to-capability matrix: every role with its expansion, in
/// ascending authority order. Used by tests, diagnostics, and architecture
/// documentation so the matrix cannot drift from the implementation.
pub fn role_capability_matrix() -> Vec<(ProjectRole, CapabilitySet)> {
    ProjectRole::ALL
        .iter()
        .map(|role| (*role, role.capabilities()))
        .collect()
}

/// Render the role/capability matrix as rows of `(role, capability, granted)`
/// for table-driven tests and operator diagnostics.
pub fn role_capability_rows() -> Vec<(ProjectRole, Capability, bool)> {
    let mut rows = Vec::new();
    for (role, set) in role_capability_matrix() {
        for capability in Capability::ALL {
            rows.push((role, capability, set.has(capability)));
        }
    }
    rows
}

/// Map a canonical principal to the projection-layer principal string.
///
/// [`crate::projection_replay`] still carries its own opaque
/// `ProjectionPrincipalId` wrapper with synthetic `"local-user"` and
/// `"internal-test"` values. Until M003 converges that seam, `LocalOwner`
/// maps to `"local-user"` and every other principal maps to its canonical
/// [`PrincipalId`] string. The mapping is one-way and diagnostic-only: it
/// MUST NOT be parsed back into authority.
pub fn adapt_principal_to_projection_id(principal: &PrincipalRecord) -> String {
    match principal.kind {
        PrincipalKind::LocalOwner => "local-user".to_owned(),
        _ => principal.id.as_str().to_owned(),
    }
}

/// `true` for the synthetic projection-layer values that are compatibility
/// projections rather than canonical [`PrincipalId`] records.
///
/// `"local-user"`, `"internal-test"`, and `"authenticated-remote"` are
/// transport/projection placeholders. They MUST NOT be accepted where a
/// canonical principal is required; M002 replaces them with bound canonical
/// principals.
pub fn is_compatibility_projection(value: &str) -> bool {
    matches!(
        value,
        "local-user" | "internal-test" | "authenticated-remote"
    )
}

fn validate_display_name(value: &str) -> Result<String, TeamError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || value.len() > MAX_PRINCIPAL_DISPLAY_NAME_LENGTH {
        return Err(TeamError::Invalid(
            "principal display name must be non-empty and bounded".to_owned(),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(TeamError::Invalid(
            "principal display name must not contain control characters".to_owned(),
        ));
    }
    Ok(trimmed.to_owned())
}

fn principal_row(
    row: (String, String, String, String, i64, i64, i64),
) -> Result<PrincipalRecord, TeamError> {
    let (id, kind, display_name, status, revision, created_at, updated_at) = row;
    let id = PrincipalId::parse(&id)?;
    let kind = PrincipalKind::parse(&kind)?;
    let status = PrincipalStatus::parse(&status)?;
    let revision = u64::try_from(revision)
        .map_err(|_| TeamError::Invalid("invalid principal revision".to_owned()))?;
    validate_display_name(&display_name)?;
    Ok(PrincipalRecord {
        id,
        kind,
        display_name,
        status,
        revision,
        created_at,
        updated_at,
    })
}

fn membership_row(
    row: (String, String, String, String, i64, i64, i64),
) -> Result<MembershipRecord, TeamError> {
    let (project_id, principal_id, role, state, revision, created_at, updated_at) = row;
    let project_id = ProjectId::parse(&project_id)?;
    let principal_id = PrincipalId::parse(&principal_id)?;
    let role = ProjectRole::parse(&role)?;
    let state = MembershipState::parse(&state)?;
    let revision = u64::try_from(revision)
        .map_err(|_| TeamError::Invalid("invalid membership revision".to_owned()))?;
    Ok(MembershipRecord {
        project_id,
        principal_id,
        role,
        state,
        revision,
        created_at,
        updated_at,
    })
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .to_string()
        .to_ascii_lowercase()
        .contains("unique constraint")
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> TeamStore {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate test store");
        TeamStore::new(pool)
    }

    #[test]
    fn principal_id_validation_rejects_paths_before_ownership() {
        assert!(PrincipalId::parse("").is_err());
        assert!(PrincipalId::parse("/tmp/project").is_err());
        assert!(PrincipalId::parse("a/b").is_err());
        assert!(PrincipalId::parse("a b").is_err());
        assert!(PrincipalId::parse("a.b").is_err());
        assert!(PrincipalId::parse("local-owner").is_ok());
        let fresh = PrincipalId::new();
        assert!(PrincipalId::parse(fresh.as_str()).is_ok());
    }

    #[test]
    fn membership_role_parse_fails_closed_on_unknown() {
        assert!(ProjectRole::parse("viewer").is_ok());
        assert!(ProjectRole::parse("Viewer").is_err());
        assert!(ProjectRole::parse("admin").is_err());
        assert!(ProjectRole::parse("").is_err());
        assert!(matches!(
            ProjectRole::parse("superuser"),
            Err(TeamError::UnknownRole(_))
        ));
    }

    #[test]
    fn membership_unknown_capability_fails_closed() {
        assert!(Capability::parse("session.create").is_ok());
        assert!(matches!(
            Capability::parse("session.admin"),
            Err(TeamError::UnknownCapability(_))
        ));
        assert!(Capability::parse("").is_err());
        assert!(Capability::parse("Viewer").is_err());
        assert!(Capability::parse("session.create ").is_err());
        assert!(CapabilitySet::parse_list(["session.read", "bogus"]).is_err());
        assert!(CapabilitySet::parse_list(Vec::<String>::new())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn membership_role_capability_matrix_is_deterministic_and_monotonic() {
        let matrix = role_capability_matrix();
        assert_eq!(matrix.len(), 4);
        let sizes: Vec<usize> = matrix.iter().map(|(_, set)| set.len()).collect();
        assert_eq!(sizes, vec![6, 14, 19, 21]);
        for window in matrix.windows(2) {
            let (_, lower) = &window[0];
            let (_, upper) = &window[1];
            for capability in lower.iter() {
                assert!(upper.has(capability), "expansion must be monotonic");
            }
        }
        // Spot-check the documented mapping.
        let viewer = ProjectRole::Viewer.capabilities();
        assert!(viewer.has(Capability::FileRead));
        assert!(!viewer.has(Capability::FileModify));
        assert!(!viewer.has(Capability::SessionCreate));
        let contributor = ProjectRole::Contributor.capabilities();
        assert!(contributor.has(Capability::FileModify));
        assert!(contributor.has(Capability::GitWrite));
        assert!(!contributor.has(Capability::ProjectConfigure));
        assert!(!contributor.has(Capability::MemberManage));
        let maintainer = ProjectRole::Maintainer.capabilities();
        assert!(maintainer.has(Capability::ProjectConfigure));
        assert!(maintainer.has(Capability::AuditRead));
        assert!(!maintainer.has(Capability::MemberManage));
        assert!(!maintainer.has(Capability::NodeTarget));
        let owner = ProjectRole::Owner.capabilities();
        assert_eq!(owner.len(), Capability::ALL.len());
        assert!(owner.has(Capability::MemberManage));
        assert!(owner.has(Capability::NodeTarget));
        // Full executable matrix has one row per role x capability.
        assert_eq!(role_capability_rows().len(), 4 * Capability::ALL.len());
        // Capability sets dedup and sort deterministically.
        let set = CapabilitySet::from_caps([
            Capability::GitWrite,
            Capability::FileRead,
            Capability::GitWrite,
        ]);
        assert_eq!(set.to_sorted_keys(), vec!["file.read", "git.write"]);
    }

    #[test]
    fn principal_compatibility_projection_classification() {
        assert!(is_compatibility_projection("local-user"));
        assert!(is_compatibility_projection("internal-test"));
        assert!(is_compatibility_projection("authenticated-remote"));
        assert!(!is_compatibility_projection("local-owner"));
        assert!(!is_compatibility_projection("some-principal"));
    }

    #[test]
    fn principal_records_contain_no_secret_material() {
        let record = PrincipalRecord {
            id: PrincipalId::parse("principal-fixture").unwrap(),
            kind: PrincipalKind::Human,
            display_name: "Ada".to_owned(),
            status: PrincipalStatus::Active,
            revision: 1,
            created_at: 1,
            updated_at: 1,
        };
        let json = serde_json::to_string(&record).expect("serialize principal");
        for forbidden in [
            "secret",
            "token",
            "password",
            "api_key",
            "bearer",
            "credential",
        ] {
            assert!(
                !json.to_ascii_lowercase().contains(forbidden),
                "principal JSON must not contain {forbidden:?}: {json}"
            );
        }
        let membership = MembershipRecord {
            project_id: ProjectId::parse("project-fixture").unwrap(),
            principal_id: PrincipalId::parse("principal-fixture").unwrap(),
            role: ProjectRole::Viewer,
            state: MembershipState::Active,
            revision: 1,
            created_at: 1,
            updated_at: 1,
        };
        let json = serde_json::to_string(&membership).expect("serialize membership");
        for forbidden in [
            "secret",
            "token",
            "password",
            "api_key",
            "bearer",
            "credential",
        ] {
            assert!(
                !json.to_ascii_lowercase().contains(forbidden),
                "membership JSON must not contain {forbidden:?}: {json}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn principal_lifecycle_create_get_disable() {
        let store = test_store().await;
        let created = store
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        assert_eq!(created.revision, 1);
        assert_eq!(created.status, PrincipalStatus::Active);
        let fetched = store.get_principal(&created.id).await.unwrap().unwrap();
        assert_eq!(fetched, created);
        let disabled = store
            .set_principal_status(&created.id, 1, PrincipalStatus::Disabled)
            .await
            .unwrap();
        assert_eq!(disabled.status, PrincipalStatus::Disabled);
        assert_eq!(disabled.revision, 2);
        let stale = store
            .set_principal_status(&created.id, 1, PrincipalStatus::Active)
            .await;
        assert!(matches!(
            stale,
            Err(TeamError::PrincipalRevisionConflict { .. })
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn principal_local_owner_bootstrap_is_deterministic() {
        let store = test_store().await;
        let first = store.ensure_local_owner().await.unwrap();
        assert_eq!(first.id.as_str(), LOCAL_OWNER_PRINCIPAL_ID);
        assert_eq!(first.kind, PrincipalKind::LocalOwner);
        assert_eq!(first.status, PrincipalStatus::Active);
        let second = store.ensure_local_owner().await.unwrap();
        assert_eq!(first, second);
        let adapted = adapt_principal_to_projection_id(&first);
        assert_eq!(adapted, "local-user");
        let human = store
            .create_principal(PrincipalKind::Human, "Grace")
            .await
            .unwrap();
        assert_eq!(adapt_principal_to_projection_id(&human), human.id.as_str());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn membership_create_update_remove_lifecycle() {
        let store = test_store().await;
        let principal = store
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let project = ProjectId::new();
        let created = store
            .create_membership(&project, &principal.id, ProjectRole::Viewer)
            .await
            .unwrap();
        assert_eq!(created.revision, 1);
        assert_eq!(created.state, MembershipState::Active);
        assert!(created.has_capability(Capability::FileRead));
        assert!(!created.has_capability(Capability::FileModify));

        let promoted = store
            .update_membership(
                &project,
                &principal.id,
                1,
                Some(ProjectRole::Contributor),
                None,
            )
            .await
            .unwrap();
        assert_eq!(promoted.role, ProjectRole::Contributor);
        assert_eq!(promoted.revision, 2);
        assert!(promoted.has_capability(Capability::FileModify));

        let suspended = store
            .update_membership(
                &project,
                &principal.id,
                2,
                None,
                Some(MembershipState::Suspended),
            )
            .await
            .unwrap();
        assert_eq!(suspended.state, MembershipState::Suspended);
        assert!(suspended.effective_capabilities().is_empty());

        let revoked = store
            .revoke_membership(&project, &principal.id, 3)
            .await
            .unwrap();
        assert_eq!(revoked.state, MembershipState::Revoked);
        assert!(revoked.effective_capabilities().is_empty());

        // Re-creation cannot bypass revocation; re-grant needs the revision.
        let conflict = store
            .create_membership(&project, &principal.id, ProjectRole::Viewer)
            .await;
        assert!(matches!(
            conflict,
            Err(TeamError::MembershipConflict { .. })
        ));
        let regranted = store
            .update_membership(
                &project,
                &principal.id,
                revoked.revision,
                Some(ProjectRole::Viewer),
                Some(MembershipState::Active),
            )
            .await
            .unwrap();
        assert_eq!(regranted.state, MembershipState::Active);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn membership_stale_revision_cannot_restore_revoked_authority() {
        let store = test_store().await;
        let principal = store
            .create_principal(PrincipalKind::Human, "Ada")
            .await
            .unwrap();
        let project = ProjectId::new();
        let created = store
            .create_membership(&project, &principal.id, ProjectRole::Maintainer)
            .await
            .unwrap();
        let revoked = store
            .revoke_membership(&project, &principal.id, created.revision)
            .await
            .unwrap();
        // Stale writer still holding revision 1 must fail, not resurrect.
        let stale = store
            .update_membership(
                &project,
                &principal.id,
                created.revision,
                Some(ProjectRole::Owner),
                Some(MembershipState::Active),
            )
            .await;
        match stale {
            Err(TeamError::RevisionConflict {
                expected, current, ..
            }) => {
                assert_eq!(expected, created.revision);
                assert_eq!(current, revoked.revision);
            }
            other => panic!("expected revision conflict, got {other:?}"),
        }
        let current = store
            .get_membership(&project, &principal.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.state, MembershipState::Revoked);
        assert!(!store
            .has_capability(&project, &principal.id, Capability::FileRead)
            .await
            .unwrap());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn membership_project_isolation_scopes_queries() {
        let store = test_store().await;
        let alice = store
            .create_principal(PrincipalKind::Human, "Alice")
            .await
            .unwrap();
        let bob = store
            .create_principal(PrincipalKind::Human, "Bob")
            .await
            .unwrap();
        let project_a = ProjectId::new();
        let project_b = ProjectId::new();
        store
            .create_membership(&project_a, &alice.id, ProjectRole::Owner)
            .await
            .unwrap();
        store
            .create_membership(&project_b, &bob.id, ProjectRole::Viewer)
            .await
            .unwrap();

        assert!(store
            .has_capability(&project_a, &alice.id, Capability::MemberManage)
            .await
            .unwrap());
        assert!(!store
            .has_capability(&project_b, &alice.id, Capability::FileRead)
            .await
            .unwrap());
        assert!(!store
            .has_capability(&project_a, &bob.id, Capability::FileRead)
            .await
            .unwrap());

        let members_a = store
            .list_memberships_for_project(&project_a)
            .await
            .unwrap();
        assert_eq!(members_a.len(), 1);
        assert_eq!(members_a[0].principal_id, alice.id);
        let members_b = store
            .list_memberships_for_project(&project_b)
            .await
            .unwrap();
        assert_eq!(members_b.len(), 1);
        assert_eq!(members_b[0].principal_id, bob.id);
        let alice_projects = store
            .list_memberships_for_principal(&alice.id)
            .await
            .unwrap();
        assert_eq!(alice_projects.len(), 1);
        assert_eq!(alice_projects[0].project_id, project_a);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn membership_restart_persistence_survives_reopen() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr as _;
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("team-restart.db");
        let project = ProjectId::new();
        let connect = |path: &std::path::Path| {
            let options = SqliteConnectOptions::from_str(&format!("sqlite:{}?", path.display()))
                .expect("options")
                .create_if_missing(true);
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
        };
        let (principal_id, membership_revision) = {
            let pool = connect(&db_path).await.expect("connect");
            sqlx::query("PRAGMA journal_mode=WAL;")
                .execute(&pool)
                .await
                .expect("wal");
            crate::session::schema::migrate(&pool)
                .await
                .expect("migrate");
            let store = TeamStore::new(pool.clone());
            let owner = store.ensure_local_owner().await.unwrap();
            assert_eq!(owner.id.as_str(), LOCAL_OWNER_PRINCIPAL_ID);
            let principal = store
                .create_principal(PrincipalKind::ServiceAccount, "ci-bot")
                .await
                .unwrap();
            let membership = store
                .create_membership(&project, &principal.id, ProjectRole::Contributor)
                .await
                .unwrap();
            pool.close().await;
            (principal.id, membership.revision)
        };
        let pool = connect(&db_path).await.expect("reconnect");
        crate::session::schema::migrate(&pool)
            .await
            .expect("remigrate is idempotent");
        let store = TeamStore::new(pool);
        let owner = store.ensure_local_owner().await.unwrap();
        assert_eq!(owner.id.as_str(), LOCAL_OWNER_PRINCIPAL_ID);
        let membership = store
            .get_membership(&project, &principal_id)
            .await
            .unwrap()
            .expect("membership survives restart");
        assert_eq!(membership.revision, membership_revision);
        assert!(membership.has_capability(Capability::GitWrite));
        pool_close(&store).await;
    }

    async fn pool_close(store: &TeamStore) {
        store.pool().close().await;
    }
}
