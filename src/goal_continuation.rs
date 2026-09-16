//! Application-layer assembly of host-observed Goal continuation evidence.
//!
//! This module reads durable owners and converts their records into the narrow
//! [`codegg_core::goal::progress::GoalContinuationEvidence`] value consumed by
//! the deterministic continuation assessment. It never executes tools, never
//! polls free-form model text, and never treats a model-named job id as a
//! live handle.
//!
//! Evidence sources (all host-owned):
//!
//! - `Goal` row: id, revision, status string, open-question count;
//! - in-memory `TodoState`: revision plus per-status counts (no content text);
//! - durable `Test` / `Subagent` jobs carrying this goal's provenance label,
//!   mapped to `Passed` / `Failed` / `InProgress`.
//!
//! A fake job named only in model prose never appears in the job store query
//! and therefore can never produce `VerifiedWait`.

use codegg_core::goal::progress::GoalContinuationEvidence;
use codegg_core::goal::{GoalExecutionEvidence, HostEvidenceStatus};
use codegg_core::jobs::store::JobStoreQuery;
use codegg_core::jobs::{JobKind, JobState, JobStore, SqliteJobStore, GOAL_PROVENANCE_LABEL_KEY};
use sqlx::SqlitePool;

const MAX_EVIDENCE_RECORDS: u32 = 128;

/// Assemble continuation evidence for the given active goal.
///
/// `todo` is the loop's current in-memory state (already loaded from durable
/// storage at turn start and updated by validated todo writes during the
/// turn). Only its revision and per-status counts enter the evidence; item
/// content text is excluded so prose edits cannot manufacture progress.
pub async fn assemble_continuation_evidence(
    pool: &SqlitePool,
    session_id: &str,
    goal: &codegg_core::goal::Goal,
    todo: &codegg_core::task_state::TodoState,
) -> Result<GoalContinuationEvidence, String> {
    let job_store = SqliteJobStore::new(pool.clone());
    let jobs = job_store
        .list_job_records(JobStoreQuery {
            kinds: vec![JobKind::Test, JobKind::Subagent],
            session_id: Some(session_id.to_string()),
            limit: Some(MAX_EVIDENCE_RECORDS),
            ..Default::default()
        })
        .await
        .map_err(|error| {
            format!(
                "goal continuation job evidence unavailable: {}",
                truncate_bounded(error.to_string(), 200)
            )
        })?;

    let mut executions = Vec::new();
    for job in jobs {
        if job.created_at < goal.created_at {
            continue;
        }
        if job
            .labels
            .get(GOAL_PROVENANCE_LABEL_KEY)
            .map(String::as_str)
            != Some(goal.id.as_str())
        {
            continue;
        }
        let source = match job.kind {
            JobKind::Test => "test",
            JobKind::Subagent => "delegated_run",
            _ => continue,
        };
        executions.push(GoalExecutionEvidence {
            id: truncate_bounded(job.job_id.as_str().to_string(), 128),
            source: source.to_string(),
            status: host_status(job.state),
        });
        if executions.len() >= MAX_EVIDENCE_RECORDS as usize {
            break;
        }
    }

    let mut pending = 0u32;
    let mut in_progress = 0u32;
    let mut blocked = 0u32;
    let mut completed = 0u32;
    let mut cancelled = 0u32;
    for item in &todo.items {
        match item.status {
            codegg_core::task_state::TodoStatus::Pending => pending += 1,
            codegg_core::task_state::TodoStatus::InProgress => in_progress += 1,
            codegg_core::task_state::TodoStatus::Blocked => blocked += 1,
            codegg_core::task_state::TodoStatus::Completed => completed += 1,
            codegg_core::task_state::TodoStatus::Cancelled => cancelled += 1,
        }
    }

    Ok(GoalContinuationEvidence {
        goal_id: truncate_bounded(goal.id.clone(), 128),
        goal_revision: goal.revision,
        goal_status: truncate_bounded(goal.status_as_str().to_string(), 32),
        todo_revision: todo.revision,
        todo_pending: pending,
        todo_in_progress: in_progress,
        todo_blocked: blocked,
        todo_completed: completed,
        todo_cancelled: cancelled,
        open_questions: goal.open_questions.len().min(1024) as u32,
        executions,
    })
}

fn host_status(state: JobState) -> HostEvidenceStatus {
    match state {
        JobState::Completed => HostEvidenceStatus::Passed,
        JobState::Failed
        | JobState::Cancelled
        | JobState::TimedOut
        | JobState::Interrupted
        | JobState::Expired => HostEvidenceStatus::Failed,
        JobState::Scheduled | JobState::Queued | JobState::Running | JobState::Blocked => {
            HostEvidenceStatus::InProgress
        }
    }
}

fn truncate_bounded(value: String, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::task_state::{TodoItem, TodoPriority, TodoStatus};

    fn todo_state_with(statuses: Vec<TodoStatus>) -> codegg_core::task_state::TodoState {
        let mut state = codegg_core::task_state::TodoState::new();
        state.items = statuses
            .into_iter()
            .enumerate()
            .map(|(i, status)| TodoItem {
                id: format!("item-{i}"),
                content: format!("content {i}"),
                status,
                priority: TodoPriority::Medium,
                blocker: None,
            })
            .collect();
        state.revision = 7;
        state
    }

    fn test_goal() -> codegg_core::goal::Goal {
        codegg_core::goal::Goal {
            id: "goal-1".into(),
            revision: 3,
            session_id: "sess-1".into(),
            project_id: "/tmp".into(),
            title: "Goal".into(),
            objective: "Do work".into(),
            status: codegg_core::goal::GoalStatus::Active,
            plan_path: None,
            checkpoint_path: None,
            current_phase: None,
            progress_summary: "model prose that must not leak".into(),
            next_action: None,
            completion_criteria: Vec::new(),
            open_questions: vec!["blocker?".into()],
            budget: Default::default(),
            usage: Default::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    async fn test_pool() -> SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:goal_continuation_test_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect in-memory sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    #[tokio::test(flavor = "current_thread")]
    async fn assembles_counts_without_content_text() {
        let pool = test_pool().await;
        let goal = test_goal();
        let todo = todo_state_with(vec![TodoStatus::Pending, TodoStatus::Completed]);
        let evidence = assemble_continuation_evidence(&pool, "sess-1", &goal, &todo)
            .await
            .unwrap();
        assert_eq!(evidence.todo_pending, 1);
        assert_eq!(evidence.todo_completed, 1);
        assert_eq!(evidence.todo_revision, 7);
        assert_eq!(evidence.goal_revision, 3);
        assert_eq!(evidence.open_questions, 1);
        // Progress summary prose is excluded by construction.
        let fingerprint = codegg_core::goal::progress::goal_continuation_fingerprint(&evidence);
        assert!(!fingerprint.contains("prose"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn model_named_job_never_becomes_wait_handle() {
        let pool = test_pool().await;
        let goal = test_goal();
        let todo = todo_state_with(vec![]);
        let evidence = assemble_continuation_evidence(&pool, "sess-1", &goal, &todo)
            .await
            .unwrap();
        // No durable job exists, so even though the model could name
        // "job-1" in prose, no live handle is assembled.
        assert!(evidence.executions.is_empty());
        assert!(codegg_core::goal::progress::live_wait_handle(&evidence).is_none());
    }
}
