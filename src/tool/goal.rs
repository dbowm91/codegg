use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::error::ToolError;
use crate::goal::model::*;
use crate::goal::store::GoalStore;
use crate::tool::Tool;

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
            let _ = crate::goal::checkpoint::append_checkpoint_update(cp_path, &update).await;
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
        "Request completion of the current active goal"
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

        if evidence.is_empty() {
            return Ok(serde_json::json!({
                "accepted": false,
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

        if tests_run.is_empty() && remaining_risks.is_empty() {
            return Ok(serde_json::json!({
                "accepted": false,
                "reason": "Either tests_run must be non-empty, or remaining_risks must explicitly justify skipped tests"
            })
            .to_string());
        }

        match store.update_status(&goal.id, GoalStatus::Complete).await {
            Ok(Some(updated_goal)) => {
                if let Some(ref cp_path) = goal.checkpoint_path {
                    let update = GoalProgressUpdate {
                        current_phase: Some("Complete".to_string()),
                        progress_summary: Some(format!(
                            "Completed with evidence: {}",
                            evidence.chars().take(200).collect::<String>()
                        )),
                        next_action: None,
                        completed_items: files_changed,
                        remaining_items: vec![],
                        open_questions: vec![],
                    };
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
                        evidence: evidence.chars().take(500).collect::<String>(),
                    },
                );
                Ok(serde_json::json!({
                    "accepted": true,
                    "goal_id": goal.id,
                    "status": "complete"
                })
                .to_string())
            }
            Ok(None) => {
                crate::bus::global::GlobalEventBus::publish(
                    crate::bus::events::AppEvent::GoalUpdated {
                        session_id: self.session_id.clone(),
                        goal: Box::new(None),
                    },
                );
                Ok(serde_json::json!({
                    "accepted": true,
                    "goal_id": goal.id,
                    "status": "complete"
                })
                .to_string())
            }
            Err(e) => Err(ToolError::Execution(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
