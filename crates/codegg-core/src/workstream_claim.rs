//! Durable intent claims for Agent Workstream Blackboard M003.
//!
//! Claims coordinate declared responsibilities only. They are never consulted
//! by filesystem, Git, sandbox, or scheduler authorization.

use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::identity::{
    AgentRunGroupId, AgentRunId, AgentTaskId, PrincipalId, ProjectId, WorkspaceId,
    WorkstreamClaimId,
};

pub const MAX_CLAIM_TARGET_BYTES: usize = 256;
pub const MAX_CLAIM_IDEMPOTENCY_BYTES: usize = 128;
pub const MAX_CLAIM_DEPENDENCIES: usize = 32;
pub const MAX_CLAIMS_PER_GROUP: usize = 128;
pub const MAX_CLAIM_EVENTS_PER_GROUP: usize = 4096;
pub const MAX_CLAIM_LEASE_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum ClaimActor {
    Principal(PrincipalId),
    Run(AgentRunId),
}

impl ClaimActor {
    fn kind(&self) -> &'static str {
        match self {
            Self::Principal(_) => "principal",
            Self::Run(_) => "run",
        }
    }

    fn id(&self) -> &str {
        match self {
            Self::Principal(id) => id.as_str(),
            Self::Run(id) => id.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "target")]
pub enum ClaimTarget {
    Task(AgentTaskId),
    Path(String),
    Interface(String),
}

impl ClaimTarget {
    fn kind(&self) -> &'static str {
        match self {
            Self::Task(_) => "task",
            Self::Path(_) => "path",
            Self::Interface(_) => "interface",
        }
    }

    fn normalized(&self) -> Result<String, WorkstreamClaimError> {
        match self {
            Self::Task(id) => Ok(id.to_string()),
            Self::Path(value) | Self::Interface(value) => normalize_relative_intent(value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimState {
    Offered,
    Claimed,
    Released,
    Blocked,
    Completed,
}

impl ClaimState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Offered => "offered",
            Self::Claimed => "claimed",
            Self::Released => "released",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
        }
    }

    fn parse(value: &str) -> Result<Self, WorkstreamClaimError> {
        match value {
            "offered" => Ok(Self::Offered),
            "claimed" => Ok(Self::Claimed),
            "released" => Ok(Self::Released),
            "blocked" => Ok(Self::Blocked),
            "completed" => Ok(Self::Completed),
            _ => Err(WorkstreamClaimError::CorruptState),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkstreamClaim {
    pub claim_id: WorkstreamClaimId,
    pub project_id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub group_id: AgentRunGroupId,
    pub target: ClaimTarget,
    pub state: ClaimState,
    pub owner: ClaimActor,
    pub version: u64,
    pub lease_generation: u64,
    pub lease_expires_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl WorkstreamClaim {
    pub fn to_projection(
        &self,
    ) -> Result<codegg_protocol::projection::WorkstreamClaimProjection, WorkstreamClaimError> {
        Ok(codegg_protocol::projection::WorkstreamClaimProjection {
            claim_id: self.claim_id.to_string(),
            project_id: self.project_id.to_string(),
            workspace_id: self.workspace_id.to_string(),
            group_id: self.group_id.to_string(),
            kind: self.target.kind().to_owned(),
            target: self.target.normalized()?,
            state: self.state.as_str().to_owned(),
            owner_kind: self.owner.kind().to_owned(),
            owner_id: self.owner.id().to_owned(),
            version: self.version,
            lease_generation: self.lease_generation,
            lease_expires_at: self.lease_expires_at,
            updated_at: self.updated_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum ClaimPrerequisite {
    Claim(WorkstreamClaimId),
    Run(AgentRunId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkstreamClaimDependency {
    pub claim_id: WorkstreamClaimId,
    pub prerequisite: ClaimPrerequisite,
    pub created_at: i64,
}

impl WorkstreamClaimDependency {
    pub fn to_projection(&self) -> codegg_protocol::projection::WorkstreamDependencyProjection {
        let (kind, id) = match &self.prerequisite {
            ClaimPrerequisite::Claim(id) => ("claim", id.as_str()),
            ClaimPrerequisite::Run(id) => ("run", id.as_str()),
        };
        codegg_protocol::projection::WorkstreamDependencyProjection {
            claim_id: self.claim_id.to_string(),
            prerequisite_kind: kind.to_owned(),
            prerequisite_id: id.to_owned(),
            created_at: self.created_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewWorkstreamClaim {
    pub project_id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub group_id: AgentRunGroupId,
    pub actor: ClaimActor,
    pub target: ClaimTarget,
    pub idempotency_key: String,
    pub lease_expires_at: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub enum ClaimTransition {
    Renew,
    Release,
    Block,
    Unblock,
    Complete,
    Handoff,
    AcceptOffer,
}

#[derive(Debug, Clone)]
pub struct ClaimCas {
    pub claim_id: WorkstreamClaimId,
    pub actor: ClaimActor,
    pub expected_version: u64,
    pub expected_lease_generation: u64,
    pub transition: ClaimTransition,
    pub new_owner: Option<ClaimActor>,
    pub lease_expires_at: Option<i64>,
}

#[derive(Debug, Error)]
pub enum WorkstreamClaimError {
    #[error("workstream claim storage error: {0}")]
    Storage(String),
    #[error("workstream group or claim not found")]
    NotFound,
    #[error("claim actor is not an active group owner or member")]
    Unauthorized,
    #[error("claim target is invalid or exceeds its bound")]
    InvalidTarget,
    #[error("claim payload conflicts with an existing idempotency key")]
    IdempotencyConflict,
    #[error("claim is owned by another actor")]
    OwnedByAnother,
    #[error("claim revision or lease generation is stale")]
    StaleRevision,
    #[error("claim state transition is not allowed")]
    InvalidTransition,
    #[error("claim capacity is exhausted")]
    Capacity,
    #[error("claim target overlaps another active intent")]
    Overlap,
    #[error("dependency would create a cycle or duplicate edge")]
    DependencyCycle,
    #[error("dependency is cross-project, outside the group, or missing")]
    InvalidDependency,
    #[error("durable claim state is corrupt")]
    CorruptState,
}

#[derive(Clone)]
pub struct WorkstreamClaimStore {
    pool: SqlitePool,
}

impl WorkstreamClaimStore {
    pub fn new(pool: SqlitePool) -> Arc<Self> {
        Arc::new(Self { pool })
    }

    /// Authorized bounded inspection for an active group member or owner.
    pub async fn list_group(
        &self,
        group_id: &AgentRunGroupId,
        project_id: &ProjectId,
        workspace_id: &WorkspaceId,
        actor: &ClaimActor,
    ) -> Result<Vec<WorkstreamClaim>, WorkstreamClaimError> {
        let mut conn = self.pool.acquire().await.map_err(storage_error)?;
        validate_group_actor(&mut conn, group_id, project_id, workspace_id, actor).await?;
        let rows = sqlx::query(
            "SELECT claim_id, project_id, workspace_id, group_id, claim_kind, target, state, \
             owner_kind, owner_id, version, lease_generation, lease_expires_at, created_at, updated_at \
             FROM workstream_claim WHERE group_id=? AND project_id=? AND workspace_id=? \
             ORDER BY created_at, claim_id LIMIT ?",
        )
        .bind(group_id.as_str())
        .bind(project_id.as_str())
        .bind(workspace_id.as_str())
        .bind(MAX_CLAIMS_PER_GROUP as i64 + 1)
        .fetch_all(&mut *conn)
        .await
        .map_err(storage_error)?;
        if rows.len() > MAX_CLAIMS_PER_GROUP {
            return Err(WorkstreamClaimError::CorruptState);
        }
        rows.iter().map(row_to_claim).collect()
    }

    /// Build the M002 workstream projection from an already-authorized source
    /// snapshot and claims read through the same group actor check.
    pub async fn project_group(
        &self,
        snapshot: &codegg_protocol::projection::SessionProjectionSnapshot,
        group_id: &AgentRunGroupId,
        actor: &ClaimActor,
    ) -> Result<codegg_protocol::projection::AgentWorkstreamSnapshot, WorkstreamClaimError> {
        let project_id =
            ProjectId::parse(&snapshot.project_id).map_err(|_| WorkstreamClaimError::NotFound)?;
        let workspace_id = WorkspaceId::parse(&snapshot.workspace_id)
            .map_err(|_| WorkstreamClaimError::NotFound)?;
        let claims = self
            .list_group(group_id, &project_id, &workspace_id, actor)
            .await?;
        let dependencies = self
            .list_dependencies(group_id, &project_id, &workspace_id, actor)
            .await?;
        let projected_claims = claims
            .iter()
            .map(WorkstreamClaim::to_projection)
            .collect::<Result<Vec<_>, _>>()?;
        let projected_dependencies = dependencies
            .iter()
            .map(WorkstreamClaimDependency::to_projection)
            .collect::<Vec<_>>();
        codegg_protocol::projection::project_workstream_with_claims(
            snapshot,
            group_id.as_str(),
            &projected_claims,
            &projected_dependencies,
        )
        .map_err(|_| WorkstreamClaimError::NotFound)
    }

    /// Bounded append-only transition provenance. This carries no claim
    /// target, description, prompt, or free-form agent text.
    pub async fn list_events(
        &self,
        group_id: &AgentRunGroupId,
        project_id: &ProjectId,
        workspace_id: &WorkspaceId,
        actor: &ClaimActor,
    ) -> Result<Vec<WorkstreamClaimEvent>, WorkstreamClaimError> {
        let mut conn = self.pool.acquire().await.map_err(storage_error)?;
        validate_group_actor(&mut conn, group_id, project_id, workspace_id, actor).await?;
        let rows = sqlx::query(
            "SELECT event_id, claim_id, actor_kind, actor_id, action, version, lease_generation, created_at \
             FROM workstream_claim_event WHERE group_id=? ORDER BY created_at, event_id LIMIT ?",
        )
        .bind(group_id.as_str())
        .bind(MAX_CLAIM_EVENTS_PER_GROUP as i64 + 1)
        .fetch_all(&mut *conn)
        .await
        .map_err(storage_error)?;
        if rows.len() > MAX_CLAIM_EVENTS_PER_GROUP {
            return Err(WorkstreamClaimError::CorruptState);
        }
        rows.iter()
            .map(|row| {
                let actor = match row
                    .try_get::<String, _>("actor_kind")
                    .map_err(storage_error)?
                    .as_str()
                {
                    "principal" => ClaimActor::Principal(
                        PrincipalId::parse(
                            &row.try_get::<String, _>("actor_id")
                                .map_err(storage_error)?,
                        )
                        .map_err(|_| WorkstreamClaimError::CorruptState)?,
                    ),
                    "run" => ClaimActor::Run(
                        AgentRunId::parse(
                            &row.try_get::<String, _>("actor_id")
                                .map_err(storage_error)?,
                        )
                        .map_err(|_| WorkstreamClaimError::CorruptState)?,
                    ),
                    _ => return Err(WorkstreamClaimError::CorruptState),
                };
                Ok(WorkstreamClaimEvent {
                    event_id: row.try_get("event_id").map_err(storage_error)?,
                    claim_id: WorkstreamClaimId::parse(
                        &row.try_get::<String, _>("claim_id")
                            .map_err(storage_error)?,
                    )
                    .map_err(|_| WorkstreamClaimError::CorruptState)?,
                    actor,
                    action: row.try_get("action").map_err(storage_error)?,
                    version: row.try_get::<i64, _>("version").map_err(storage_error)? as u64,
                    lease_generation: row
                        .try_get::<i64, _>("lease_generation")
                        .map_err(storage_error)? as u64,
                    created_at: row.try_get("created_at").map_err(storage_error)?,
                })
            })
            .collect()
    }

    pub async fn list_dependencies(
        &self,
        group_id: &AgentRunGroupId,
        project_id: &ProjectId,
        workspace_id: &WorkspaceId,
        actor: &ClaimActor,
    ) -> Result<Vec<WorkstreamClaimDependency>, WorkstreamClaimError> {
        let mut conn = self.pool.acquire().await.map_err(storage_error)?;
        validate_group_actor(&mut conn, group_id, project_id, workspace_id, actor).await?;
        let rows = sqlx::query(
            "SELECT d.claim_id, d.prerequisite_claim_id, d.prerequisite_run_id, d.created_at \
             FROM workstream_claim_dependency d JOIN workstream_claim c ON c.claim_id=d.claim_id \
             WHERE c.group_id=? AND c.project_id=? AND c.workspace_id=? \
             ORDER BY d.created_at, d.claim_id LIMIT ?",
        )
        .bind(group_id.as_str())
        .bind(project_id.as_str())
        .bind(workspace_id.as_str())
        .bind((MAX_CLAIMS_PER_GROUP * MAX_CLAIM_DEPENDENCIES + 1) as i64)
        .fetch_all(&mut *conn)
        .await
        .map_err(storage_error)?;
        if rows.len() > MAX_CLAIMS_PER_GROUP * MAX_CLAIM_DEPENDENCIES {
            return Err(WorkstreamClaimError::CorruptState);
        }
        rows.iter()
            .map(|row| {
                let claim_id = WorkstreamClaimId::parse(
                    &row.try_get::<String, _>("claim_id")
                        .map_err(storage_error)?,
                )
                .map_err(|_| WorkstreamClaimError::CorruptState)?;
                let prerequisite_claim: Option<String> = row
                    .try_get("prerequisite_claim_id")
                    .map_err(storage_error)?;
                let prerequisite_run: Option<String> =
                    row.try_get("prerequisite_run_id").map_err(storage_error)?;
                let prerequisite = match (prerequisite_claim, prerequisite_run) {
                    (Some(id), None) => ClaimPrerequisite::Claim(
                        WorkstreamClaimId::parse(&id)
                            .map_err(|_| WorkstreamClaimError::CorruptState)?,
                    ),
                    (None, Some(id)) => ClaimPrerequisite::Run(
                        AgentRunId::parse(&id).map_err(|_| WorkstreamClaimError::CorruptState)?,
                    ),
                    _ => return Err(WorkstreamClaimError::CorruptState),
                };
                Ok(WorkstreamClaimDependency {
                    claim_id,
                    prerequisite,
                    created_at: row.try_get("created_at").map_err(storage_error)?,
                })
            })
            .collect()
    }

    /// Create an exact task claim or an advisory path/interface intent.
    /// The group/member checks and target arbitration run under SQLite's
    /// immediate writer lock so simultaneous contenders see one winner.
    pub async fn claim(
        &self,
        input: NewWorkstreamClaim,
    ) -> Result<WorkstreamClaim, WorkstreamClaimError> {
        let target = input.target.normalized()?;
        let normalized_target = match &input.target {
            ClaimTarget::Task(id) => ClaimTarget::Task(id.clone()),
            ClaimTarget::Path(_) => ClaimTarget::Path(target.clone()),
            ClaimTarget::Interface(_) => ClaimTarget::Interface(target.clone()),
        };
        let input = NewWorkstreamClaim {
            target: normalized_target,
            ..input
        };
        if input.idempotency_key.is_empty()
            || input.idempotency_key.len() > MAX_CLAIM_IDEMPOTENCY_BYTES
            || input.lease_expires_at.is_some_and(|deadline| {
                deadline <= now_ms() || deadline > now_ms().saturating_add(MAX_CLAIM_LEASE_MS)
            })
        {
            return Err(WorkstreamClaimError::InvalidTarget);
        }
        let mut conn = self.pool.acquire().await.map_err(storage_error)?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(storage_error)?;
        let result = self.claim_locked(&mut conn, input, target).await;
        match result {
            Ok(claim) => {
                sqlx::query("COMMIT")
                    .execute(&mut *conn)
                    .await
                    .map_err(storage_error)?;
                Ok(claim)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                Err(error)
            }
        }
    }

    async fn claim_locked(
        &self,
        conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
        input: NewWorkstreamClaim,
        target: String,
    ) -> Result<WorkstreamClaim, WorkstreamClaimError> {
        validate_group_actor(
            conn,
            &input.group_id,
            &input.project_id,
            &input.workspace_id,
            &input.actor,
        )
        .await?;
        if let ClaimTarget::Task(task_id) = &input.target {
            let member: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM agent_run_group_member gm \
                 JOIN agent_run r ON r.run_id = gm.run_id \
                 JOIN agent_task t ON t.task_id = r.task_id \
                 WHERE gm.group_id = ? AND t.task_id = ? AND t.project_id = ? AND t.workspace_id = ?",
            )
            .bind(input.group_id.as_str())
            .bind(task_id.as_str())
            .bind(input.project_id.as_str())
            .bind(input.workspace_id.as_str())
            .fetch_one(&mut **conn)
            .await
            .map_err(storage_error)?;
            if member == 0 {
                return Err(WorkstreamClaimError::InvalidTarget);
            }
        }

        if let Some(existing) = sqlx::query(
            "SELECT claim_id, project_id, workspace_id, group_id, claim_kind, target, state, \
             owner_kind, owner_id, version, lease_generation, lease_expires_at, created_at, updated_at \
             FROM workstream_claim WHERE group_id = ? AND idempotency_key = ?",
        )
        .bind(input.group_id.as_str())
        .bind(&input.idempotency_key)
        .fetch_optional(&mut **conn)
        .await
        .map_err(storage_error)?
        {
            let claim = row_to_claim(&existing)?;
            if claim.project_id == input.project_id
                && claim.workspace_id == input.workspace_id
                && claim.target.kind() == input.target.kind()
                && claim.target.normalized()? == target
                && claim.owner == input.actor
            {
                return Ok(claim);
            }
            return Err(WorkstreamClaimError::IdempotencyConflict);
        }

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM workstream_claim WHERE group_id = ?")
                .bind(input.group_id.as_str())
                .fetch_one(&mut **conn)
                .await
                .map_err(storage_error)?;
        if count as usize >= MAX_CLAIMS_PER_GROUP {
            return Err(WorkstreamClaimError::Capacity);
        }

        let active = sqlx::query(
            "SELECT claim_id, project_id, workspace_id, group_id, claim_kind, target, state, \
             owner_kind, owner_id, version, lease_generation, lease_expires_at, created_at, updated_at \
             FROM workstream_claim WHERE group_id = ? AND state IN ('offered','claimed','blocked')",
        )
        .bind(input.group_id.as_str())
        .fetch_all(&mut **conn)
        .await
        .map_err(storage_error)?;
        for row in active {
            let current = row_to_claim(&row)?;
            if current
                .lease_expires_at
                .is_some_and(|deadline| deadline <= now_ms())
            {
                sqlx::query(
                    "UPDATE workstream_claim SET state='released', version=version+1, \
                     lease_generation=lease_generation+1, updated_at=? WHERE claim_id=? \
                     AND version=? AND lease_generation=?",
                )
                .bind(now_ms())
                .bind(current.claim_id.as_str())
                .bind(current.version as i64)
                .bind(current.lease_generation as i64)
                .execute(&mut **conn)
                .await
                .map_err(storage_error)?;
                let mut expired = current;
                expired.state = ClaimState::Released;
                expired.version += 1;
                expired.lease_generation += 1;
                expired.lease_expires_at = None;
                expired.updated_at = now_ms();
                insert_claim_event(conn, &expired, &expired.owner, "expired").await?;
                continue;
            }
            if current.project_id != input.project_id || current.workspace_id != input.workspace_id
            {
                return Err(WorkstreamClaimError::InvalidDependency);
            }
            if targets_overlap(
                current.target.kind(),
                &current.target.normalized()?,
                input.target.kind(),
                &target,
            ) {
                return Err(WorkstreamClaimError::Overlap);
            }
        }

        let id = WorkstreamClaimId::new();
        let now = now_ms();
        let inserted = sqlx::query(
            "INSERT INTO workstream_claim \
             (claim_id, project_id, workspace_id, group_id, claim_kind, target, state, \
              owner_kind, owner_id, version, lease_generation, lease_expires_at, idempotency_key, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'claimed', ?, ?, 1, 1, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(input.project_id.as_str())
        .bind(input.workspace_id.as_str())
        .bind(input.group_id.as_str())
        .bind(input.target.kind())
        .bind(&target)
        .bind(input.actor.kind())
        .bind(input.actor.id())
        .bind(input.lease_expires_at)
        .bind(&input.idempotency_key)
        .bind(now)
        .bind(now)
        .execute(&mut **conn)
        .await
        .map_err(|error| {
            if error.to_string().contains("UNIQUE constraint failed") {
                WorkstreamClaimError::OwnedByAnother
            } else {
                storage_error(error)
            }
        })?;
        let _ = inserted;
        let claim = WorkstreamClaim {
            claim_id: id,
            project_id: input.project_id,
            workspace_id: input.workspace_id,
            group_id: input.group_id,
            target: input.target,
            state: ClaimState::Claimed,
            owner: input.actor,
            version: 1,
            lease_generation: 1,
            lease_expires_at: input.lease_expires_at,
            created_at: now,
            updated_at: now,
        };
        insert_claim_event(conn, &claim, &claim.owner, "claimed").await?;
        Ok(claim)
    }

    /// Revision- and generation-fenced lifecycle transition.
    pub async fn transition(&self, cas: ClaimCas) -> Result<WorkstreamClaim, WorkstreamClaimError> {
        let mut conn = self.pool.acquire().await.map_err(storage_error)?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(storage_error)?;
        let result = self.transition_locked(&mut conn, cas).await;
        match result {
            Ok(claim) => {
                sqlx::query("COMMIT")
                    .execute(&mut *conn)
                    .await
                    .map_err(storage_error)?;
                Ok(claim)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                Err(error)
            }
        }
    }

    async fn transition_locked(
        &self,
        conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
        cas: ClaimCas,
    ) -> Result<WorkstreamClaim, WorkstreamClaimError> {
        let row = sqlx::query(
            "SELECT claim_id, project_id, workspace_id, group_id, claim_kind, target, state, \
             owner_kind, owner_id, version, lease_generation, lease_expires_at, created_at, updated_at \
             FROM workstream_claim WHERE claim_id = ?",
        )
        .bind(cas.claim_id.as_str())
        .fetch_optional(&mut **conn)
        .await
        .map_err(storage_error)?
        .ok_or(WorkstreamClaimError::NotFound)?;
        let mut claim = row_to_claim(&row)?;
        validate_group_actor(
            conn,
            &claim.group_id,
            &claim.project_id,
            &claim.workspace_id,
            &cas.actor,
        )
        .await?;
        if claim.owner != cas.actor {
            return Err(WorkstreamClaimError::OwnedByAnother);
        }
        if claim.version != cas.expected_version
            || claim.lease_generation != cas.expected_lease_generation
        {
            return Err(WorkstreamClaimError::StaleRevision);
        }
        if claim
            .lease_expires_at
            .is_some_and(|deadline| deadline <= now_ms())
        {
            return Err(WorkstreamClaimError::StaleRevision);
        }
        let (state, owner, deadline, bump_generation) = transition_target(&claim, &cas)?;
        if let Some(new_owner) = owner.as_ref() {
            validate_group_actor(
                conn,
                &claim.group_id,
                &claim.project_id,
                &claim.workspace_id,
                new_owner,
            )
            .await?;
        }
        let now = now_ms();
        let result = sqlx::query(
            "UPDATE workstream_claim SET state=?, owner_kind=?, owner_id=?, version=version+1, \
             lease_generation=lease_generation+?, lease_expires_at=?, updated_at=? \
             WHERE claim_id=? AND version=? AND lease_generation=?",
        )
        .bind(state.as_str())
        .bind(owner.as_ref().unwrap_or(&claim.owner).kind())
        .bind(owner.as_ref().unwrap_or(&claim.owner).id())
        .bind(if bump_generation { 1_i64 } else { 0_i64 })
        .bind(deadline)
        .bind(now)
        .bind(claim.claim_id.as_str())
        .bind(cas.expected_version as i64)
        .bind(cas.expected_lease_generation as i64)
        .execute(&mut **conn)
        .await
        .map_err(storage_error)?;
        if result.rows_affected() != 1 {
            return Err(WorkstreamClaimError::StaleRevision);
        }
        claim.state = state;
        if let Some(owner) = owner {
            claim.owner = owner;
        }
        claim.version += 1;
        if bump_generation {
            claim.lease_generation += 1;
        }
        claim.lease_expires_at = deadline;
        claim.updated_at = now;
        insert_claim_event(conn, &claim, &cas.actor, transition_action(cas.transition)).await?;
        Ok(claim)
    }

    /// Add a same-group prerequisite with transactional cycle/fanout checks.
    pub async fn add_dependency(
        &self,
        claim_id: &WorkstreamClaimId,
        prerequisite: ClaimPrerequisite,
        actor: &ClaimActor,
        expected_version: u64,
    ) -> Result<WorkstreamClaim, WorkstreamClaimError> {
        let mut conn = self.pool.acquire().await.map_err(storage_error)?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(storage_error)?;
        let result = self
            .add_dependency_locked(&mut conn, claim_id, prerequisite, actor, expected_version)
            .await;
        match result {
            Ok(claim) => {
                sqlx::query("COMMIT")
                    .execute(&mut *conn)
                    .await
                    .map_err(storage_error)?;
                Ok(claim)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                Err(error)
            }
        }
    }

    async fn add_dependency_locked(
        &self,
        conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
        claim_id: &WorkstreamClaimId,
        prerequisite: ClaimPrerequisite,
        actor: &ClaimActor,
        expected_version: u64,
    ) -> Result<WorkstreamClaim, WorkstreamClaimError> {
        let row = sqlx::query(
            "SELECT claim_id, project_id, workspace_id, group_id, claim_kind, target, state, \
             owner_kind, owner_id, version, lease_generation, lease_expires_at, created_at, updated_at \
             FROM workstream_claim WHERE claim_id=?",
        )
        .bind(claim_id.as_str())
        .fetch_optional(&mut **conn)
        .await
        .map_err(storage_error)?
        .ok_or(WorkstreamClaimError::NotFound)?;
        let mut claim = row_to_claim(&row)?;
        validate_group_actor(
            conn,
            &claim.group_id,
            &claim.project_id,
            &claim.workspace_id,
            actor,
        )
        .await?;
        if claim.owner != *actor {
            return Err(WorkstreamClaimError::OwnedByAnother);
        }
        if claim.version != expected_version {
            return Err(WorkstreamClaimError::StaleRevision);
        }
        let fanout: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM workstream_claim_dependency WHERE claim_id=?")
                .bind(claim_id.as_str())
                .fetch_one(&mut **conn)
                .await
                .map_err(storage_error)?;
        if fanout as usize >= MAX_CLAIM_DEPENDENCIES {
            return Err(WorkstreamClaimError::Capacity);
        }
        let (prerequisite_claim, prerequisite_run) = match prerequisite {
            ClaimPrerequisite::Claim(id) => {
                if id == *claim_id {
                    return Err(WorkstreamClaimError::DependencyCycle);
                }
                let other: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM workstream_claim WHERE claim_id=? AND group_id=? AND project_id=? AND workspace_id=?",
                )
                .bind(id.as_str())
                .bind(claim.group_id.as_str())
                .bind(claim.project_id.as_str())
                .bind(claim.workspace_id.as_str())
                .fetch_one(&mut **conn)
                .await
                .map_err(storage_error)?;
                if other == 0 {
                    return Err(WorkstreamClaimError::InvalidDependency);
                }
                let cycle: i64 = sqlx::query_scalar(
                    "WITH RECURSIVE deps(id) AS ( \
                     SELECT prerequisite_claim_id FROM workstream_claim_dependency WHERE claim_id=? AND prerequisite_claim_id IS NOT NULL \
                     UNION SELECT d.prerequisite_claim_id FROM workstream_claim_dependency d JOIN deps x ON d.claim_id=x.id WHERE d.prerequisite_claim_id IS NOT NULL \
                     ) SELECT COUNT(*) FROM deps WHERE id=?",
                )
                .bind(id.as_str())
                .bind(claim_id.as_str())
                .fetch_one(&mut **conn)
                .await
                .map_err(storage_error)?;
                if cycle > 0 {
                    return Err(WorkstreamClaimError::DependencyCycle);
                }
                (Some(id), None)
            }
            ClaimPrerequisite::Run(id) => {
                let member: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM agent_run_group_member gm \
                     JOIN agent_run r ON r.run_id=gm.run_id \
                     JOIN agent_task t ON t.task_id=r.task_id \
                     WHERE gm.group_id=? AND r.run_id=? AND t.project_id=? AND t.workspace_id=?",
                )
                .bind(claim.group_id.as_str())
                .bind(id.as_str())
                .bind(claim.project_id.as_str())
                .bind(claim.workspace_id.as_str())
                .fetch_one(&mut **conn)
                .await
                .map_err(storage_error)?;
                if member == 0 {
                    return Err(WorkstreamClaimError::InvalidDependency);
                }
                (None, Some(id))
            }
        };
        let now = now_ms();
        sqlx::query(
            "INSERT INTO workstream_claim_dependency (claim_id, prerequisite_claim_id, prerequisite_run_id, created_at) VALUES (?,?,?,?)",
        )
        .bind(claim_id.as_str())
        .bind(prerequisite_claim.as_ref().map(|id| id.as_str()))
        .bind(prerequisite_run.as_ref().map(|id| id.as_str()))
        .bind(now)
        .execute(&mut **conn)
        .await
        .map_err(|error| {
            if error.to_string().contains("UNIQUE constraint failed") {
                WorkstreamClaimError::DependencyCycle
            } else {
                storage_error(error)
            }
        })?;
        let updated = sqlx::query(
            "UPDATE workstream_claim SET version=version+1, updated_at=? WHERE claim_id=? AND version=?",
        )
        .bind(now)
        .bind(claim_id.as_str())
        .bind(expected_version as i64)
        .execute(&mut **conn)
        .await
        .map_err(storage_error)?;
        if updated.rows_affected() != 1 {
            return Err(WorkstreamClaimError::StaleRevision);
        }
        claim.version += 1;
        claim.updated_at = now;
        insert_claim_event(conn, &claim, actor, "dependency_added").await?;
        Ok(claim)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkstreamClaimEvent {
    pub event_id: String,
    pub claim_id: WorkstreamClaimId,
    pub actor: ClaimActor,
    pub action: String,
    pub version: u64,
    pub lease_generation: u64,
    pub created_at: i64,
}

async fn validate_group_actor(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    group_id: &AgentRunGroupId,
    project_id: &ProjectId,
    workspace_id: &WorkspaceId,
    actor: &ClaimActor,
) -> Result<(), WorkstreamClaimError> {
    let row = sqlx::query(
        "SELECT g.owner_kind, g.owner_run_id, g.owner_session_id, \
         (SELECT COUNT(*) FROM agent_run_group_member gm \
          JOIN agent_run r ON r.run_id=gm.run_id \
          JOIN agent_task t ON t.task_id=r.task_id \
          WHERE gm.group_id=g.group_id AND t.project_id=? AND t.workspace_id=?) AS scoped_members \
         FROM agent_run_group g WHERE g.group_id=?",
    )
    .bind(project_id.as_str())
    .bind(workspace_id.as_str())
    .bind(group_id.as_str())
    .fetch_optional(&mut **conn)
    .await
    .map_err(storage_error)?
    .ok_or(WorkstreamClaimError::NotFound)?;
    if row
        .try_get::<i64, _>("scoped_members")
        .map_err(storage_error)?
        == 0
    {
        return Err(WorkstreamClaimError::NotFound);
    }
    match actor {
        ClaimActor::Run(id) => {
            let authorized: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM agent_run_group g JOIN agent_run r ON r.run_id=? \
                 JOIN agent_task t ON t.task_id=r.task_id \
                 WHERE g.group_id=? AND t.project_id=? AND t.workspace_id=? \
                 AND r.status NOT IN ('completed','failed','interrupted','cancelled') \
                 AND (g.owner_run_id=? OR EXISTS(SELECT 1 FROM agent_run_group_member gm WHERE gm.group_id=g.group_id AND gm.run_id=r.run_id))",
            )
            .bind(id.as_str())
            .bind(group_id.as_str())
            .bind(project_id.as_str())
            .bind(workspace_id.as_str())
            .bind(id.as_str())
            .fetch_one(&mut **conn)
            .await
            .map_err(storage_error)?;
            if authorized == 0 {
                return Err(WorkstreamClaimError::Unauthorized);
            }
        }
        ClaimActor::Principal(id) => {
            let owner_kind: String = row.try_get("owner_kind").map_err(storage_error)?;
            let session: Option<String> = row.try_get("owner_session_id").map_err(storage_error)?;
            let authorized: i64 = if owner_kind == "turn" {
                if let Some(session_id) = session {
                    sqlx::query_scalar(
                        "SELECT COUNT(*) FROM origin_attribution WHERE scope_kind='session' \
                     AND scope_id=? AND origin_principal=?",
                    )
                    .bind(session_id)
                    .bind(id.as_str())
                    .fetch_one(&mut **conn)
                    .await
                    .map_err(storage_error)?
                } else {
                    0
                }
            } else {
                0
            };
            if authorized == 0 {
                return Err(WorkstreamClaimError::Unauthorized);
            }
        }
    }
    Ok(())
}

fn transition_target(
    claim: &WorkstreamClaim,
    cas: &ClaimCas,
) -> Result<(ClaimState, Option<ClaimActor>, Option<i64>, bool), WorkstreamClaimError> {
    let outcome = match cas.transition {
        ClaimTransition::Renew if claim.state == ClaimState::Claimed => {
            let deadline = validate_lease(cas.lease_expires_at)?;
            (ClaimState::Claimed, None, deadline, false)
        }
        ClaimTransition::Release
            if matches!(
                claim.state,
                ClaimState::Offered | ClaimState::Claimed | ClaimState::Blocked
            ) =>
        {
            (ClaimState::Released, None, None, true)
        }
        ClaimTransition::Block if claim.state == ClaimState::Claimed => {
            (ClaimState::Blocked, None, claim.lease_expires_at, false)
        }
        ClaimTransition::Unblock if claim.state == ClaimState::Blocked => {
            (ClaimState::Claimed, None, claim.lease_expires_at, false)
        }
        ClaimTransition::Complete if claim.state == ClaimState::Claimed => {
            (ClaimState::Completed, None, None, true)
        }
        ClaimTransition::Handoff
            if matches!(claim.state, ClaimState::Claimed | ClaimState::Blocked) =>
        {
            let owner = cas
                .new_owner
                .clone()
                .ok_or(WorkstreamClaimError::InvalidTransition)?;
            if owner == claim.owner {
                return Err(WorkstreamClaimError::InvalidTransition);
            }
            (
                ClaimState::Offered,
                Some(owner),
                validate_lease(cas.lease_expires_at)?,
                true,
            )
        }
        ClaimTransition::AcceptOffer if claim.state == ClaimState::Offered => {
            (ClaimState::Claimed, None, claim.lease_expires_at, false)
        }
        _ => return Err(WorkstreamClaimError::InvalidTransition),
    };
    Ok(outcome)
}

fn validate_lease(deadline: Option<i64>) -> Result<Option<i64>, WorkstreamClaimError> {
    let now = now_ms();
    match deadline {
        Some(deadline) if deadline > now && deadline <= now.saturating_add(MAX_CLAIM_LEASE_MS) => {
            Ok(Some(deadline))
        }
        _ => Err(WorkstreamClaimError::InvalidTransition),
    }
}

fn row_to_claim(row: &sqlx::sqlite::SqliteRow) -> Result<WorkstreamClaim, WorkstreamClaimError> {
    let target_kind: String = row.try_get("claim_kind").map_err(storage_error)?;
    let target_value: String = row.try_get("target").map_err(storage_error)?;
    let target = match target_kind.as_str() {
        "task" => ClaimTarget::Task(
            AgentTaskId::parse(&target_value).map_err(|_| WorkstreamClaimError::CorruptState)?,
        ),
        "path" => ClaimTarget::Path(target_value),
        "interface" => ClaimTarget::Interface(target_value),
        _ => return Err(WorkstreamClaimError::CorruptState),
    };
    let owner_kind: String = row.try_get("owner_kind").map_err(storage_error)?;
    let owner_id: String = row.try_get("owner_id").map_err(storage_error)?;
    let owner = match owner_kind.as_str() {
        "principal" => ClaimActor::Principal(
            PrincipalId::parse(&owner_id).map_err(|_| WorkstreamClaimError::CorruptState)?,
        ),
        "run" => ClaimActor::Run(
            AgentRunId::parse(&owner_id).map_err(|_| WorkstreamClaimError::CorruptState)?,
        ),
        _ => return Err(WorkstreamClaimError::CorruptState),
    };
    Ok(WorkstreamClaim {
        claim_id: WorkstreamClaimId::parse(
            &row.try_get::<String, _>("claim_id")
                .map_err(storage_error)?,
        )
        .map_err(|_| WorkstreamClaimError::CorruptState)?,
        project_id: ProjectId::parse(
            &row.try_get::<String, _>("project_id")
                .map_err(storage_error)?,
        )
        .map_err(|_| WorkstreamClaimError::CorruptState)?,
        workspace_id: WorkspaceId::parse(
            &row.try_get::<String, _>("workspace_id")
                .map_err(storage_error)?,
        )
        .map_err(|_| WorkstreamClaimError::CorruptState)?,
        group_id: AgentRunGroupId::parse(
            &row.try_get::<String, _>("group_id")
                .map_err(storage_error)?,
        )
        .map_err(|_| WorkstreamClaimError::CorruptState)?,
        target,
        state: ClaimState::parse(&row.try_get::<String, _>("state").map_err(storage_error)?)?,
        owner,
        version: row.try_get::<i64, _>("version").map_err(storage_error)? as u64,
        lease_generation: row
            .try_get::<i64, _>("lease_generation")
            .map_err(storage_error)? as u64,
        lease_expires_at: row.try_get("lease_expires_at").map_err(storage_error)?,
        created_at: row.try_get("created_at").map_err(storage_error)?,
        updated_at: row.try_get("updated_at").map_err(storage_error)?,
    })
}

fn normalize_relative_intent(value: &str) -> Result<String, WorkstreamClaimError> {
    if value.is_empty()
        || value.len() > MAX_CLAIM_TARGET_BYTES
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        return Err(WorkstreamClaimError::InvalidTarget);
    }
    let normalized: String = value.nfc().collect();
    if normalized.len() > MAX_CLAIM_TARGET_BYTES
        || normalized
            .chars()
            .any(|ch| ch.is_control() || is_bidi_control(ch))
    {
        return Err(WorkstreamClaimError::InvalidTarget);
    }
    let segments: Vec<_> = normalized.split('/').collect();
    if segments.iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.matches('*').count() + part.matches('?').count() > 4
            || part.contains("**")
            || part.chars().any(|ch| matches!(ch, '[' | ']' | '{' | '}'))
    }) {
        return Err(WorkstreamClaimError::InvalidTarget);
    }
    if segments.len() == 1 && matches!(segments[0], "*" | "?") {
        return Err(WorkstreamClaimError::InvalidTarget);
    }
    Ok(normalized)
}

fn is_bidi_control(ch: char) -> bool {
    matches!(ch as u32, 0x061C | 0x200E | 0x200F | 0x202A..=0x202E | 0x2066..=0x2069)
}

/// Conservative advisory overlap check. It intentionally allows false
/// positives and never grants disjoint-write or filesystem authority.
fn targets_overlap(left_kind: &str, left: &str, right_kind: &str, right: &str) -> bool {
    if left_kind == "task" || right_kind == "task" {
        return left_kind == right_kind && left == right;
    }
    if left == right {
        return true;
    }
    fn fixed_prefix(pattern: &str) -> &str {
        pattern
            .char_indices()
            .find(|(_, ch)| matches!(ch, '*' | '?'))
            .map_or(pattern, |(index, _)| &pattern[..index])
    }
    let left_folded = left.to_lowercase();
    let right_folded = right.to_lowercase();
    let left_prefix = fixed_prefix(&left_folded);
    let right_prefix = fixed_prefix(&right_folded);
    left_prefix.starts_with(right_prefix) || right_prefix.starts_with(left_prefix)
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn storage_error(error: impl std::fmt::Display) -> WorkstreamClaimError {
    WorkstreamClaimError::Storage(error.to_string())
}

async fn insert_claim_event(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    claim: &WorkstreamClaim,
    actor: &ClaimActor,
    action: &str,
) -> Result<(), WorkstreamClaimError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workstream_claim_event WHERE group_id=?")
            .bind(claim.group_id.as_str())
            .fetch_one(&mut **conn)
            .await
            .map_err(storage_error)?;
    if count as usize >= MAX_CLAIM_EVENTS_PER_GROUP {
        return Err(WorkstreamClaimError::Capacity);
    }
    sqlx::query(
        "INSERT INTO workstream_claim_event \
         (event_id, claim_id, group_id, actor_kind, actor_id, action, version, lease_generation, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(claim.claim_id.as_str())
    .bind(claim.group_id.as_str())
    .bind(actor.kind())
    .bind(actor.id())
    .bind(action)
    .bind(claim.version as i64)
    .bind(claim.lease_generation as i64)
    .bind(claim.updated_at)
    .execute(&mut **conn)
    .await
    .map_err(storage_error)?;
    Ok(())
}

fn transition_action(transition: ClaimTransition) -> &'static str {
    match transition {
        ClaimTransition::Renew => "renewed",
        ClaimTransition::Release => "released",
        ClaimTransition::Block => "blocked",
        ClaimTransition::Unblock => "unblocked",
        ClaimTransition::Complete => "completed",
        ClaimTransition::Handoff => "handed_off",
        ClaimTransition::AcceptOffer => "offer_accepted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_intents_reject_traversal_wildcard_bombs_and_bidi() {
        for invalid in [
            "../secret",
            "/absolute",
            "src\\escape.rs",
            "**/all.rs",
            "*",
            "src/../../escape",
            "src/ab**cd.rs",
            "src/\u{202e}name.rs",
        ] {
            assert!(
                normalize_relative_intent(invalid).is_err(),
                "accepted invalid intent {invalid:?}"
            );
        }
        assert_eq!(normalize_relative_intent("e\u{301}.rs").unwrap(), "é.rs");
    }

    #[test]
    fn conservative_glob_overlap_is_advisory_and_exact_task_ids_are_authoritative() {
        assert!(targets_overlap("path", "src/*.rs", "path", "src/main.rs"));
        assert!(targets_overlap(
            "interface",
            "api/v1",
            "path",
            "api/v1/users.rs"
        ));
        assert!(!targets_overlap("task", "task_a", "task", "task_b"));
        assert!(targets_overlap("task", "task_a", "task", "task_a"));
    }
}
