//! Concrete [`ScheduleStore`] implementations: in-memory and SQLite.
//!
//! Both implementations use an [`Arc<dyn JobStore>`] reference to count
//! running jobs per schedule when evaluating overlap policy in
//! [`claim_due`].
//!
//! # The tick runs in three phases
//!
//! [`claim_due`] is deliberately split into **plan**, **materialize**, and
//! **commit**:
//!
//! 1. **Plan** — a short read of the due schedules and the occurrences
//!    each one owes. No transaction is held.
//! 2. **Materialize** — the slow part: a job submission, and for a remote
//!    target a node round trip. It runs with **no** transaction open.
//! 3. **Commit** — one write transaction that records the occurrence rows
//!    and advances the schedules' cursors together.
//!
//! The split exists because materialization needs the job store, and the
//! job store needs a pool connection. A materialization performed while
//! the schedule store held its own connection would wait for a second
//! connection that a single-connection pool never has — a self-deadlock
//! that no amount of waiting resolves.
//!
//! The phases are safe to separate because the occurrence's job id is a
//! pure function of `(schedule_id, scheduled_for)`: a crash between the
//! materialization and the commit replays into the *same* job, and the
//! `job.id` primary key keeps it to one.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use tokio::sync::Mutex as AsyncMutex;

use crate::error::StorageError;
use crate::jobs::{JobId, JobState, JobStore, JobStoreQuery, ScheduleId};
use crate::session::user_prompt_queue::ImmediateWrite;
use crate::workspace::WorkspaceId;

use super::schedule::{
    compute_next_run, missed_run_targets, ClaimedOccurrence, JobTemplate, MissedRunPolicy,
    OccurrenceMaterializer, OccurrenceStatus, OverlapPolicy, ScheduleError, ScheduleKind,
    ScheduleQuery, ScheduleRecord, ScheduleState, ScheduleStore, ScheduleSummary, ScheduleTemplate,
};

// ── Tick phases ───────────────────────────────────────────────────────────

/// The tick's plan for one due schedule.
struct SchedulePlan {
    schedule: ScheduleRecord,
    /// The occurrences this schedule owes, in order.
    targets: Vec<DateTime<Utc>>,
    /// The number of this schedule's jobs already running.
    running: u32,
}

/// What the tick decided about one occurrence.
#[derive(Clone)]
enum OccurrenceOutcome {
    /// The occurrence owns a durable job.
    Queued(JobId),
    /// The overlap policy skipped it.
    Skipped,
    /// The materializer refused it. It is not recorded, so it stays
    /// retryable.
    Failed {
        /// A stable, secret-free reason code.
        ///
        /// The operator surface is a log line, so this is the only part
        /// of the refusal that reaches one. It carries no message, no
        /// endpoint, and no policy detail.
        code: String,
    },
}

/// The tick's results for one schedule.
struct ScheduleOutcome {
    schedule_id: ScheduleId,
    kind: ScheduleKind,
    /// The occurrences that reached a decision, in order.
    decided: Vec<(DateTime<Utc>, OccurrenceOutcome)>,
}

impl ScheduleOutcome {
    /// The last occurrence that reached a decision.
    ///
    /// A refused occurrence is not one: the cursor must not advance past
    /// it, or the occurrence would be lost.
    fn last_decided(&self) -> Option<DateTime<Utc>> {
        self.decided
            .iter()
            .rev()
            .find(|(_, outcome)| !matches!(outcome, OccurrenceOutcome::Failed { .. }))
            .map(|(at, _)| *at)
    }

    /// The first refused occurrence, if any.
    fn first_failed(&self) -> Option<DateTime<Utc>> {
        self.decided
            .iter()
            .find(|(_, outcome)| matches!(outcome, OccurrenceOutcome::Failed { .. }))
            .map(|(at, _)| *at)
    }
}

/// Materialize a schedule's planned occurrences.
///
/// The overlap decision is made here, not in the plan phase, because a
/// skipped occurrence must not count towards the running total that
/// skips the next one.
async fn materialize_plan(
    plan: SchedulePlan,
    materialize: &dyn OccurrenceMaterializer,
) -> ScheduleOutcome {
    let overlap = plan.schedule.overlap_policy;
    let mut running = plan.running;
    let mut decided = Vec::new();

    for target in plan.targets {
        if should_skip(overlap, running) {
            decided.push((target, OccurrenceOutcome::Skipped));
            continue;
        }
        match materialize.materialize(&plan.schedule, target).await {
            Ok(job_id) => {
                decided.push((target, OccurrenceOutcome::Queued(job_id)));
                running += 1;
            }
            Err(error) => {
                // The occurrence was refused. It is deliberately **not**
                // recorded: the occurrence row is the durable "this
                // already happened" marker, and its
                // `(schedule_id, scheduled_for)` primary key would turn
                // the later retry into a no-op. Leaving it absent keeps
                // the occurrence retryable.
                //
                // The remaining occurrences of this schedule are not
                // processed, so they stay due and are retried in order.
                decided.push((
                    target,
                    OccurrenceOutcome::Failed {
                        code: error.code().to_owned(),
                    },
                ));
                break;
            }
        }
    }

    ScheduleOutcome {
        schedule_id: plan.schedule.schedule_id.clone(),
        kind: plan.schedule.kind.clone(),
        decided,
    }
}

/// The schedule's next horizon after a tick.
///
/// A refused occurrence keeps the schedule due: the horizon must not move
/// past it, or the occurrence would be lost rather than retried.
fn next_horizon(
    kind: &ScheduleKind,
    now: DateTime<Utc>,
    last_decided: Option<DateTime<Utc>>,
    first_failed: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let mut next = compute_next_run(kind, now, last_decided);
    if let Some(failed_at) = first_failed {
        next = Some(match next {
            Some(candidate) if candidate < failed_at => candidate,
            _ => failed_at,
        });
    }
    next
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn serialize_kind(kind: &ScheduleKind) -> Result<String, ScheduleError> {
    serde_json::to_string(kind).map_err(|e| ScheduleError::Serialization(e.to_string()))
}

fn deserialize_kind(s: &str) -> Result<ScheduleKind, ScheduleError> {
    serde_json::from_str(s).map_err(|e| ScheduleError::Serialization(e.to_string()))
}

fn serialize_template(t: &JobTemplate) -> Result<String, ScheduleError> {
    serde_json::to_string(t).map_err(|e| ScheduleError::Serialization(e.to_string()))
}

fn deserialize_template(s: &str) -> Result<JobTemplate, ScheduleError> {
    serde_json::from_str(s).map_err(|e| ScheduleError::Serialization(e.to_string()))
}

fn serialize_missed_run(p: &MissedRunPolicy) -> Result<String, ScheduleError> {
    serde_json::to_string(p).map_err(|e| ScheduleError::Serialization(e.to_string()))
}

fn deserialize_missed_run(s: &str) -> Result<MissedRunPolicy, ScheduleError> {
    serde_json::from_str(s).map_err(|e| ScheduleError::Serialization(e.to_string()))
}

fn serialize_labels(labels: &HashMap<String, String>) -> String {
    serde_json::to_string(labels).unwrap_or_else(|_| "{}".to_string())
}

fn deserialize_labels(s: &str) -> HashMap<String, String> {
    serde_json::from_str(s).unwrap_or_default()
}

fn state_to_str(s: ScheduleState) -> &'static str {
    s.as_str()
}

fn overlap_to_str(o: OverlapPolicy) -> &'static str {
    match o {
        OverlapPolicy::SkipIfRunning => "skip_if_running",
        OverlapPolicy::QueueOne => "queue_one",
        OverlapPolicy::Allow => "allow",
    }
}

fn overlap_from_str(s: &str) -> OverlapPolicy {
    match s {
        "queue_one" => OverlapPolicy::QueueOne,
        "allow" => OverlapPolicy::Allow,
        _ => OverlapPolicy::SkipIfRunning,
    }
}

fn occurrence_status_to_str(s: OccurrenceStatus) -> &'static str {
    s.as_str()
}

// ── Shared overlap check ─────────────────────────────────────────────────

async fn count_running_for_schedule(
    job_store: &dyn JobStore,
    schedule_id: &ScheduleId,
) -> Result<u32, ScheduleError> {
    let query = JobStoreQuery {
        states: vec![JobState::Running],
        ..Default::default()
    };
    let summaries = job_store
        .list_jobs(query)
        .await
        .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
    let sid = schedule_id.as_str();
    Ok(summaries
        .iter()
        .filter(|s| {
            s.schedule_id
                .as_ref()
                .map(|id| id.as_str() == sid)
                .unwrap_or(false)
        })
        .count() as u32)
}

fn should_skip(overlap: OverlapPolicy, running_count: u32) -> bool {
    match overlap {
        OverlapPolicy::SkipIfRunning => running_count > 0,
        OverlapPolicy::QueueOne => running_count > 0,
        OverlapPolicy::Allow => false,
    }
}

// ── In-memory implementation ─────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct OccurrenceRow {
    schedule_id: ScheduleId,
    scheduled_for: DateTime<Utc>,
    job_id: Option<JobId>,
    status: OccurrenceStatus,
    time_created: DateTime<Utc>,
}

/// In-memory schedule store. Suitable for unit tests and
/// non-durability-required scenarios.
pub struct InMemoryScheduleStore {
    schedules: AsyncMutex<HashMap<String, ScheduleRecord>>,
    occurrences: AsyncMutex<HashMap<(String, i64), OccurrenceRow>>,
    job_store: Arc<dyn JobStore>,
}

impl InMemoryScheduleStore {
    pub fn new(job_store: Arc<dyn JobStore>) -> Self {
        Self {
            schedules: AsyncMutex::new(HashMap::new()),
            occurrences: AsyncMutex::new(HashMap::new()),
            job_store,
        }
    }
}

#[async_trait]
impl ScheduleStore for InMemoryScheduleStore {
    async fn create(&self, template: ScheduleTemplate) -> Result<ScheduleRecord, ScheduleError> {
        let now = Utc::now();
        let schedule_id = ScheduleId::new_unchecked(uuid::Uuid::new_v4().to_string());
        let next_run = template
            .next_run_at
            .unwrap_or_else(|| compute_next_run(&template.kind, now, None).unwrap_or(now));
        let record = ScheduleRecord {
            schedule_id: schedule_id.clone(),
            workspace_id: template.workspace_id,
            session_id: template.session_id,
            kind: template.kind,
            job_template: template.job_template,
            state: ScheduleState::Active,
            overlap_policy: template.overlap_policy,
            missed_run_policy: template.missed_run_policy,
            next_run_at: Some(next_run),
            last_occurrence_at: None,
            created_at: now,
            updated_at: now,
            labels: template.labels,
        };
        self.schedules
            .lock()
            .await
            .insert(schedule_id.to_string(), record.clone());
        Ok(record)
    }

    async fn set_state(
        &self,
        id: &ScheduleId,
        state: ScheduleState,
    ) -> Result<ScheduleRecord, ScheduleError> {
        let mut guard = self.schedules.lock().await;
        let record = guard
            .get(id.as_str())
            .cloned()
            .ok_or_else(|| ScheduleError::ScheduleNotFound(id.to_string()))?;
        if record.state.is_terminal() {
            return Err(ScheduleError::Terminal(id.to_string(), record.state));
        }
        if state.is_terminal() && record.state.is_terminal() {
            return Err(ScheduleError::Terminal(id.to_string(), record.state));
        }
        let now = Utc::now();
        let updated = ScheduleRecord {
            state,
            updated_at: now,
            ..record
        };
        guard.insert(id.to_string(), updated.clone());
        Ok(updated)
    }

    async fn delete(&self, id: &ScheduleId) -> Result<(), ScheduleError> {
        self.schedules.lock().await.remove(id.as_str());
        let prefix = id.to_string();
        self.occurrences
            .lock()
            .await
            .retain(|(sid, _), _| sid != &prefix);
        Ok(())
    }

    async fn get(&self, id: &ScheduleId) -> Result<Option<ScheduleRecord>, ScheduleError> {
        Ok(self.schedules.lock().await.get(id.as_str()).cloned())
    }

    async fn list(&self, query: ScheduleQuery) -> Result<Vec<ScheduleSummary>, ScheduleError> {
        let guard = self.schedules.lock().await;
        let mut out: Vec<ScheduleSummary> = guard
            .values()
            .filter(|r| {
                query
                    .workspace_id
                    .as_ref()
                    .map(|w| r.workspace_id == *w)
                    .unwrap_or(true)
            })
            .filter(|r| query.state.map(|s| r.state == s).unwrap_or(true))
            .filter(|r| query.include_archived || !matches!(r.state, ScheduleState::Archived))
            .map(|r| ScheduleSummary {
                schedule_id: r.schedule_id.to_string(),
                workspace_id: r.workspace_id.to_string(),
                kind: r.kind.clone(),
                state: r.state,
                next_run_at: r.next_run_at,
                last_occurrence_at: r.last_occurrence_at,
            })
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.next_run_at));
        Ok(out)
    }

    async fn claim_due(
        &self,
        now: DateTime<Utc>,
        materialize: &dyn OccurrenceMaterializer,
    ) -> Result<Vec<ClaimedOccurrence>, ScheduleError> {
        // Phase 1 — plan. A short read; no transaction is held across the
        // materialization below.
        let plans = self.plan_due(now).await?;

        // Phase 2 — materialize, with no transaction open.
        let mut outcomes = Vec::new();
        for plan in plans {
            outcomes.push(materialize_plan(plan, materialize).await);
        }

        // Phase 3 — commit: the occurrence rows and the schedules' cursors
        // move together.
        let claimed = self.commit_outcomes(&outcomes, now).await?;
        Ok(claimed)
    }
}

impl InMemoryScheduleStore {
    /// The due schedules and the occurrences each one owes.
    async fn plan_due(&self, now: DateTime<Utc>) -> Result<Vec<SchedulePlan>, ScheduleError> {
        let due: Vec<ScheduleRecord> = {
            let guard = self.schedules.lock().await;
            guard
                .values()
                .filter(|r| r.state == ScheduleState::Active)
                .filter(|r| r.next_run_at.map(|t| t <= now).unwrap_or(false))
                .cloned()
                .collect()
        };

        let mut plans = Vec::new();
        for schedule in due {
            let running =
                count_running_for_schedule(&*self.job_store, &schedule.schedule_id).await?;
            let targets = missed_run_targets(
                &schedule.kind,
                schedule.last_occurrence_at,
                now,
                &schedule.missed_run_policy,
            );
            plans.push(SchedulePlan {
                schedule,
                targets,
                running,
            });
        }
        Ok(plans)
    }

    /// Record the decided occurrences and advance the schedules' cursors.
    async fn commit_outcomes(
        &self,
        outcomes: &[ScheduleOutcome],
        now: DateTime<Utc>,
    ) -> Result<Vec<ClaimedOccurrence>, ScheduleError> {
        let mut claimed = Vec::new();
        let mut schedules = self.schedules.lock().await;
        let mut occurrences = self.occurrences.lock().await;

        for outcome in outcomes {
            let last_decided = outcome.last_decided();
            let first_failed = outcome.first_failed();

            for (target, decision) in &outcome.decided {
                let mut failure_code: Option<String> = None;
                let (job_id, status) = match decision {
                    OccurrenceOutcome::Queued(job_id) => {
                        (Some(job_id.clone()), OccurrenceStatus::Queued)
                    }
                    OccurrenceOutcome::Skipped => (None, OccurrenceStatus::Skipped),
                    // A refused occurrence is not recorded, so the retry
                    // is not turned into a no-op by the primary key.
                    OccurrenceOutcome::Failed { code } => {
                        failure_code = Some(code.clone());
                        (None, OccurrenceStatus::Failed)
                    }
                };
                if !matches!(decision, OccurrenceOutcome::Failed { .. }) {
                    let key = (outcome.schedule_id.as_str().to_owned(), target.timestamp());
                    let row = OccurrenceRow {
                        schedule_id: outcome.schedule_id.clone(),
                        scheduled_for: *target,
                        job_id: job_id.clone(),
                        status,
                        time_created: now,
                    };
                    occurrences.entry(key).or_insert(row);
                }
                claimed.push(ClaimedOccurrence {
                    schedule_id: outcome.schedule_id.clone(),
                    scheduled_for: *target,
                    job_id,
                    status,
                    failure_code,
                });
            }

            let new_next = next_horizon(&outcome.kind, now, last_decided, first_failed);
            if let Some(record) = schedules.get_mut(outcome.schedule_id.as_str()) {
                // The cursor only ever advances: a concurrent tick that
                // already moved it further is not walked back.
                let new_last = match (record.last_occurrence_at, last_decided) {
                    (Some(stored), Some(planned)) => Some(stored.max(planned)),
                    (None, planned) => planned,
                    (stored, None) => stored,
                };
                record.last_occurrence_at = new_last;
                record.next_run_at = new_next;
                record.updated_at = now;
                // A one-shot is complete only once its occurrence reached
                // a decision. A refused occurrence leaves the schedule
                // active so the occurrence is retried.
                if matches!(record.kind, ScheduleKind::OneShot { .. }) && new_next.is_none() {
                    record.state = ScheduleState::Completed;
                }
            }
        }
        Ok(claimed)
    }
}

// ── SQLite implementation ────────────────────────────────────────────────

/// SQLite-backed schedule store.
pub struct SqliteScheduleStore {
    pool: SqlitePool,
    job_store: Arc<dyn JobStore>,
}

impl SqliteScheduleStore {
    pub fn new(pool: SqlitePool, job_store: Arc<dyn JobStore>) -> Self {
        Self { pool, job_store }
    }
}

#[async_trait]
impl ScheduleStore for SqliteScheduleStore {
    async fn create(&self, template: ScheduleTemplate) -> Result<ScheduleRecord, ScheduleError> {
        let now = Utc::now();
        let schedule_id = ScheduleId::new_unchecked(uuid::Uuid::new_v4().to_string());
        let next_run = template
            .next_run_at
            .unwrap_or_else(|| compute_next_run(&template.kind, now, None).unwrap_or(now));
        let kind_json = serialize_kind(&template.kind)?;
        let template_json = serialize_template(&template.job_template)?;
        let missed_json = serialize_missed_run(&template.missed_run_policy)?;
        let labels_json = serialize_labels(&template.labels);

        sqlx::query(
            r#"
            INSERT INTO schedule (
                id, workspace_id, session_id, kind_json, job_template_json,
                state, overlap_policy, missed_run_policy_json,
                next_run_at, last_occurrence_at,
                time_created, time_updated, labels_json
            ) VALUES (?, ?, ?, ?, ?, 'active', ?, ?, ?, NULL, ?, ?, ?)
            "#,
        )
        .bind(schedule_id.as_str())
        .bind(template.workspace_id.as_str())
        .bind(template.session_id.as_deref())
        .bind(&kind_json)
        .bind(&template_json)
        .bind(overlap_to_str(template.overlap_policy))
        .bind(&missed_json)
        .bind(next_run.timestamp_millis())
        .bind(now.timestamp_millis())
        .bind(now.timestamp_millis())
        .bind(&labels_json)
        .execute(&self.pool)
        .await
        .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;

        Ok(ScheduleRecord {
            schedule_id,
            workspace_id: template.workspace_id,
            session_id: template.session_id,
            kind: template.kind,
            job_template: template.job_template,
            state: ScheduleState::Active,
            overlap_policy: template.overlap_policy,
            missed_run_policy: template.missed_run_policy,
            next_run_at: Some(next_run),
            last_occurrence_at: None,
            created_at: now,
            updated_at: now,
            labels: template.labels,
        })
    }

    async fn set_state(
        &self,
        id: &ScheduleId,
        state: ScheduleState,
    ) -> Result<ScheduleRecord, ScheduleError> {
        let existing = self
            .get(id)
            .await?
            .ok_or_else(|| ScheduleError::ScheduleNotFound(id.to_string()))?;
        if existing.state.is_terminal() {
            return Err(ScheduleError::Terminal(id.to_string(), existing.state));
        }
        let now = Utc::now();
        sqlx::query("UPDATE schedule SET state = ?, time_updated = ? WHERE id = ?")
            .bind(state_to_str(state))
            .bind(now.timestamp_millis())
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
        self.get(id)
            .await?
            .ok_or_else(|| ScheduleError::ScheduleNotFound(id.to_string()))
    }

    async fn delete(&self, id: &ScheduleId) -> Result<(), ScheduleError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
        sqlx::query("DELETE FROM schedule_occurrence WHERE schedule_id = ?")
            .bind(id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
        sqlx::query("DELETE FROM schedule WHERE id = ?")
            .bind(id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
        tx.commit()
            .await
            .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
        Ok(())
    }

    async fn get(&self, id: &ScheduleId) -> Result<Option<ScheduleRecord>, ScheduleError> {
        let row = sqlx::query(
            r#"
            SELECT id, workspace_id, session_id, kind_json, job_template_json,
                   state, overlap_policy, missed_run_policy_json,
                   next_run_at, last_occurrence_at,
                   time_created, time_updated, labels_json
            FROM schedule WHERE id = ?
            "#,
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(row_to_schedule(&row)?))
    }

    async fn list(&self, query: ScheduleQuery) -> Result<Vec<ScheduleSummary>, ScheduleError> {
        let mut sql = String::from(
            r#"
            SELECT id, workspace_id, kind_json, state, next_run_at, last_occurrence_at
            FROM schedule WHERE 1=1
            "#,
        );
        if query.workspace_id.is_some() {
            sql.push_str(" AND workspace_id = ?");
        }
        if query.state.is_some() {
            sql.push_str(" AND state = ?");
        }
        if !query.include_archived {
            sql.push_str(" AND state != 'archived'");
        }
        sql.push_str(" ORDER BY COALESCE(next_run_at, 9999999999999) ASC");

        let mut q = sqlx::query(&sql);
        if let Some(w) = &query.workspace_id {
            q = q.bind(w.as_str());
        }
        if let Some(state) = query.state {
            q = q.bind(state_to_str(state));
        }
        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.get("id");
            let workspace_id: String = row.get("workspace_id");
            let kind_json: String = row.get("kind_json");
            let state: String = row.get("state");
            let next_run_at: Option<i64> = row.get("next_run_at");
            let last_occurrence_at: Option<i64> = row.get("last_occurrence_at");
            out.push(ScheduleSummary {
                schedule_id: id,
                workspace_id,
                kind: deserialize_kind(&kind_json)?,
                state: ScheduleState::from_str_lossy(&state),
                next_run_at: next_run_at.and_then(DateTime::<Utc>::from_timestamp_millis),
                last_occurrence_at: last_occurrence_at
                    .and_then(DateTime::<Utc>::from_timestamp_millis),
            });
        }
        Ok(out)
    }

    async fn claim_due(
        &self,
        now: DateTime<Utc>,
        materialize: &dyn OccurrenceMaterializer,
    ) -> Result<Vec<ClaimedOccurrence>, ScheduleError> {
        // Phase 1 — plan. A short read; no transaction is held across the
        // materialization below.
        let plans = self.plan_due(now).await?;

        // Phase 2 — materialize, with no transaction open. The job store
        // needs a pool connection of its own; holding one here would
        // deadlock a single-connection pool.
        let mut outcomes = Vec::new();
        for plan in plans {
            outcomes.push(materialize_plan(plan, materialize).await);
        }

        // Phase 3 — commit: the occurrence rows and the schedules' cursors
        // move together.
        let claimed = self.commit_outcomes(&outcomes, now).await?;
        Ok(claimed)
    }
}

impl SqliteScheduleStore {
    /// The due schedules and the occurrences each one owes.
    async fn plan_due(&self, now: DateTime<Utc>) -> Result<Vec<SchedulePlan>, ScheduleError> {
        let due_rows = sqlx::query(
            r#"
            SELECT id, workspace_id, session_id, kind_json, job_template_json,
                   state, overlap_policy, missed_run_policy_json,
                   next_run_at, last_occurrence_at,
                   time_created, time_updated, labels_json
            FROM schedule
            WHERE state = 'active' AND next_run_at IS NOT NULL AND next_run_at <= ?
            "#,
        )
        .bind(now.timestamp_millis())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;

        let mut plans = Vec::new();
        for row in due_rows {
            let record = row_to_schedule(&row)?;
            let running = count_running_for_schedule(&*self.job_store, &record.schedule_id).await?;
            let targets = missed_run_targets(
                &record.kind,
                record.last_occurrence_at,
                now,
                &record.missed_run_policy,
            );
            plans.push(SchedulePlan {
                schedule: record,
                targets,
                running,
            });
        }
        Ok(plans)
    }

    /// Record the decided occurrences and advance the schedules' cursors.
    ///
    /// The transaction reads each schedule's stored cursor and then writes
    /// it back, so it opens with `BEGIN IMMEDIATE`: a deferred transaction
    /// would hold a shared read lock across that read and then have to
    /// upgrade it to write — which SQLite refuses with `SQLITE_BUSY`
    /// without waiting.
    async fn commit_outcomes(
        &self,
        outcomes: &[ScheduleOutcome],
        now: DateTime<Utc>,
    ) -> Result<Vec<ClaimedOccurrence>, ScheduleError> {
        let mut tx = ImmediateWrite::begin_with(&self.pool, |e| {
            ScheduleError::Storage(StorageError::Database(e.to_string()))
        })
        .await?;

        let mut claimed = Vec::new();

        for outcome in outcomes {
            let last_decided = outcome.last_decided();
            let first_failed = outcome.first_failed();
            let sid = outcome.schedule_id.clone();

            for (target, decision) in &outcome.decided {
                let mut failure_code: Option<String> = None;
                let (job_id, status) = match decision {
                    OccurrenceOutcome::Queued(job_id) => {
                        (Some(job_id.clone()), OccurrenceStatus::Queued)
                    }
                    OccurrenceOutcome::Skipped => (None, OccurrenceStatus::Skipped),
                    // A refused occurrence is not recorded, so the retry
                    // is not turned into a no-op by the primary key.
                    OccurrenceOutcome::Failed { code } => {
                        failure_code = Some(code.clone());
                        (None, OccurrenceStatus::Failed)
                    }
                };
                if !matches!(decision, OccurrenceOutcome::Failed { .. }) {
                    sqlx::query(
                        r#"
                        INSERT OR IGNORE INTO schedule_occurrence
                            (schedule_id, scheduled_for, job_id, status, time_created)
                        VALUES (?, ?, ?, ?, ?)
                        "#,
                    )
                    .bind(sid.as_str())
                    .bind(target.timestamp())
                    .bind(job_id.as_ref().map(|id| id.as_str()))
                    .bind(occurrence_status_to_str(status))
                    .bind(now.timestamp_millis())
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
                }
                claimed.push(ClaimedOccurrence {
                    schedule_id: sid.clone(),
                    scheduled_for: *target,
                    job_id,
                    status,
                    failure_code,
                });
            }

            // The cursor only ever advances: a concurrent tick that
            // already moved it further is not walked back.
            let stored_row =
                sqlx::query("SELECT last_occurrence_at, state FROM schedule WHERE id = ?")
                    .bind(sid.as_str())
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
            let stored_last = stored_row
                .as_ref()
                .map(|row| row.get::<Option<i64>, _>("last_occurrence_at"))
                .unwrap_or(None)
                .and_then(DateTime::<Utc>::from_timestamp_millis);
            let stored_state = stored_row
                .as_ref()
                .map(|row| row.get::<String, _>("state"))
                .unwrap_or_else(|| state_to_str(ScheduleState::Active).to_string());
            let new_last = match (stored_last, last_decided) {
                (Some(stored), Some(planned)) => Some(stored.max(planned)),
                (None, planned) => planned,
                (stored, None) => stored,
            };

            let new_next = next_horizon(&outcome.kind, now, new_last, first_failed);
            let is_one_shot = matches!(outcome.kind, ScheduleKind::OneShot { .. });
            let new_state = if is_one_shot && new_next.is_none() {
                "completed"
            } else {
                stored_state.as_str()
            };
            sqlx::query(
                r#"
                UPDATE schedule SET
                    last_occurrence_at = ?,
                    next_run_at = ?,
                    state = ?,
                    time_updated = ?
                WHERE id = ?
                "#,
            )
            .bind(new_last.map(|t| t.timestamp_millis()))
            .bind(new_next.map(|t| t.timestamp_millis()))
            .bind(new_state)
            .bind(now.timestamp_millis())
            .bind(sid.as_str())
            .execute(&mut **tx)
            .await
            .map_err(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))?;
        }

        tx.commit_with(|e| ScheduleError::Storage(StorageError::Database(e.to_string())))
            .await?;

        Ok(claimed)
    }
}

fn row_to_schedule(row: &sqlx::sqlite::SqliteRow) -> Result<ScheduleRecord, ScheduleError> {
    let id: String = row.get("id");
    let workspace_id: String = row.get("workspace_id");
    let session_id: Option<String> = row.get("session_id");
    let kind_json: String = row.get("kind_json");
    let job_template_json: String = row.get("job_template_json");
    let state: String = row.get("state");
    let overlap_policy: String = row.get("overlap_policy");
    let missed_run_policy_json: String = row.get("missed_run_policy_json");
    let next_run_at: Option<i64> = row.get("next_run_at");
    let last_occurrence_at: Option<i64> = row.get("last_occurrence_at");
    let time_created: i64 = row.get("time_created");
    let time_updated: i64 = row.get("time_updated");
    let labels_json: String = row
        .try_get::<String, _>("labels_json")
        .unwrap_or_else(|_| "{}".to_string());
    Ok(ScheduleRecord {
        schedule_id: ScheduleId::new_unchecked(id),
        workspace_id: WorkspaceId::new_unchecked(workspace_id),
        session_id,
        kind: deserialize_kind(&kind_json)?,
        job_template: deserialize_template(&job_template_json)?,
        state: ScheduleState::from_str_lossy(&state),
        overlap_policy: overlap_from_str(&overlap_policy),
        missed_run_policy: deserialize_missed_run(&missed_run_policy_json)?,
        next_run_at: next_run_at.and_then(DateTime::<Utc>::from_timestamp_millis),
        last_occurrence_at: last_occurrence_at.and_then(DateTime::<Utc>::from_timestamp_millis),
        created_at: DateTime::<Utc>::from_timestamp_millis(time_created).unwrap_or_else(Utc::now),
        updated_at: DateTime::<Utc>::from_timestamp_millis(time_updated).unwrap_or_else(Utc::now),
        labels: deserialize_labels(&labels_json),
    })
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{
        DaemonGeneration, ExecutionTarget, IdempotencyClass, InMemoryJobStore, JobKind,
        JobPriority, JobSource, NewJob, ResourceRequest, RetryPolicy,
    };

    fn ws() -> WorkspaceId {
        WorkspaceId::new_unchecked("test-ws")
    }

    fn test_template(agent: &str) -> JobTemplate {
        JobTemplate::for_subagent(
            JobKind::Subagent,
            "test prompt".to_string(),
            agent.to_string(),
            None,
        )
    }

    fn test_materializer() -> impl OccurrenceMaterializer {
        struct Stub;
        #[async_trait]
        impl OccurrenceMaterializer for Stub {
            async fn materialize(
                &self,
                _schedule: &ScheduleRecord,
                _at: DateTime<Utc>,
            ) -> Result<JobId, super::super::schedule::MaterializerError> {
                Ok(JobId::new_unchecked(uuid::Uuid::new_v4().to_string()))
            }
        }
        Stub
    }

    /// A materializer that really creates the job, so a test can assert
    /// on the durable job spec the template produced.
    fn creating_materializer(job_store: Arc<dyn JobStore>) -> impl OccurrenceMaterializer {
        struct Creator {
            job_store: Arc<dyn JobStore>,
        }
        #[async_trait]
        impl OccurrenceMaterializer for Creator {
            async fn materialize(
                &self,
                schedule: &ScheduleRecord,
                at: DateTime<Utc>,
            ) -> Result<JobId, super::super::schedule::MaterializerError> {
                let spec = schedule.job_template.new_job_for_occurrence(
                    schedule.workspace_id.clone(),
                    schedule.schedule_id.clone(),
                    schedule.session_id.clone(),
                    at,
                );
                let job = self.job_store.create_job(spec).await?;
                Ok(job.job_id)
            }
        }
        Creator { job_store }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn in_memory_create_and_get() {
        let store = InMemoryScheduleStore::new(Arc::new(InMemoryJobStore::new()));
        let tmpl = ScheduleTemplate {
            workspace_id: ws(),
            session_id: None,
            kind: ScheduleKind::Interval {
                every: std::time::Duration::from_secs(60),
                anchor: Utc::now(),
            },
            job_template: test_template("build"),
            overlap_policy: OverlapPolicy::SkipIfRunning,
            missed_run_policy: MissedRunPolicy::RunOnceNow,
            next_run_at: None,
            labels: HashMap::new(),
        };
        let rec = store.create(tmpl).await.unwrap();
        assert_eq!(rec.state, ScheduleState::Active);
        assert!(rec.next_run_at.is_some());

        let fetched = store.get(&rec.schedule_id).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().schedule_id, rec.schedule_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn in_memory_lifecycle_paused_archived() {
        let store = InMemoryScheduleStore::new(Arc::new(InMemoryJobStore::new()));
        let rec = store
            .create(ScheduleTemplate {
                workspace_id: ws(),
                session_id: None,
                kind: ScheduleKind::OneShot {
                    run_at: Utc::now() + chrono::Duration::hours(1),
                },
                job_template: test_template("build"),
                overlap_policy: OverlapPolicy::SkipIfRunning,
                missed_run_policy: MissedRunPolicy::RunOnceNow,
                next_run_at: None,
                labels: HashMap::new(),
            })
            .await
            .unwrap();

        let paused = store
            .set_state(&rec.schedule_id, ScheduleState::Paused)
            .await
            .unwrap();
        assert_eq!(paused.state, ScheduleState::Paused);

        let archived = store
            .set_state(&rec.schedule_id, ScheduleState::Archived)
            .await
            .unwrap();
        assert!(archived.state.is_terminal());

        let err = store
            .set_state(&archived.schedule_id, ScheduleState::Active)
            .await;
        assert!(err.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn in_memory_overlap_skip_if_running() {
        let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let store = InMemoryScheduleStore::new(job_store.clone());

        let rec = store
            .create(ScheduleTemplate {
                workspace_id: ws(),
                session_id: None,
                kind: ScheduleKind::OneShot {
                    run_at: Utc::now() - chrono::Duration::seconds(10),
                },
                job_template: test_template("build"),
                overlap_policy: OverlapPolicy::SkipIfRunning,
                missed_run_policy: MissedRunPolicy::RunOnceNow,
                next_run_at: None,
                labels: HashMap::new(),
            })
            .await
            .unwrap();

        let gen = DaemonGeneration::new();
        let job = job_store
            .create_job(NewJob {
                workspace_id: ws(),
                session_id: None,
                turn_id: None,
                kind: JobKind::Subagent,
                source: JobSource::Scheduled {
                    schedule_id: rec.schedule_id.clone(),
                    occurrence: Utc::now(),
                },
                priority: JobPriority::Normal,
                payload: test_template("build").payload.clone(),
                resource_request: ResourceRequest::default(),
                timeout: None,
                retry_policy: RetryPolicy::no_retry(),
                idempotency: IdempotencyClass::SafeRepeat,
                not_before: None,
                deadline: None,
                schedule_id: Some(rec.schedule_id.clone()),
                depends_on: vec![],
                parent_job_id: None,
                parent_attempt_id: None,
                parent_call_id: None,
                parent_program_id: None,
                parent_instruction_sequence: None,
                relation_kind: None,
                target: ExecutionTarget::default(),
            })
            .await
            .unwrap();
        let _attempt = job_store.begin_attempt(&job.job_id, &gen).await.unwrap();

        let claimed = store
            .claim_due(Utc::now(), &test_materializer())
            .await
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].status, OccurrenceStatus::Skipped);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn in_memory_missed_run_bounded_catchup() {
        let store = InMemoryScheduleStore::new(Arc::new(InMemoryJobStore::new()));
        let anchor = Utc::now() - chrono::Duration::seconds(300);
        let _rec = store
            .create(ScheduleTemplate {
                workspace_id: ws(),
                session_id: None,
                kind: ScheduleKind::Interval {
                    every: std::time::Duration::from_secs(60),
                    anchor,
                },
                job_template: test_template("build"),
                overlap_policy: OverlapPolicy::SkipIfRunning,
                missed_run_policy: MissedRunPolicy::CatchUpBounded { max_occurrences: 2 },
                next_run_at: Some(anchor),
                labels: HashMap::new(),
            })
            .await
            .unwrap();

        let claimed = store
            .claim_due(Utc::now(), &test_materializer())
            .await
            .unwrap();
        assert!(claimed.len() <= 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn in_memory_occurrence_uniqueness() {
        let store = InMemoryScheduleStore::new(Arc::new(InMemoryJobStore::new()));
        let now = Utc::now();
        let _rec = store
            .create(ScheduleTemplate {
                workspace_id: ws(),
                session_id: None,
                kind: ScheduleKind::OneShot { run_at: now },
                job_template: test_template("build"),
                overlap_policy: OverlapPolicy::SkipIfRunning,
                missed_run_policy: MissedRunPolicy::RunOnceNow,
                next_run_at: Some(now),
                labels: HashMap::new(),
            })
            .await
            .unwrap();

        let c1 = store.claim_due(now, &test_materializer()).await.unwrap();
        let c2 = store.claim_due(now, &test_materializer()).await.unwrap();
        assert!(!c1.is_empty());
        assert!(c2.is_empty(), "one-shot should be exhausted");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn in_memory_one_shot_exhaustion() {
        let store = InMemoryScheduleStore::new(Arc::new(InMemoryJobStore::new()));
        let past = Utc::now() - chrono::Duration::seconds(10);
        let _rec = store
            .create(ScheduleTemplate {
                workspace_id: ws(),
                session_id: None,
                kind: ScheduleKind::OneShot { run_at: past },
                job_template: test_template("build"),
                overlap_policy: OverlapPolicy::Allow,
                missed_run_policy: MissedRunPolicy::RunOnceNow,
                next_run_at: None,
                labels: HashMap::new(),
            })
            .await
            .unwrap();

        let c1 = store
            .claim_due(Utc::now(), &test_materializer())
            .await
            .unwrap();
        assert_eq!(c1.len(), 1);
        let c2 = store
            .claim_due(Utc::now(), &test_materializer())
            .await
            .unwrap();
        assert!(c2.is_empty());
    }

    /// M001: a template that targets a remote node materializes a job
    /// with the same target. The template is the only place the target
    /// lives, so the materialization path must carry it unchanged.
    #[tokio::test(flavor = "current_thread")]
    async fn in_memory_materialization_preserves_remote_target() {
        let job_store = Arc::new(InMemoryJobStore::new());
        let store = InMemoryScheduleStore::new(job_store.clone());
        let now = Utc::now();

        let mut template = test_template("build");
        template.target = ExecutionTarget::EggworkNode {
            node_id: "builder".to_owned(),
        };
        let record = store
            .create(ScheduleTemplate {
                workspace_id: ws(),
                session_id: None,
                kind: ScheduleKind::OneShot { run_at: now },
                job_template: template,
                overlap_policy: OverlapPolicy::Allow,
                missed_run_policy: MissedRunPolicy::RunOnceNow,
                next_run_at: Some(now),
                labels: HashMap::new(),
            })
            .await
            .unwrap();

        let claimed = store
            .claim_due(now, &creating_materializer(job_store.clone()))
            .await
            .unwrap();
        assert_eq!(claimed.len(), 1);
        let job_id = claimed[0]
            .job_id
            .clone()
            .expect("a materialized occurrence owns a job");
        let job = job_store
            .get_job(&job_id)
            .await
            .unwrap()
            .expect("job exists");
        assert_eq!(
            job.target,
            ExecutionTarget::EggworkNode {
                node_id: "builder".to_owned()
            }
        );
        assert_eq!(job.schedule_id.as_ref(), Some(&record.schedule_id));
    }

    /// M001: a template persisted before explicit targeting existed has
    /// no `target` key and must decode as the local executor.
    #[test]
    fn template_without_target_key_decodes_as_local() {
        // A real persisted template with the `target` key removed is
        // exactly what a pre-M001 schedule row holds.
        let mut encoded = serde_json::to_value(test_template("build")).expect("template encodes");
        let object = encoded.as_object_mut().expect("template is an object");
        assert!(
            object.remove("target").is_some(),
            "the template carries a target"
        );
        let template: JobTemplate =
            serde_json::from_value(encoded).expect("legacy template decodes");
        assert_eq!(template.target, ExecutionTarget::Local);
        assert_eq!(template.target_node_id(), None);
    }

    /// M001: a template that names a remote node round-trips through the
    /// persisted JSON form, so a restarted daemon keeps the target.
    #[test]
    fn template_with_remote_target_round_trips() {
        let mut template = test_template("build");
        template.target = ExecutionTarget::EggworkNode {
            node_id: "builder".to_owned(),
        };
        let encoded = serde_json::to_value(&template).expect("template encodes");
        let decoded: JobTemplate = serde_json::from_value(encoded).expect("template decodes");
        assert_eq!(decoded.target, template.target);
        assert_eq!(decoded.target_node_id(), Some("builder"));
    }

    /// M001: a one-shot schedule that targets a remote node materializes
    /// exactly one job, and that job keeps the remote target. The
    /// occurrence key `(schedule_id, scheduled_for)` is what keeps a
    /// replay from producing a second job.
    #[tokio::test(flavor = "current_thread")]
    async fn sqlite_one_shot_schedule_materializes_one_remote_job() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory db");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        let job_store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let store = SqliteScheduleStore::new(pool, job_store.clone());
        let now = Utc::now();

        let mut template = test_template("build");
        template.target = ExecutionTarget::EggworkNode {
            node_id: "builder".to_owned(),
        };
        store
            .create(ScheduleTemplate {
                workspace_id: ws(),
                session_id: None,
                kind: ScheduleKind::OneShot { run_at: now },
                job_template: template,
                overlap_policy: OverlapPolicy::Allow,
                missed_run_policy: MissedRunPolicy::RunOnceNow,
                next_run_at: Some(now),
                labels: HashMap::new(),
            })
            .await
            .unwrap();

        let claimed = store
            .claim_due(now, &creating_materializer(job_store.clone()))
            .await
            .unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].status, OccurrenceStatus::Queued);

        let job = job_store
            .get_job(
                claimed[0]
                    .job_id
                    .as_ref()
                    .expect("a materialized occurrence owns a job"),
            )
            .await
            .unwrap()
            .expect("materialized job exists");
        assert_eq!(
            job.target,
            ExecutionTarget::EggworkNode {
                node_id: "builder".to_owned()
            }
        );

        // A replay of the same tick does not duplicate the occurrence.
        let replay = store
            .claim_due(now, &creating_materializer(job_store.clone()))
            .await
            .unwrap();
        assert!(
            replay.is_empty(),
            "the one-shot schedule does not fire twice"
        );
    }
}
