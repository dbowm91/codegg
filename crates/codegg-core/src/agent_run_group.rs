//! Durable, bounded coordination for a set of delegated agent runs.
//!
//! A group never admits work and never owns an executor.  Its members are
//! already durable `AgentRun` records and are admitted independently by the
//! scheduler.  This module only records membership, computes a deterministic
//! join result, and propagates an explicitly requested cancellation.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{broadcast, Mutex, Notify};

use super::agent_run::{AgentRunStatus, AgentRunStore};
use crate::identity::{AgentRunGroupId, AgentRunId};

pub const MAX_GROUP_MEMBERS: usize = 16;
pub const MAX_GROUP_SUMMARY_MEMBERS: usize = MAX_GROUP_MEMBERS;
pub const MAX_GROUP_WAIT_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunJoinPolicy {
    All,
    AnySuccessful,
    FirstCompleted,
    Detached,
}

impl RunJoinPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::AnySuccessful => "any_successful",
            Self::FirstCompleted => "first_completed",
            Self::Detached => "detached",
        }
    }

    fn parse(value: &str) -> Result<Self, AgentRunGroupError> {
        match value {
            "all" => Ok(Self::All),
            "any_successful" => Ok(Self::AnySuccessful),
            "first_completed" => Ok(Self::FirstCompleted),
            "detached" => Ok(Self::Detached),
            other => Err(AgentRunGroupError::Store(format!(
                "unknown join policy '{other}'"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunGroupStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl RunGroupStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn parse(value: &str) -> Result<Self, AgentRunGroupError> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(AgentRunGroupError::Store(format!(
                "unknown group status '{other}'"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunGroupRecord {
    pub group_id: AgentRunGroupId,
    pub root_run_id: AgentRunId,
    pub owner_run_id: AgentRunId,
    pub member_run_ids: Vec<AgentRunId>,
    pub join_policy: RunJoinPolicy,
    pub cancel_remaining_on_satisfaction: bool,
    pub status: RunGroupStatus,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub winner_run_id: Option<AgentRunId>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunGroupMemberSummary {
    pub ordinal: u32,
    pub run_id: AgentRunId,
    pub status: AgentRunStatus,
    pub result_ref: Option<String>,
    pub failure_class: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunGroupSummary {
    pub group: AgentRunGroupRecord,
    pub members: Vec<AgentRunGroupMemberSummary>,
    pub successful: usize,
    pub failed: usize,
    pub active: usize,
    pub timed_out: bool,
}

#[derive(Debug, Clone)]
pub struct NewAgentRunGroup {
    pub group_id: AgentRunGroupId,
    pub root_run_id: AgentRunId,
    pub owner_run_id: AgentRunId,
    pub member_run_ids: Vec<AgentRunId>,
    pub join_policy: RunJoinPolicy,
    pub cancel_remaining_on_satisfaction: bool,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunGroupNotification {
    pub group_id: AgentRunGroupId,
    pub owner_run_id: AgentRunId,
    pub status: RunGroupStatus,
    pub successful: usize,
    pub failed: usize,
    pub member_count: usize,
}

#[derive(Debug, Error)]
pub enum AgentRunGroupError {
    #[error("group store error: {0}")]
    Store(String),
    #[error("group '{0}' not found")]
    NotFound(String),
    #[error("group member limit is {MAX_GROUP_MEMBERS}")]
    TooManyMembers,
    #[error("group must contain at least one member")]
    EmptyMembers,
    #[error("group contains duplicate member runs")]
    DuplicateMember,
    #[error("group member '{0}' is not an owned direct child of the group owner")]
    UnauthorizedMember(String),
    #[error("group actor is not authorized")]
    Unauthorized,
    #[error("group idempotency key conflicts with an existing group")]
    IdempotencyConflict,
    #[error("group wait timed out")]
    WaitTimedOut(Box<AgentRunGroupSummary>),
}

#[async_trait]
pub trait AgentRunGroupStore: Send + Sync {
    async fn create_or_get(
        &self,
        input: NewAgentRunGroup,
    ) -> Result<AgentRunGroupRecord, AgentRunGroupError>;
    async fn get(
        &self,
        group_id: &AgentRunGroupId,
    ) -> Result<Option<AgentRunGroupRecord>, AgentRunGroupError>;
    async fn update(&self, group: AgentRunGroupRecord) -> Result<(), AgentRunGroupError>;
    async fn list_by_owner(
        &self,
        owner_run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunGroupRecord>, AgentRunGroupError>;
    async fn list_by_member(
        &self,
        member_run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunGroupRecord>, AgentRunGroupError>;
    async fn claim_notification(
        &self,
        group_id: &AgentRunGroupId,
    ) -> Result<bool, AgentRunGroupError>;
}

#[derive(Default)]
struct MemoryState {
    groups: HashMap<AgentRunGroupId, AgentRunGroupRecord>,
    by_key: HashMap<String, AgentRunGroupId>,
    notification_claimed: HashSet<AgentRunGroupId>,
}

#[derive(Default)]
pub struct InMemoryAgentRunGroupStore {
    state: Mutex<MemoryState>,
}

impl InMemoryAgentRunGroupStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn validate_new(input: &NewAgentRunGroup) -> Result<(), AgentRunGroupError> {
    if input.member_run_ids.is_empty() {
        return Err(AgentRunGroupError::EmptyMembers);
    }
    if input.member_run_ids.len() > MAX_GROUP_MEMBERS {
        return Err(AgentRunGroupError::TooManyMembers);
    }
    if input.member_run_ids.iter().collect::<HashSet<_>>().len() != input.member_run_ids.len() {
        return Err(AgentRunGroupError::DuplicateMember);
    }
    if input.idempotency_key.is_empty() || input.idempotency_key.len() > 512 {
        return Err(AgentRunGroupError::Store(
            "group idempotency key must be between 1 and 512 bytes".into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl AgentRunGroupStore for InMemoryAgentRunGroupStore {
    async fn create_or_get(
        &self,
        input: NewAgentRunGroup,
    ) -> Result<AgentRunGroupRecord, AgentRunGroupError> {
        validate_new(&input)?;
        let mut state = self.state.lock().await;
        if let Some(id) = state.by_key.get(&input.idempotency_key) {
            let existing = state
                .groups
                .get(id)
                .ok_or_else(|| AgentRunGroupError::NotFound(id.to_string()))?;
            if existing.root_run_id != input.root_run_id
                || existing.owner_run_id != input.owner_run_id
                || existing.member_run_ids != input.member_run_ids
                || existing.join_policy != input.join_policy
                || existing.cancel_remaining_on_satisfaction
                    != input.cancel_remaining_on_satisfaction
            {
                return Err(AgentRunGroupError::IdempotencyConflict);
            }
            return Ok(existing.clone());
        }
        let now = Utc::now().timestamp_millis();
        let record = AgentRunGroupRecord {
            group_id: input.group_id,
            root_run_id: input.root_run_id,
            owner_run_id: input.owner_run_id,
            member_run_ids: input.member_run_ids,
            join_policy: input.join_policy,
            cancel_remaining_on_satisfaction: input.cancel_remaining_on_satisfaction,
            status: RunGroupStatus::Pending,
            created_at: now,
            completed_at: None,
            winner_run_id: None,
            idempotency_key: input.idempotency_key.clone(),
        };
        state
            .by_key
            .insert(input.idempotency_key, record.group_id.clone());
        state.groups.insert(record.group_id.clone(), record.clone());
        Ok(record)
    }

    async fn get(
        &self,
        group_id: &AgentRunGroupId,
    ) -> Result<Option<AgentRunGroupRecord>, AgentRunGroupError> {
        Ok(self.state.lock().await.groups.get(group_id).cloned())
    }

    async fn update(&self, group: AgentRunGroupRecord) -> Result<(), AgentRunGroupError> {
        let mut state = self.state.lock().await;
        if !state.groups.contains_key(&group.group_id) {
            return Err(AgentRunGroupError::NotFound(group.group_id.to_string()));
        }
        state.groups.insert(group.group_id.clone(), group);
        Ok(())
    }

    async fn list_by_owner(
        &self,
        owner_run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunGroupRecord>, AgentRunGroupError> {
        Ok(self
            .state
            .lock()
            .await
            .groups
            .values()
            .filter(|group| &group.owner_run_id == owner_run_id)
            .cloned()
            .collect())
    }

    async fn list_by_member(
        &self,
        member_run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunGroupRecord>, AgentRunGroupError> {
        Ok(self
            .state
            .lock()
            .await
            .groups
            .values()
            .filter(|group| group.member_run_ids.iter().any(|id| id == member_run_id))
            .cloned()
            .collect())
    }

    async fn claim_notification(
        &self,
        group_id: &AgentRunGroupId,
    ) -> Result<bool, AgentRunGroupError> {
        let mut state = self.state.lock().await;
        if state.notification_claimed.insert(group_id.clone()) {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

pub struct SqliteAgentRunGroupStore {
    pool: SqlitePool,
}

impl SqliteAgentRunGroupStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn parse_group(value: String) -> Result<AgentRunGroupId, AgentRunGroupError> {
    AgentRunGroupId::parse(&value).map_err(|e| AgentRunGroupError::Store(e.to_string()))
}

fn parse_run(value: String) -> Result<AgentRunId, AgentRunGroupError> {
    AgentRunId::parse(&value).map_err(|e| AgentRunGroupError::Store(e.to_string()))
}

async fn load_sqlite_group(
    pool: &SqlitePool,
    group_id: &AgentRunGroupId,
) -> Result<Option<AgentRunGroupRecord>, AgentRunGroupError> {
    let row = sqlx::query(
        "SELECT group_id, root_run_id, owner_run_id, join_policy, cancel_remaining, status, created_at, completed_at, winner_run_id, idempotency_key FROM agent_run_group WHERE group_id = ?",
    )
    .bind(group_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
    let Some(row) = row else { return Ok(None) };
    let members = sqlx::query(
        "SELECT run_id FROM agent_run_group_member WHERE group_id = ? ORDER BY ordinal",
    )
    .bind(group_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(|e| AgentRunGroupError::Store(e.to_string()))?
    .into_iter()
    .map(|row| parse_run(row.get("run_id")))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(AgentRunGroupRecord {
        group_id: parse_group(row.get("group_id"))?,
        root_run_id: parse_run(row.get("root_run_id"))?,
        owner_run_id: parse_run(row.get("owner_run_id"))?,
        member_run_ids: members,
        join_policy: RunJoinPolicy::parse(row.get::<String, _>("join_policy").as_str())?,
        cancel_remaining_on_satisfaction: row.get::<i64, _>("cancel_remaining") != 0,
        status: RunGroupStatus::parse(row.get::<String, _>("status").as_str())?,
        created_at: row.get("created_at"),
        completed_at: row.get("completed_at"),
        winner_run_id: row
            .get::<Option<String>, _>("winner_run_id")
            .map(parse_run)
            .transpose()?,
        idempotency_key: row.get("idempotency_key"),
    }))
}

#[async_trait]
impl AgentRunGroupStore for SqliteAgentRunGroupStore {
    async fn create_or_get(
        &self,
        input: NewAgentRunGroup,
    ) -> Result<AgentRunGroupRecord, AgentRunGroupError> {
        validate_new(&input)?;
        if let Some(existing) =
            sqlx::query("SELECT group_id FROM agent_run_group WHERE idempotency_key = ?")
                .bind(&input.idempotency_key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AgentRunGroupError::Store(e.to_string()))?
        {
            let id = parse_group(existing.get("group_id"))?;
            let record = load_sqlite_group(&self.pool, &id)
                .await?
                .ok_or_else(|| AgentRunGroupError::NotFound(id.to_string()))?;
            if record.root_run_id != input.root_run_id
                || record.owner_run_id != input.owner_run_id
                || record.member_run_ids != input.member_run_ids
                || record.join_policy != input.join_policy
                || record.cancel_remaining_on_satisfaction != input.cancel_remaining_on_satisfaction
            {
                return Err(AgentRunGroupError::IdempotencyConflict);
            }
            return Ok(record);
        }
        let now = Utc::now().timestamp_millis();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
        let inserted = sqlx::query("INSERT OR IGNORE INTO agent_run_group (group_id, root_run_id, owner_run_id, join_policy, cancel_remaining, status, created_at, idempotency_key) VALUES (?, ?, ?, ?, ?, 'pending', ?, ?)")
            .bind(input.group_id.as_str())
            .bind(input.root_run_id.as_str())
            .bind(input.owner_run_id.as_str())
            .bind(input.join_policy.as_str())
            .bind(i64::from(input.cancel_remaining_on_satisfaction))
            .bind(now)
            .bind(&input.idempotency_key)
            .execute(&mut *tx)
            .await
            .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
        if inserted.rows_affected() == 0 {
            tx.rollback()
                .await
                .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
            let existing =
                sqlx::query("SELECT group_id FROM agent_run_group WHERE idempotency_key = ?")
                    .bind(&input.idempotency_key)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| AgentRunGroupError::Store(e.to_string()))?
                    .ok_or_else(|| {
                        AgentRunGroupError::Store("group identity already exists".into())
                    })?;
            let id = parse_group(existing.get("group_id"))?;
            let record = load_sqlite_group(&self.pool, &id)
                .await?
                .ok_or_else(|| AgentRunGroupError::NotFound(id.to_string()))?;
            if record.root_run_id != input.root_run_id
                || record.owner_run_id != input.owner_run_id
                || record.member_run_ids != input.member_run_ids
                || record.join_policy != input.join_policy
                || record.cancel_remaining_on_satisfaction != input.cancel_remaining_on_satisfaction
            {
                return Err(AgentRunGroupError::IdempotencyConflict);
            }
            return Ok(record);
        }
        for (ordinal, run_id) in input.member_run_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO agent_run_group_member (group_id, ordinal, run_id) VALUES (?, ?, ?)",
            )
            .bind(input.group_id.as_str())
            .bind(ordinal as i64)
            .bind(run_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
        load_sqlite_group(&self.pool, &input.group_id)
            .await?
            .ok_or_else(|| AgentRunGroupError::NotFound(input.group_id.to_string()))
    }

    async fn get(
        &self,
        group_id: &AgentRunGroupId,
    ) -> Result<Option<AgentRunGroupRecord>, AgentRunGroupError> {
        load_sqlite_group(&self.pool, group_id).await
    }

    async fn update(&self, group: AgentRunGroupRecord) -> Result<(), AgentRunGroupError> {
        let result = sqlx::query("UPDATE agent_run_group SET status = ?, completed_at = ?, winner_run_id = ? WHERE group_id = ?")
            .bind(group.status.as_str())
            .bind(group.completed_at)
            .bind(group.winner_run_id.as_ref().map(AgentRunId::as_str))
            .bind(group.group_id.as_str())
            .execute(&self.pool)
            .await
            .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(AgentRunGroupError::NotFound(group.group_id.to_string()));
        }
        Ok(())
    }

    async fn list_by_owner(
        &self,
        owner_run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunGroupRecord>, AgentRunGroupError> {
        let rows = sqlx::query(
            "SELECT group_id FROM agent_run_group WHERE owner_run_id = ? ORDER BY created_at",
        )
        .bind(owner_run_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
        let mut groups = Vec::with_capacity(rows.len());
        for row in rows {
            let id = parse_group(row.get("group_id"))?;
            if let Some(group) = load_sqlite_group(&self.pool, &id).await? {
                groups.push(group);
            }
        }
        Ok(groups)
    }

    async fn list_by_member(
        &self,
        member_run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunGroupRecord>, AgentRunGroupError> {
        let rows = sqlx::query(
            "SELECT group_id FROM agent_run_group_member WHERE run_id = ? ORDER BY ordinal",
        )
        .bind(member_run_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
        let mut groups = Vec::with_capacity(rows.len());
        for row in rows {
            let id = parse_group(row.get("group_id"))?;
            if let Some(group) = load_sqlite_group(&self.pool, &id).await? {
                groups.push(group);
            }
        }
        Ok(groups)
    }

    async fn claim_notification(
        &self,
        group_id: &AgentRunGroupId,
    ) -> Result<bool, AgentRunGroupError> {
        let result = sqlx::query("UPDATE agent_run_group SET notification_claimed = 1 WHERE group_id = ? AND notification_claimed = 0")
            .bind(group_id.as_str())
            .execute(&self.pool)
            .await
            .map_err(|e| AgentRunGroupError::Store(e.to_string()))?;
        Ok(result.rows_affected() == 1)
    }
}

#[derive(Debug, Clone)]
pub struct GroupActor {
    pub run_id: Option<AgentRunId>,
    pub session_id: Option<String>,
}

pub struct AgentRunGroupService {
    runs: Arc<dyn AgentRunStore>,
    groups: Arc<dyn AgentRunGroupStore>,
    changed: Arc<Notify>,
    notifications: broadcast::Sender<AgentRunGroupNotification>,
}

impl AgentRunGroupService {
    pub fn in_memory(runs: Arc<dyn AgentRunStore>) -> Arc<Self> {
        let (notifications, _) = broadcast::channel(32);
        Arc::new(Self {
            runs,
            groups: Arc::new(InMemoryAgentRunGroupStore::new()),
            changed: Arc::new(Notify::new()),
            notifications,
        })
    }

    pub fn with_store(
        runs: Arc<dyn AgentRunStore>,
        groups: Arc<dyn AgentRunGroupStore>,
    ) -> Arc<Self> {
        let (notifications, _) = broadcast::channel(32);
        Arc::new(Self {
            runs,
            groups,
            changed: Arc::new(Notify::new()),
            notifications,
        })
    }

    pub fn with_pool(runs: Arc<dyn AgentRunStore>, pool: SqlitePool) -> Arc<Self> {
        Self::with_store(runs, Arc::new(SqliteAgentRunGroupStore::new(pool)))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentRunGroupNotification> {
        self.notifications.subscribe()
    }

    pub async fn create(
        &self,
        input: NewAgentRunGroup,
    ) -> Result<AgentRunGroupSummary, AgentRunGroupError> {
        validate_new(&input)?;
        let owner = self
            .runs
            .get_run(&input.owner_run_id)
            .await
            .map_err(|e| AgentRunGroupError::Store(e.to_string()))?
            .ok_or_else(|| {
                AgentRunGroupError::UnauthorizedMember(input.owner_run_id.to_string())
            })?;
        if owner.root_run_id != input.root_run_id {
            return Err(AgentRunGroupError::Unauthorized);
        }
        let mut seen = HashSet::new();
        for member_id in &input.member_run_ids {
            if !seen.insert(member_id) {
                return Err(AgentRunGroupError::DuplicateMember);
            }
            let member = self
                .runs
                .get_run(member_id)
                .await
                .map_err(|e| AgentRunGroupError::Store(e.to_string()))?
                .ok_or_else(|| AgentRunGroupError::UnauthorizedMember(member_id.to_string()))?;
            if member.root_run_id != input.root_run_id
                || member.parent_run_id.as_ref() != Some(&input.owner_run_id)
            {
                return Err(AgentRunGroupError::UnauthorizedMember(
                    member_id.to_string(),
                ));
            }
        }
        let group = self.groups.create_or_get(input).await?;
        self.recompute_group(group).await
    }

    pub async fn status(
        &self,
        actor: &GroupActor,
        group_id: AgentRunGroupId,
    ) -> Result<AgentRunGroupSummary, AgentRunGroupError> {
        let group = self.authorize(actor, &group_id).await?;
        self.recompute_group(group).await
    }

    pub async fn wait(
        &self,
        actor: &GroupActor,
        group_id: AgentRunGroupId,
        timeout: std::time::Duration,
    ) -> Result<AgentRunGroupSummary, AgentRunGroupError> {
        let timeout = timeout.min(std::time::Duration::from_millis(MAX_GROUP_WAIT_MS));
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let group = self.authorize(actor, &group_id).await?;
            let summary = self.refresh(&group.group_id).await?;
            if summary.group.status.is_terminal() {
                return Ok(summary);
            }
            let notified = self.changed.notified();
            tokio::select! {
                _ = notified => {},
                _ = tokio::time::sleep_until(deadline) => {
                    let current = self.refresh(&group.group_id).await?;
                    return Err(AgentRunGroupError::WaitTimedOut(Box::new(current)));
                }
            }
        }
    }

    pub async fn cancel(
        &self,
        actor: &GroupActor,
        group_id: AgentRunGroupId,
    ) -> Result<AgentRunGroupSummary, AgentRunGroupError> {
        let group = self.authorize(actor, &group_id).await?;
        if !group.status.is_terminal() {
            let mut cancelled = group.clone();
            cancelled.status = RunGroupStatus::Cancelled;
            cancelled.completed_at = Some(Utc::now().timestamp_millis());
            self.groups.update(cancelled.clone()).await?;
            for run_id in &group.member_run_ids {
                let _ = self.runs.request_cancel(run_id).await;
            }
            self.emit_notification(&cancelled, 0, group.member_run_ids.len())
                .await?;
            self.changed.notify_waiters();
        }
        self.refresh(&group.group_id).await
    }

    /// Recompute after a member terminal transition. Callers that receive
    /// scheduler/run terminal events should invoke this method; no per-group
    /// polling task is created.
    pub async fn member_changed(
        &self,
        member_run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunGroupSummary>, AgentRunGroupError> {
        let groups = self.groups.list_by_member(member_run_id).await?;
        let mut summaries = Vec::with_capacity(groups.len());
        for group in groups {
            summaries.push(self.refresh(&group.group_id).await?);
        }
        Ok(summaries)
    }

    pub async fn recompute(
        &self,
        owner_run_id: &AgentRunId,
    ) -> Result<AgentRunGroupSummary, AgentRunGroupError> {
        let groups = self.groups.list_by_owner(owner_run_id).await?;
        let group = groups
            .into_iter()
            .next()
            .ok_or_else(|| AgentRunGroupError::NotFound(owner_run_id.to_string()))?;
        self.recompute_group(group).await
    }

    pub async fn refresh(
        &self,
        group_id: &AgentRunGroupId,
    ) -> Result<AgentRunGroupSummary, AgentRunGroupError> {
        let group = self
            .groups
            .get(group_id)
            .await?
            .ok_or_else(|| AgentRunGroupError::NotFound(group_id.to_string()))?;
        self.recompute_group(group).await
    }

    /// Return groups that reference a run as owner or member. This is a
    /// read-only snapshot seam for projections and restart reconciliation;
    /// it does not admit work or alter group state.
    pub async fn groups_for_run(
        &self,
        run_id: &AgentRunId,
    ) -> Result<Vec<AgentRunGroupRecord>, AgentRunGroupError> {
        let mut groups = self.groups.list_by_owner(run_id).await?;
        for group in self.groups.list_by_member(run_id).await? {
            if !groups
                .iter()
                .any(|existing| existing.group_id == group.group_id)
            {
                groups.push(group);
            }
        }
        Ok(groups)
    }

    async fn recompute_group(
        &self,
        mut group: AgentRunGroupRecord,
    ) -> Result<AgentRunGroupSummary, AgentRunGroupError> {
        let mut members = Vec::with_capacity(group.member_run_ids.len());
        for (ordinal, run_id) in group.member_run_ids.iter().enumerate() {
            let run = self
                .runs
                .get_run(run_id)
                .await
                .map_err(|e| AgentRunGroupError::Store(e.to_string()))?
                .ok_or_else(|| AgentRunGroupError::UnauthorizedMember(run_id.to_string()))?;
            members.push(AgentRunGroupMemberSummary {
                ordinal: ordinal as u32,
                run_id: run.run_id,
                status: run.status,
                result_ref: run.result_ref,
                failure_class: run.failure_class,
                failure_message: run.failure_message,
            });
        }
        let successful = members
            .iter()
            .filter(|member| member.status == AgentRunStatus::Completed)
            .count();
        let active = members
            .iter()
            .filter(|member| !member.status.is_terminal())
            .count();
        let failed = members
            .iter()
            .filter(|member| {
                member.status.is_terminal() && member.status != AgentRunStatus::Completed
            })
            .count();

        if !group.status.is_terminal() {
            let first_terminal = members.iter().find(|member| member.status.is_terminal());
            let (status, winner) = match group.join_policy {
                RunJoinPolicy::All => {
                    if active == 0 {
                        if failed == 0 {
                            (RunGroupStatus::Completed, None)
                        } else {
                            (RunGroupStatus::Failed, None)
                        }
                    } else {
                        (RunGroupStatus::Running, None)
                    }
                }
                RunJoinPolicy::AnySuccessful => {
                    if successful > 0 {
                        let winner = members
                            .iter()
                            .find(|member| member.status == AgentRunStatus::Completed)
                            .map(|member| member.run_id.clone());
                        (RunGroupStatus::Completed, winner)
                    } else if active == 0 {
                        (RunGroupStatus::Failed, None)
                    } else {
                        (RunGroupStatus::Running, None)
                    }
                }
                RunJoinPolicy::FirstCompleted => match first_terminal {
                    Some(member) if member.status == AgentRunStatus::Completed => {
                        (RunGroupStatus::Completed, Some(member.run_id.clone()))
                    }
                    Some(member) if member.status == AgentRunStatus::Cancelled => {
                        (RunGroupStatus::Cancelled, Some(member.run_id.clone()))
                    }
                    Some(member) => (RunGroupStatus::Failed, Some(member.run_id.clone())),
                    None => (RunGroupStatus::Running, None),
                },
                RunJoinPolicy::Detached => {
                    if active == 0 {
                        if failed == 0 {
                            (RunGroupStatus::Completed, None)
                        } else {
                            (RunGroupStatus::Failed, None)
                        }
                    } else {
                        (RunGroupStatus::Running, None)
                    }
                }
            };
            if status != group.status || winner.is_some() {
                group.status = if group.status == RunGroupStatus::Pending
                    && status == RunGroupStatus::Running
                {
                    RunGroupStatus::Running
                } else {
                    status
                };
                if group.winner_run_id.is_none() {
                    group.winner_run_id = winner;
                }
                if group.status.is_terminal() {
                    group.completed_at = Some(Utc::now().timestamp_millis());
                }
                self.groups.update(group.clone()).await?;
                if group.status.is_terminal() {
                    if group.cancel_remaining_on_satisfaction
                        && matches!(
                            group.join_policy,
                            RunJoinPolicy::AnySuccessful | RunJoinPolicy::FirstCompleted
                        )
                    {
                        for member in &members {
                            if !member.status.is_terminal()
                                && group.winner_run_id.as_ref() != Some(&member.run_id)
                            {
                                let _ = self.runs.request_cancel(&member.run_id).await;
                            }
                        }
                    }
                    self.emit_notification(&group, successful, failed).await?;
                }
            }
        }
        let timed_out = false;
        self.changed.notify_waiters();
        Ok(AgentRunGroupSummary {
            group,
            members,
            successful,
            failed,
            active,
            timed_out,
        })
    }

    async fn emit_notification(
        &self,
        group: &AgentRunGroupRecord,
        successful: usize,
        failed: usize,
    ) -> Result<(), AgentRunGroupError> {
        if self.groups.claim_notification(&group.group_id).await? {
            let _ = self.notifications.send(AgentRunGroupNotification {
                group_id: group.group_id.clone(),
                owner_run_id: group.owner_run_id.clone(),
                status: group.status,
                successful,
                failed,
                member_count: group.member_run_ids.len(),
            });
        }
        Ok(())
    }

    async fn authorize(
        &self,
        actor: &GroupActor,
        group_id: &AgentRunGroupId,
    ) -> Result<AgentRunGroupRecord, AgentRunGroupError> {
        let group = self
            .groups
            .get(group_id)
            .await?
            .ok_or_else(|| AgentRunGroupError::NotFound(group_id.to_string()))?;
        if actor.run_id.as_ref() == Some(&group.owner_run_id) {
            return Ok(group);
        }
        if let Some(session_id) = actor.session_id.as_deref() {
            let owner = self
                .runs
                .get_run(&group.owner_run_id)
                .await
                .map_err(|e| AgentRunGroupError::Store(e.to_string()))?
                .ok_or(AgentRunGroupError::Unauthorized)?;
            let task = self
                .runs
                .get_task(&owner.task_id)
                .await
                .map_err(|e| AgentRunGroupError::Store(e.to_string()))?
                .ok_or(AgentRunGroupError::Unauthorized)?;
            if task.originating_session_id == session_id {
                return Ok(group);
            }
        }
        Err(AgentRunGroupError::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::super::agent_run::{AgentRunBudget, AgentTaskStatus, NewAgentRun, NewAgentTask};
    use super::*;
    use crate::identity::{AgentTaskId, ProjectId};
    use crate::workspace::WorkspaceId;

    async fn make_run(
        runs: &Arc<dyn AgentRunStore>,
        parent: Option<(&AgentRunId, &AgentTaskId)>,
        key: &str,
    ) -> AgentRunRecordForTest {
        let task_id = AgentTaskId::new();
        let run_id = AgentRunId::new();
        let submission = runs
            .create_or_get(
                NewAgentTask {
                    task_id: task_id.clone(),
                    parent_task_id: parent.map(|(_, task)| task.clone()),
                    originating_session_id: "group-test-session".into(),
                    originating_turn_id: None,
                    project_id: ProjectId::new(),
                    repository_id: None,
                    workspace_id: WorkspaceId::new_unchecked("group-test-workspace"),
                    requested_agent: "general".into(),
                    delegation_key: key.into(),
                    description: key.into(),
                },
                NewAgentRun {
                    run_id,
                    parent_run_id: parent.map(|(run, _)| run.clone()),
                    workspace_id: WorkspaceId::new_unchecked("group-test-workspace"),
                    agent_name: "general".into(),
                    agent_digest: None,
                    provider: "test".into(),
                    model: "test".into(),
                    authority_digest: "test".into(),
                    budget: AgentRunBudget::default(),
                },
            )
            .await
            .unwrap();
        runs.transition_task(&task_id, AgentTaskStatus::Queued)
            .await
            .unwrap();
        runs.transition(&submission.run.run_id, AgentRunStatus::Queued)
            .await
            .unwrap();
        runs.transition(&submission.run.run_id, AgentRunStatus::Preparing)
            .await
            .unwrap();
        runs.transition(&submission.run.run_id, AgentRunStatus::Running)
            .await
            .unwrap();
        AgentRunRecordForTest {
            task_id,
            run_id: submission.run.run_id,
        }
    }

    #[derive(Clone)]
    struct AgentRunRecordForTest {
        task_id: AgentTaskId,
        run_id: AgentRunId,
    }

    async fn group(
        policy: RunJoinPolicy,
        cancel_remaining: bool,
    ) -> (
        Arc<dyn AgentRunStore>,
        Arc<AgentRunGroupService>,
        AgentRunId,
        Vec<AgentRunId>,
    ) {
        let runs: Arc<dyn AgentRunStore> =
            Arc::new(super::super::agent_run::InMemoryAgentRunStore::new());
        let owner = make_run(&runs, None, "owner").await;
        let first = make_run(&runs, Some((&owner.run_id, &owner.task_id)), "first").await;
        let second = make_run(&runs, Some((&owner.run_id, &owner.task_id)), "second").await;
        let service = AgentRunGroupService::in_memory(runs.clone());
        let result = service
            .create(NewAgentRunGroup {
                group_id: AgentRunGroupId::new(),
                root_run_id: owner.run_id.clone(),
                owner_run_id: owner.run_id.clone(),
                member_run_ids: vec![first.run_id.clone(), second.run_id.clone()],
                join_policy: policy,
                cancel_remaining_on_satisfaction: cancel_remaining,
                idempotency_key: format!("group-{policy:?}"),
            })
            .await
            .unwrap();
        (
            runs,
            service,
            result.group.owner_run_id,
            result.group.member_run_ids,
        )
    }

    #[tokio::test]
    async fn all_requires_every_member_and_reports_partial_failure() {
        let (runs, service, owner, members) = group(RunJoinPolicy::All, false).await;
        runs.finish(
            &members[0],
            super::super::agent_run::AgentRunTerminalOutcome::Completed,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let running = service.recompute(&owner).await.unwrap();
        assert_eq!(running.group.status, RunGroupStatus::Running);
        runs.finish(
            &members[1],
            super::super::agent_run::AgentRunTerminalOutcome::Failed,
            None,
            Some("test".into()),
            None,
        )
        .await
        .unwrap();
        let complete = service.recompute(&owner).await.unwrap();
        assert_eq!(complete.group.status, RunGroupStatus::Failed);
        assert_eq!(
            (complete.successful, complete.failed, complete.active),
            (1, 1, 0)
        );
    }

    #[tokio::test]
    async fn any_successful_wins_and_cancels_only_remaining_members() {
        let (runs, service, owner, members) = group(RunJoinPolicy::AnySuccessful, true).await;
        runs.finish(
            &members[0],
            super::super::agent_run::AgentRunTerminalOutcome::Completed,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let summary = service.recompute(&owner).await.unwrap();
        assert_eq!(summary.group.status, RunGroupStatus::Completed);
        assert_eq!(summary.group.winner_run_id, Some(members[0].clone()));
        assert_eq!(
            runs.get_run(&members[1]).await.unwrap().unwrap().status,
            AgentRunStatus::Cancelling
        );
    }

    #[tokio::test]
    async fn first_completed_uses_member_order_for_a_deterministic_tie() {
        let (runs, service, owner, members) = group(RunJoinPolicy::FirstCompleted, false).await;
        runs.finish(
            &members[1],
            super::super::agent_run::AgentRunTerminalOutcome::Failed,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        runs.finish(
            &members[0],
            super::super::agent_run::AgentRunTerminalOutcome::Completed,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let summary = service.recompute(&owner).await.unwrap();
        assert_eq!(summary.group.winner_run_id, Some(members[0].clone()));
        assert_eq!(summary.group.status, RunGroupStatus::Completed);
    }

    #[tokio::test]
    async fn detached_waits_for_all_members_and_emits_once() {
        let (runs, service, owner, members) = group(RunJoinPolicy::Detached, false).await;
        let mut notifications = service.subscribe();
        runs.finish(
            &members[0],
            super::super::agent_run::AgentRunTerminalOutcome::Completed,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            service.recompute(&owner).await.unwrap().group.status,
            RunGroupStatus::Running
        );
        runs.finish(
            &members[1],
            super::super::agent_run::AgentRunTerminalOutcome::Completed,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let summary = service.recompute(&owner).await.unwrap();
        assert_eq!(summary.group.status, RunGroupStatus::Completed);
        assert_eq!(notifications.recv().await.unwrap().member_count, 2);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), notifications.recv())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn sqlite_group_reloads_after_service_recreation_without_respawn() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        super::super::session::schema::migrate(&pool).await.unwrap();
        let runs: Arc<dyn AgentRunStore> = Arc::new(
            super::super::agent_run::SqliteAgentRunStore::new(pool.clone()),
        );
        let owner = make_run(&runs, None, "sqlite-owner").await;
        let member = make_run(
            &runs,
            Some((&owner.run_id, &owner.task_id)),
            "sqlite-member",
        )
        .await;
        let service = AgentRunGroupService::with_store(
            runs.clone(),
            Arc::new(SqliteAgentRunGroupStore::new(pool.clone())),
        );
        let created = service
            .create(NewAgentRunGroup {
                group_id: AgentRunGroupId::new(),
                root_run_id: owner.run_id.clone(),
                owner_run_id: owner.run_id.clone(),
                member_run_ids: vec![member.run_id.clone()],
                join_policy: RunJoinPolicy::Detached,
                cancel_remaining_on_satisfaction: false,
                idempotency_key: "sqlite-group-restart".into(),
            })
            .await
            .unwrap();
        runs.finish(
            &member.run_id,
            super::super::agent_run::AgentRunTerminalOutcome::Completed,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        service.recompute(&owner.run_id).await.unwrap();

        let restarted =
            AgentRunGroupService::with_store(runs, Arc::new(SqliteAgentRunGroupStore::new(pool)));
        let summary = restarted
            .status(
                &GroupActor {
                    run_id: Some(owner.run_id),
                    session_id: None,
                },
                created.group.group_id,
            )
            .await
            .unwrap();
        assert_eq!(summary.group.status, RunGroupStatus::Completed);
        assert_eq!(summary.members.len(), 1);
    }
}
