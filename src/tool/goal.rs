use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::error::ToolError;
use crate::goal::model::*;
use crate::goal::store::GoalStore;
use crate::tool::Tool;
use codegg_core::goal::{GoalCompletionProposal, GoalVerificationService, GoalVerificationVerdict};

pub struct GoalGetTool {
    pool: SqlitePool,
    session_id: String,
}

impl GoalGetTool {
    pub fn new(pool: SqlitePool, session_id: String) -> Self {
        Self { pool, session_id }
    }
}

#[async_trait]
impl Tool for GoalGetTool {
    fn name(&self) -> &str {
        "goal_get"
    }

    fn description(&self) -> &str {
        "Get the current active goal for this session"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        let store = GoalStore::new(self.pool.clone());
        match store.active_for_session(&self.session_id).await {
            Ok(Some(goal)) => {
                let checkpoint_excerpt = if let Some(ref path) = goal.checkpoint_path {
                    crate::goal::checkpoint::read_checkpoint_excerpt(path, 4000)
                        .await
                        .ok()
                        .flatten()
                } else {
                    None
                };
                Ok(serde_json::json!({
                    "active": true,
                    "goal": {
                        "id": goal.id,
                        "title": goal.title,
                        "objective": goal.objective,
                        "status": goal.status_as_str(),
                        "current_phase": goal.current_phase,
                        "progress_summary": goal.progress_summary,
                        "next_action": goal.next_action,
                    },
                    "checkpoint_excerpt": checkpoint_excerpt,
                })
                .to_string())
            }
            Ok(None) => Ok(serde_json::json!({ "active": false }).to_string()),
            Err(e) => Err(ToolError::Execution(e.to_string())),
        }
    }
}

pub struct GoalUpdateProgressTool {
    pool: SqlitePool,
    session_id: String,
}

impl GoalUpdateProgressTool {
    pub fn new(pool: SqlitePool, session_id: String) -> Self {
        Self { pool, session_id }
    }
}

#[async_trait]
impl Tool for GoalUpdateProgressTool {
    fn name(&self) -> &str {
        "goal_update_progress"
    }

    fn description(&self) -> &str {
        "Update progress on the current active goal"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "current_phase": { "type": "string" },
                "progress_summary": { "type": "string" },
                "next_action": { "type": "string" },
                "completed_items": { "type": "array", "items": { "type": "string" } },
                "remaining_items": { "type": "array", "items": { "type": "string" } },
                "open_questions": { "type": "array", "items": { "type": "string" } }
            }
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let store = GoalStore::new(self.pool.clone());
        let goal = store
            .active_for_session(&self.session_id)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?
            .ok_or_else(|| ToolError::Execution("No active goal".to_string()))?;

        let update = GoalProgressUpdate {
            current_phase: input
                .get("current_phase")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            progress_summary: input
                .get("progress_summary")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            next_action: input
                .get("next_action")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            completed_items: input
                .get("completed_items")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            remaining_items: input
                .get("remaining_items")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            open_questions: input
                .get("open_questions")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        };

        let updated_goal = store
            .update_progress(&goal.id, update.clone())
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?
            .ok_or_else(|| ToolError::Execution("Goal not found after update".to_string()))?;

        if let Some(ref cp_path) = updated_goal.checkpoint_path {
            if let Err(e) =
                crate::goal::checkpoint::append_checkpoint_update(cp_path, &update).await
            {
                tracing::warn!(error = %e, goal_id = %goal.id, "failed to append checkpoint update");
            }
        }

        crate::bus::global::GlobalEventBus::publish(crate::bus::events::AppEvent::GoalUpdated {
            session_id: self.session_id.clone(),
            goal: Box::new(Some(updated_goal.to_snapshot())),
        });

        Ok(serde_json::json!({
            "id": updated_goal.id,
            "title": updated_goal.title,
            "status": updated_goal.status_as_str(),
            "current_phase": updated_goal.current_phase,
            "progress_summary": updated_goal.progress_summary,
            "next_action": updated_goal.next_action,
        })
        .to_string())
    }
}

pub struct GoalRequestCompletionTool {
    pool: SqlitePool,
    session_id: String,
}

impl GoalRequestCompletionTool {
    pub fn new(pool: SqlitePool, session_id: String) -> Self {
        Self { pool, session_id }
    }
}

#[async_trait]
impl Tool for GoalRequestCompletionTool {
    fn name(&self) -> &str {
        "goal_request_completion"
    }

    fn description(&self) -> &str {
        "Request host verification and completion of the current active goal"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["evidence"],
            "properties": {
                "evidence": { "type": "string" },
                "files_changed": { "type": "array", "items": { "type": "string" } },
                "tests_run": { "type": "array", "items": { "type": "string" } },
                "remaining_risks": { "type": "array", "items": { "type": "string" } }
            }
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let evidence = input.get("evidence").and_then(|v| v.as_str()).unwrap_or("");
        let tests_run: Vec<String> = input
            .get("tests_run")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let remaining_risks: Vec<String> = input
            .get("remaining_risks")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let files_changed: Vec<String> = input
            .get("files_changed")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        if evidence.trim().is_empty() {
            return Ok(serde_json::json!({
                "accepted": false,
                "verdict": "not_met",
                "reason": "Evidence is required to request completion"
            })
            .to_string());
        }

        let store = GoalStore::new(self.pool.clone());
        let goal = store
            .active_for_session(&self.session_id)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?
            .ok_or_else(|| ToolError::Execution("No active goal".to_string()))?;

        let proposal = match GoalCompletionProposal::from_request(CompletionRequest {
            evidence: evidence.to_string(),
            files_changed,
            tests_run,
            remaining_risks,
        }) {
            Ok(proposal) => proposal,
            Err(reason) => {
                return Ok(serde_json::json!({
                    "accepted": false,
                    "verdict": "not_met",
                    "reason": reason,
                })
                .to_string())
            }
        };

        let host_evidence = crate::goal_verification::assemble(
            &self.pool,
            &self.session_id,
            &goal.id,
            goal.created_at,
        )
        .await
        .map_err(ToolError::Execution)?;
        let verdict = GoalVerificationService.verify(&goal, &proposal, &host_evidence);

        match verdict {
            GoalVerificationVerdict::Met { summary } => {
                let Some(updated_goal) = store
                    .complete_if_active(&goal.id, goal.revision)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?
                else {
                    return Ok(serde_json::json!({
                        "accepted": false,
                        "verdict": "stale",
                        "goal_id": goal.id,
                        "reason": "goal changed while completion was being verified",
                    })
                    .to_string());
                };

                let update = GoalProgressUpdate {
                    current_phase: Some("Complete".to_string()),
                    progress_summary: Some(summary.clone()),
                    next_action: None,
                    completed_items: vec![],
                    remaining_items: vec![],
                    open_questions: vec![],
                };
                if let Some(ref cp_path) = updated_goal.checkpoint_path {
                    let _ =
                        crate::goal::checkpoint::append_checkpoint_update(cp_path, &update).await;
                }
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::GoalUpdated {
                        session_id: self.session_id.clone(),
                        goal: Box::new(Some(updated_goal.to_snapshot())),
                    },
                );
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::GoalCompleted {
                        session_id: self.session_id.clone(),
                        goal_id: goal.id.clone(),
                        evidence: summary.clone(),
                    },
                );
                Ok(serde_json::json!({
                    "accepted": true,
                    "verdict": "met",
                    "goal_id": goal.id,
                    "status": "complete",
                    "summary": summary,
                })
                .to_string())
            }
            GoalVerificationVerdict::NotMet {
                unmet_criteria,
                evidence_gaps,
                next_action,
            } => {
                let mut open_questions = unmet_criteria;
                open_questions.extend(evidence_gaps);
                open_questions.truncate(codegg_core::goal::verification::MAX_VERDICT_ITEMS);
                let progress_summary =
                    format!("Host verification not met: {}", open_questions.join("; "));
                let updated_goal = store
                    .update_progress_if_revision(
                        &goal.id,
                        goal.revision,
                        GoalProgressUpdate {
                            current_phase: Some("Completion verification".to_string()),
                            progress_summary: Some(progress_summary),
                            next_action: Some(next_action.clone()),
                            completed_items: vec![],
                            remaining_items: vec![],
                            open_questions,
                        },
                    )
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                publish_goal_update(&self.session_id, updated_goal.as_ref());
                Ok(serde_json::json!({
                    "accepted": false,
                    "verdict": "not_met",
                    "goal_id": goal.id,
                    "status": updated_goal.as_ref().map(|g| g.status_as_str()),
                    "next_action": next_action,
                })
                .to_string())
            }
            GoalVerificationVerdict::AwaitingUser { reason } => {
                let updated_goal = store
                    .update_status_if_revision(&goal.id, goal.revision, GoalStatus::AwaitingUser)
                    .await
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                publish_goal_update(&self.session_id, updated_goal.as_ref());
                Ok(serde_json::json!({
                    "accepted": false,
                    "verdict": "awaiting_user",
                    "goal_id": goal.id,
                    "status": updated_goal.as_ref().map(|g| g.status_as_str()),
                    "reason": reason,
                })
                .to_string())
            }
        }
    }
}

fn publish_goal_update(session_id: &str, goal: Option<&Goal>) {
    crate::bus::global::GlobalEventBus::publish(crate::bus::events::AppEvent::GoalUpdated {
        session_id: session_id.to_string(),
        goal: Box::new(goal.map(Goal::to_snapshot)),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{
        AttemptCompletion, AttemptState, DaemonGeneration, IdempotencyClass, JobPayload,
        JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy, SqliteJobStore,
    };
    use codegg_core::workspace::WorkspaceId;

    async fn test_pool() -> SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:tool_goal_test_{}?mode=memory&cache=shared",
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

    async fn ensure_test_session(pool: &SqlitePool, session_id: &str, project_id: &str) {
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, ?, '[]', ?, ?)",
        )
        .bind(project_id)
        .bind("/tmp/test")
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, 'test', '/tmp/test', 'Test', '1', ?, ?)",
        )
        .bind(session_id)
        .bind(project_id)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_test_job(pool: &SqlitePool, session_id: &str, terminal_state: AttemptState) {
        let goal_id = GoalStore::new(pool.clone())
            .active_for_session(session_id)
            .await
            .unwrap()
            .map(|goal| goal.id);
        insert_test_job_for_goal(pool, session_id, terminal_state, goal_id.as_deref()).await;
    }

    async fn insert_test_job_for_goal(
        pool: &SqlitePool,
        session_id: &str,
        terminal_state: AttemptState,
        goal_id: Option<&str>,
    ) {
        let jobs = SqliteJobStore::new(pool.clone());
        let job = jobs
            .create_job(NewJob {
                workspace_id: WorkspaceId::new_unchecked("/tmp/test"),
                session_id: Some(session_id.to_string()),
                turn_id: None,
                kind: codegg_core::jobs::JobKind::Test,
                source: JobSource::Interactive,
                priority: JobPriority::Interactive,
                payload: JobPayload::Test {
                    command: "cargo test".into(),
                    argv: vec!["cargo".into(), "test".into()],
                    cwd: Some("/tmp/test".into()),
                    scope: Some("workspace".into()),
                },
                resource_request: ResourceRequest::for_kind(codegg_core::jobs::JobKind::Test),
                timeout: None,
                retry_policy: RetryPolicy::no_retry(),
                idempotency: IdempotencyClass::SafeRepeat,
                not_before: None,
                deadline: None,
                schedule_id: None,
                depends_on: Vec::new(),
                parent_job_id: None,
                parent_attempt_id: None,
                parent_call_id: None,
                parent_program_id: None,
                parent_instruction_sequence: None,
                relation_kind: None,
            })
            .await
            .unwrap();
        if let Some(goal_id) = goal_id {
            let mut labels = std::collections::HashMap::new();
            labels.insert(
                codegg_core::jobs::GOAL_PROVENANCE_LABEL_KEY.to_string(),
                goal_id.to_string(),
            );
            jobs.set_job_labels(&job.job_id, labels).await.unwrap();
        }
        let attempt = jobs
            .begin_attempt(
                &job.job_id,
                &DaemonGeneration::new_unchecked("test-generation"),
            )
            .await
            .unwrap();
        jobs.mark_attempt_running(&attempt.attempt_id)
            .await
            .unwrap();
        jobs.finish_attempt(AttemptCompletion {
            attempt_id: attempt.attempt_id,
            state: terminal_state,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();
    }

    async fn insert_completed_test_job(pool: &SqlitePool, session_id: &str) {
        insert_test_job(pool, session_id, AttemptState::Completed).await;
    }

    async fn insert_failed_test_job(pool: &SqlitePool, session_id: &str) {
        insert_test_job(pool, session_id, AttemptState::Failed).await;
    }

    #[tokio::test]
    async fn test_goal_get_no_active_goal() {
        let pool = test_pool().await;
        let tool = GoalGetTool::new(pool, "session1".to_string());
        let result = tool.execute(serde_json::json!({})).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["active"], false);
    }

    #[tokio::test]
    async fn test_goal_get_with_active_goal() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "session1", "/tmp/test").await;
        let store = GoalStore::new(pool.clone());
        store
            .create_active(
                "session1",
                "/tmp/test",
                "Test Goal",
                "Do something",
                None,
                None,
                vec!["Criteria 1".to_string()],
            )
            .await
            .unwrap();
        let tool = GoalGetTool::new(pool, "session1".to_string());
        let result = tool.execute(serde_json::json!({})).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["active"], true);
        assert_eq!(parsed["goal"]["title"], "Test Goal");
    }

    #[tokio::test]
    async fn test_goal_update_progress_no_goal() {
        let pool = test_pool().await;
        let tool = GoalUpdateProgressTool::new(pool, "session1".to_string());
        let result = tool
            .execute(serde_json::json!({ "current_phase": "Phase 1" }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_goal_request_completion_empty_evidence() {
        let pool = test_pool().await;
        let tool = GoalRequestCompletionTool::new(pool, "session1".to_string());
        let result = tool
            .execute(serde_json::json!({ "evidence": "" }))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["accepted"], false);
    }

    #[tokio::test]
    async fn test_goal_request_completion_rejects_no_tests() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "session1", "/tmp/test").await;
        let store = GoalStore::new(pool.clone());
        store
            .create_active(
                "session1",
                "/tmp/test",
                "Test Goal",
                "Do something",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();
        let tool = GoalRequestCompletionTool::new(pool, "session1".to_string());
        let result = tool
            .execute(serde_json::json!({
                "evidence": "I did the work",
                "tests_run": [],
                "remaining_risks": []
            }))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["accepted"], false);
    }

    #[tokio::test]
    async fn test_goal_request_completion_accepts_with_tests() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "session1", "/tmp/test").await;
        let store = GoalStore::new(pool.clone());
        store
            .create_active(
                "session1",
                "/tmp/test",
                "Test Goal",
                "Do something",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();
        insert_completed_test_job(&pool, "session1").await;
        let tool = GoalRequestCompletionTool::new(pool, "session1".to_string());
        let result = tool
            .execute(serde_json::json!({
                "evidence": "I did the work",
                "tests_run": ["cargo test"],
                "files_changed": ["src/foo.rs"]
            }))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["accepted"], true);
        assert_eq!(parsed["status"], "complete");
    }

    #[tokio::test]
    async fn test_goal_request_completion_failed_host_test_is_not_met() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "session1", "/tmp/test").await;
        let store = GoalStore::new(pool.clone());
        store
            .create_active(
                "session1",
                "/tmp/test",
                "Test Goal",
                "Do something",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();
        insert_failed_test_job(&pool, "session1").await;
        let tool = GoalRequestCompletionTool::new(pool.clone(), "session1".to_string());
        let result = tool
            .execute(serde_json::json!({
                "evidence": "the model says it passed",
                "tests_run": ["cargo test"]
            }))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["accepted"], false);
        assert_eq!(parsed["verdict"], "not_met");
        assert_eq!(
            store
                .active_for_session("session1")
                .await
                .unwrap()
                .unwrap()
                .status,
            GoalStatus::Active
        );
    }

    #[tokio::test]
    async fn test_goal_request_completion_unfinished_todo_is_not_met() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "session1", "/tmp/test").await;
        let store = GoalStore::new(pool.clone());
        store
            .create_active(
                "session1",
                "/tmp/test",
                "Test Goal",
                "Do something",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();
        insert_completed_test_job(&pool, "session1").await;
        codegg_core::session::store::TodoStore::new(pool.clone())
            .set(
                "session1",
                vec![codegg_core::session::models::TodoItemInput {
                    content: "unfinished work".into(),
                    status: "pending".into(),
                    priority: "high".into(),
                }],
            )
            .await
            .unwrap();
        let tool = GoalRequestCompletionTool::new(pool, "session1".to_string());
        let result = tool
            .execute(serde_json::json!({
                "evidence": "all done",
                "tests_run": ["cargo test"]
            }))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["accepted"], false);
        assert_eq!(parsed["verdict"], "not_met");
    }

    #[tokio::test]
    async fn test_goal_request_completion_semantic_criterion_awaits_user() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "session1", "/tmp/test").await;
        let store = GoalStore::new(pool.clone());
        store
            .create_active(
                "session1",
                "/tmp/test",
                "Test Goal",
                "Do something",
                None,
                None,
                vec!["Product owner signs off".into()],
            )
            .await
            .unwrap();
        insert_completed_test_job(&pool, "session1").await;
        let tool = GoalRequestCompletionTool::new(pool.clone(), "session1".to_string());
        let result = tool
            .execute(serde_json::json!({
                "evidence": "all done",
                "tests_run": ["cargo test"]
            }))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["accepted"], false);
        assert_eq!(parsed["verdict"], "awaiting_user");
        assert_eq!(
            store
                .active_for_session("session1")
                .await
                .unwrap()
                .unwrap()
                .status,
            GoalStatus::AwaitingUser
        );
    }

    #[tokio::test]
    async fn test_goal_request_completion_pass_security_review_awaits_user() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "session1", "/tmp/test").await;
        let store = GoalStore::new(pool.clone());
        store
            .create_active(
                "session1",
                "/tmp/test",
                "Test Goal",
                "Do something",
                None,
                None,
                vec!["Pass security review".into()],
            )
            .await
            .unwrap();
        insert_completed_test_job(&pool, "session1").await;
        let tool = GoalRequestCompletionTool::new(pool, "session1".to_string());
        let result = tool
            .execute(serde_json::json!({
                "evidence": "all done",
                "tests_run": ["cargo test"]
            }))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["verdict"], "awaiting_user");
    }

    #[tokio::test]
    async fn test_goal_request_completion_ignores_other_goal_evidence() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "session1", "/tmp/test").await;
        let store = GoalStore::new(pool.clone());
        let goal_a = store
            .create_active(
                "session1",
                "/tmp/test",
                "Goal A",
                "Do A",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();
        insert_test_job_for_goal(&pool, "session1", AttemptState::Completed, Some(&goal_a.id))
            .await;
        let goal_b = store
            .create_active(
                "session1",
                "/tmp/test",
                "Goal B",
                "Do B",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();
        let evidence =
            crate::goal_verification::assemble(&pool, "session1", &goal_b.id, goal_b.created_at)
                .await
                .unwrap();
        assert!(evidence.executions.is_empty());
    }

    #[tokio::test]
    async fn test_goal_request_completion_other_goal_failure_does_not_poison_current_goal() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "session1", "/tmp/test").await;
        let store = GoalStore::new(pool.clone());
        let goal_a = store
            .create_active(
                "session1",
                "/tmp/test",
                "Goal A",
                "Do A",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();
        insert_test_job_for_goal(&pool, "session1", AttemptState::Failed, Some(&goal_a.id)).await;
        let goal_b = store
            .create_active(
                "session1",
                "/tmp/test",
                "Goal B",
                "Do B",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();
        insert_test_job_for_goal(&pool, "session1", AttemptState::Completed, Some(&goal_b.id))
            .await;
        let tool = GoalRequestCompletionTool::new(pool, "session1".to_string());
        let result = tool
            .execute(serde_json::json!({
                "evidence": "all done",
                "tests_run": ["unrelated claim"]
            }))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["accepted"], true);
    }
}
