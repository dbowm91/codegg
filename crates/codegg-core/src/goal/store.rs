use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::error::StorageError;

use super::{Goal, GoalBudget, GoalProgressUpdate, GoalStatus, GoalUsage};

/// Result of advancing a goal's usage counters. Carries the new usage
/// and the original budget so callers can render a live status without
/// re-reading the database.
#[derive(Debug, Clone)]
pub struct GoalUsageUpdate {
    pub usage: GoalUsage,
    pub budget: GoalBudget,
    pub budget_limited: bool,
    pub reason: Option<String>,
}

/// Check whether `usage` has exceeded any axis of `budget`. Returns the
/// first breach as a human-readable string, or `None` if the goal is
/// still within budget.
pub fn first_budget_breach(budget: &GoalBudget, usage: &GoalUsage) -> Option<String> {
    if let Some(max) = budget.max_model_tokens {
        let total = usage.input_tokens.saturating_add(usage.output_tokens);
        if total >= max {
            return Some(format!("token budget exceeded: {} used of {}", total, max));
        }
    }
    if let Some(max) = budget.max_tool_calls {
        if usage.tool_calls >= max {
            return Some(format!(
                "tool-call budget exceeded: {} used of {}",
                usage.tool_calls, max
            ));
        }
    }
    if let Some(max) = budget.max_turns {
        if usage.turns_used >= max {
            return Some(format!(
                "turn budget exceeded: {} used of {}",
                usage.turns_used, max
            ));
        }
    }
    if let Some(max) = budget.max_wallclock_secs {
        if usage.wallclock_secs >= max {
            return Some(format!(
                "wall-clock budget exceeded: {}s used of {}s",
                usage.wallclock_secs, max
            ));
        }
    }
    None
}

#[derive(Clone)]
pub struct GoalStore {
    pub pool: SqlitePool,
}

#[derive(sqlx::FromRow)]
struct GoalRow {
    id: String,
    session_id: String,
    project_id: String,
    title: String,
    objective: String,
    status: String,
    plan_path: Option<String>,
    checkpoint_path: Option<String>,
    current_phase: Option<String>,
    progress_summary: String,
    next_action: Option<String>,
    completion_criteria: String,
    open_questions: String,
    budget: String,
    usage: String,
    created_at: i64,
    updated_at: i64,
    started_at: Option<i64>,
    completed_at: Option<i64>,
}

fn millis_to_datetime(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default()
}

fn datetime_to_millis(dt: &DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

fn status_to_string(s: &GoalStatus) -> String {
    match s {
        GoalStatus::Active => "active".to_string(),
        GoalStatus::Paused => "paused".to_string(),
        GoalStatus::AwaitingUser => "awaiting_user".to_string(),
        GoalStatus::BudgetLimited => "budget_limited".to_string(),
        GoalStatus::Complete => "complete".to_string(),
        GoalStatus::Failed => "failed".to_string(),
        GoalStatus::Cancelled => "cancelled".to_string(),
    }
}

fn status_from_string(s: &str) -> GoalStatus {
    serde_json::from_str(&format!("\"{}\"", s)).unwrap_or(GoalStatus::Active)
}

fn row_to_goal(row: GoalRow) -> Result<Goal, StorageError> {
    let completion_criteria: Vec<String> = serde_json::from_str(&row.completion_criteria)
        .map_err(|e| StorageError::Database(format!("invalid completion_criteria: {}", e)))?;
    let open_questions: Vec<String> = serde_json::from_str(&row.open_questions)
        .map_err(|e| StorageError::Database(format!("invalid open_questions: {}", e)))?;
    let budget: GoalBudget = serde_json::from_str(&row.budget)
        .map_err(|e| StorageError::Database(format!("invalid budget: {}", e)))?;
    let usage: GoalUsage = serde_json::from_str(&row.usage)
        .map_err(|e| StorageError::Database(format!("invalid usage: {}", e)))?;

    Ok(Goal {
        id: row.id,
        session_id: row.session_id,
        project_id: row.project_id,
        title: row.title,
        objective: row.objective,
        status: status_from_string(&row.status),
        plan_path: row.plan_path,
        checkpoint_path: row.checkpoint_path,
        current_phase: row.current_phase,
        progress_summary: row.progress_summary,
        next_action: row.next_action,
        completion_criteria,
        open_questions,
        budget,
        usage,
        created_at: millis_to_datetime(row.created_at),
        updated_at: millis_to_datetime(row.updated_at),
        started_at: row.started_at.map(millis_to_datetime),
        completed_at: row.completed_at.map(millis_to_datetime),
    })
}

impl GoalStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_active(
        &self,
        session_id: &str,
        project_id: &str,
        title: &str,
        objective: &str,
        plan_path: Option<String>,
        checkpoint_path: Option<String>,
        completion_criteria: Vec<String>,
    ) -> Result<Goal, StorageError> {
        // Pause any existing active/awaiting_user/budget_limited goal for this session
        sqlx::query(
            r#"UPDATE goal SET status = 'paused', updated_at = ?1
               WHERE session_id = ?2 AND status IN ('active', 'awaiting_user', 'budget_limited')"#,
        )
        .bind(datetime_to_millis(&Utc::now()))
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        let id = uuid::Uuid::new_v4().to_string();
        let now = datetime_to_millis(&Utc::now());
        let status = status_to_string(&GoalStatus::Active);
        let criteria_json =
            serde_json::to_string(&completion_criteria).unwrap_or_else(|_| "[]".to_string());

        sqlx::query(
            r#"INSERT INTO goal (id, session_id, project_id, title, objective, status,
               plan_path, checkpoint_path,
               completion_criteria, open_questions, budget, usage,
               progress_summary, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '[]', '{}', '{}', '', ?10, ?11)"#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(project_id)
        .bind(title)
        .bind(objective)
        .bind(&status)
        .bind(&plan_path)
        .bind(&checkpoint_path)
        .bind(&criteria_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        self.get(&id)
            .await?
            .ok_or_else(|| StorageError::Database("inserted goal not found".into()))
    }

    pub async fn active_for_session(&self, session_id: &str) -> Result<Option<Goal>, StorageError> {
        let row = sqlx::query_as::<_, GoalRow>(
            r#"SELECT * FROM goal WHERE session_id = ?1
               AND status IN ('active', 'awaiting_user', 'budget_limited')
               ORDER BY created_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_goal).transpose()
    }

    pub async fn get(&self, id: &str) -> Result<Option<Goal>, StorageError> {
        let row = sqlx::query_as::<_, GoalRow>("SELECT * FROM goal WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        row.map(row_to_goal).transpose()
    }

    pub async fn update_status(
        &self,
        goal_id: &str,
        status: GoalStatus,
    ) -> Result<Option<Goal>, StorageError> {
        let now = datetime_to_millis(&Utc::now());
        let status_str = status_to_string(&status);

        if status == GoalStatus::Complete {
            sqlx::query(
                r#"UPDATE goal SET status = ?1, updated_at = ?2, completed_at = ?2
                   WHERE id = ?3"#,
            )
            .bind(&status_str)
            .bind(now)
            .bind(goal_id)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query("UPDATE goal SET status = ?1, updated_at = ?2 WHERE id = ?3")
                .bind(&status_str)
                .bind(now)
                .bind(goal_id)
                .execute(&self.pool)
                .await?;
        }

        self.get(goal_id).await
    }

    pub async fn clear_active_for_session(&self, session_id: &str) -> Result<(), StorageError> {
        let now = datetime_to_millis(&Utc::now());
        let cancelled = status_to_string(&GoalStatus::Cancelled);

        sqlx::query(
            r#"UPDATE goal SET status = ?1, updated_at = ?2
               WHERE session_id = ?3 AND status IN ('active', 'awaiting_user', 'budget_limited', 'paused')"#,
        )
        .bind(&cancelled)
        .bind(now)
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_progress(
        &self,
        goal_id: &str,
        update: GoalProgressUpdate,
    ) -> Result<Option<Goal>, StorageError> {
        let goal = self.get(goal_id).await?;
        let goal = match goal {
            Some(g) => g,
            None => return Ok(None),
        };

        let mut new_summary = goal.progress_summary.clone();

        if !update.completed_items.is_empty() || !update.remaining_items.is_empty() {
            let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S");
            let mut line = format!("[{}] Progress update:", timestamp);

            if !update.completed_items.is_empty() {
                let completed = update.completed_items.join(", ");
                line.push_str(&format!(" completed: {}", completed));
            }
            if !update.remaining_items.is_empty() {
                let remaining = update.remaining_items.join(", ");
                line.push_str(&format!(" remaining: {}", remaining));
            }

            if new_summary.is_empty() {
                new_summary = line;
            } else {
                new_summary.push('\n');
                new_summary.push_str(&line);
            }
        }

        let current_phase = update
            .current_phase
            .as_deref()
            .unwrap_or(goal.current_phase.as_deref().unwrap_or(""));

        let next_action = update
            .next_action
            .as_deref()
            .unwrap_or(goal.next_action.as_deref().unwrap_or(""));

        let open_questions = if update.open_questions.is_empty() {
            goal.open_questions.clone()
        } else {
            update.open_questions
        };

        let now = datetime_to_millis(&Utc::now());
        let open_questions_json = serde_json::to_string(&open_questions)
            .map_err(|e| StorageError::Database(format!("serialize open_questions: {}", e)))?;

        sqlx::query(
            r#"UPDATE goal
               SET current_phase = ?1,
                   next_action = ?2,
                   progress_summary = ?3,
                   open_questions = ?4,
                   updated_at = ?5
               WHERE id = ?6"#,
        )
        .bind(current_phase)
        .bind(next_action)
        .bind(&new_summary)
        .bind(&open_questions_json)
        .bind(now)
        .bind(goal_id)
        .execute(&self.pool)
        .await?;

        self.get(goal_id).await
    }

    /// Atomically advance the goal's usage counters and check the budget.
    ///
    /// `turns_delta` is the number of agent turns to add (typically 1).
    /// `wallclock_delta_secs` is the wall-clock time spent on the goal
    /// during the in-flight turn or batch of work. `input_tokens` and
    /// `output_tokens` are the prompt/completion tokens used.
    ///
    /// Returns a `GoalUsageUpdate` describing the new usage and whether
    /// the goal was transitioned to `BudgetLimited`. If the goal is not
    /// `Active`, the deltas are not applied and `Ok(None)` is returned.
    pub async fn increment_usage(
        &self,
        goal_id: &str,
        input_tokens: i64,
        output_tokens: i64,
        tool_calls: i64,
        turns_delta: i64,
        wallclock_delta_secs: i64,
    ) -> Result<Option<GoalUsageUpdate>, StorageError> {
        let goal = self.get(goal_id).await?;
        let goal = match goal {
            Some(g) => g,
            None => return Ok(None),
        };
        if !goal.is_active() {
            return Ok(None);
        }

        let mut usage = goal.usage.clone();
        usage.input_tokens = usage.input_tokens.saturating_add(input_tokens.max(0));
        usage.output_tokens = usage.output_tokens.saturating_add(output_tokens.max(0));
        usage.tool_calls = usage.tool_calls.saturating_add(tool_calls.max(0));
        usage.turns_used = usage.turns_used.saturating_add(turns_delta.max(0));
        usage.wallclock_secs = usage
            .wallclock_secs
            .saturating_add(wallclock_delta_secs.max(0));

        let usage_json = serde_json::to_string(&usage)
            .map_err(|e| StorageError::Database(format!("serialize usage: {}", e)))?;

        let now = datetime_to_millis(&Utc::now());
        sqlx::query("UPDATE goal SET usage = ?1, updated_at = ?2 WHERE id = ?3")
            .bind(&usage_json)
            .bind(now)
            .bind(goal_id)
            .execute(&self.pool)
            .await?;

        // Check whether the new usage exceeds any budget axis.
        let breach = first_budget_breach(&goal.budget, &usage);
        if let Some(reason) = breach {
            self.update_status(goal_id, GoalStatus::BudgetLimited)
                .await?;
            return Ok(Some(GoalUsageUpdate {
                usage,
                budget: goal.budget,
                budget_limited: true,
                reason: Some(reason),
            }));
        }

        Ok(Some(GoalUsageUpdate {
            usage,
            budget: goal.budget,
            budget_limited: false,
            reason: None,
        }))
    }

    /// Convenience: enforce the budget on a goal without advancing the
    /// counters. Returns the new status if a transition occurred.
    pub async fn enforce_budget(&self, goal_id: &str) -> Result<Option<Goal>, StorageError> {
        let goal = self.get(goal_id).await?;
        let Some(goal) = goal else {
            return Ok(None);
        };
        if !goal.is_active() {
            return Ok(Some(goal));
        }
        if first_budget_breach(&goal.budget, &goal.usage).is_some() {
            return self.update_status(goal_id, GoalStatus::BudgetLimited).await;
        }
        Ok(Some(goal))
    }

    /// Set or replace the budget on an active goal. Used by the
    /// `/goal budget raise …` slash command.
    pub async fn set_budget(
        &self,
        goal_id: &str,
        budget: GoalBudget,
    ) -> Result<Option<Goal>, StorageError> {
        let budget_json = serde_json::to_string(&budget)
            .map_err(|e| StorageError::Database(format!("serialize budget: {}", e)))?;
        let now = datetime_to_millis(&Utc::now());
        // If the goal is currently BudgetLimited because the old
        // budget was hit, transition it back to Active when the user
        // raises the cap.
        sqlx::query(
            r#"UPDATE goal
               SET budget = ?1,
                   updated_at = ?2,
                   status = CASE
                       WHEN status = 'budget_limited' THEN 'active'
                       ELSE status
                   END
               WHERE id = ?3"#,
        )
        .bind(&budget_json)
        .bind(now)
        .bind(goal_id)
        .execute(&self.pool)
        .await?;
        self.get(goal_id).await
    }

    pub async fn latest_paused_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<Goal>, StorageError> {
        let row = sqlx::query_as::<_, GoalRow>(
            r#"SELECT * FROM goal WHERE session_id = ?1 AND status = 'paused'
               ORDER BY updated_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_goal).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::schema::migrate;

    async fn test_pool() -> SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:goal_test_{}?mode=memory&cache=shared",
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
        migrate(&pool).await.expect("migrate");
        pool
    }

    /// Insert a minimal project + session so FK constraints are satisfied.
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

    #[tokio::test(flavor = "current_thread")]
    async fn test_create_active() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        let goal = store
            .create_active(
                "sess1",
                "proj1",
                "Test Goal",
                "Do something",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();

        assert_eq!(goal.session_id, "sess1");
        assert_eq!(goal.project_id, "proj1");
        assert_eq!(goal.title, "Test Goal");
        assert_eq!(goal.objective, "Do something");
        assert_eq!(goal.status, GoalStatus::Active);
        assert!(goal.completed_at.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_active_for_session() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        store
            .create_active(
                "sess1",
                "proj1",
                "Goal A",
                "Objective A",
                None,
                None,
                vec![],
            )
            .await
            .unwrap();

        let active = store.active_for_session("sess1").await.unwrap();
        assert!(active.is_some());
        assert_eq!(active.unwrap().status, GoalStatus::Active);

        // No goal for other sessions
        let none = store.active_for_session("sess2").await.unwrap();
        assert!(none.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_second_active_pauses_first() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        let g1 = store
            .create_active("sess1", "proj1", "Goal 1", "Obj 1", None, None, vec![])
            .await
            .unwrap();

        store
            .create_active("sess1", "proj1", "Goal 2", "Obj 2", None, None, vec![])
            .await
            .unwrap();

        // g1 should now be paused
        let g1_reloaded = store.get(&g1.id).await.unwrap().unwrap();
        assert_eq!(g1_reloaded.status, GoalStatus::Paused);

        // Active for session should return g2
        let active = store.active_for_session("sess1").await.unwrap().unwrap();
        assert_eq!(active.title, "Goal 2");
        assert_eq!(active.status, GoalStatus::Active);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_update_status_complete_sets_completed_at() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        let goal = store
            .create_active("sess1", "proj1", "Goal", "Obj", None, None, vec![])
            .await
            .unwrap();
        assert!(goal.completed_at.is_none());

        let updated = store
            .update_status(&goal.id, GoalStatus::Complete)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(updated.status, GoalStatus::Complete);
        assert!(updated.completed_at.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_update_progress() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        let goal = store
            .create_active("sess1", "proj1", "Goal", "Obj", None, None, vec![])
            .await
            .unwrap();

        let update = GoalProgressUpdate {
            current_phase: Some("Phase 1".into()),
            progress_summary: None,
            next_action: Some("Write tests".into()),
            completed_items: vec!["Module A".into(), "Module B".into()],
            remaining_items: vec!["Module C".into()],
            open_questions: vec!["Is X correct?".into()],
        };

        let updated = store
            .update_progress(&goal.id, update)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(updated.current_phase.as_deref(), Some("Phase 1"));
        assert_eq!(updated.next_action.as_deref(), Some("Write tests"));
        assert_eq!(updated.open_questions, vec!["Is X correct?"]);
        assert!(updated
            .progress_summary
            .contains("completed: Module A, Module B"));
        assert!(updated.progress_summary.contains("remaining: Module C"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_increment_usage() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        let goal = store
            .create_active("sess1", "proj1", "Goal", "Obj", None, None, vec![])
            .await
            .unwrap();

        assert_eq!(goal.usage.input_tokens, 0);
        assert_eq!(goal.usage.output_tokens, 0);
        assert_eq!(goal.usage.tool_calls, 0);

        let r1 = store
            .increment_usage(&goal.id, 100, 50, 5, 1, 0)
            .await
            .unwrap()
            .unwrap();
        assert!(!r1.budget_limited);
        let r2 = store
            .increment_usage(&goal.id, 200, 75, 10, 1, 0)
            .await
            .unwrap()
            .unwrap();
        assert!(!r2.budget_limited);

        let updated = store.get(&goal.id).await.unwrap().unwrap();
        assert_eq!(updated.usage.input_tokens, 300);
        assert_eq!(updated.usage.output_tokens, 125);
        assert_eq!(updated.usage.tool_calls, 15);
        assert_eq!(updated.usage.turns_used, 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_increment_usage_budget_limited() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        let goal = store
            .create_active("sess1", "proj1", "Goal", "Obj", None, None, vec![])
            .await
            .unwrap();
        // Set a tight tool-call budget and watch it trip.
        let budget = GoalBudget {
            max_tool_calls: Some(3),
            ..GoalBudget::default()
        };
        store.set_budget(&goal.id, budget.clone()).await.unwrap();
        let r = store
            .increment_usage(&goal.id, 10, 5, 3, 1, 0)
            .await
            .unwrap()
            .unwrap();
        assert!(r.budget_limited);
        assert!(r.reason.is_some());
        let updated = store.get(&goal.id).await.unwrap().unwrap();
        assert_eq!(updated.status, GoalStatus::BudgetLimited);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_set_budget_revives_budget_limited() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        let goal = store
            .create_active("sess1", "proj1", "Goal", "Obj", None, None, vec![])
            .await
            .unwrap();
        store
            .increment_usage(&goal.id, 0, 0, 0, 0, 999_999)
            .await
            .unwrap();
        let limited = store
            .set_budget(
                &goal.id,
                GoalBudget {
                    max_wallclock_secs: Some(1_000_000),
                    ..GoalBudget::default()
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(limited.status, GoalStatus::Active);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_clear_active_for_session() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        let g1 = store
            .create_active("sess1", "proj1", "Goal 1", "Obj 1", None, None, vec![])
            .await
            .unwrap();
        let g2 = store
            .create_active("sess1", "proj1", "Goal 2", "Obj 2", None, None, vec![])
            .await
            .unwrap();

        store.clear_active_for_session("sess1").await.unwrap();

        // Both goals should be cancelled
        let g1_status = store.get(&g1.id).await.unwrap().unwrap().status;
        let g2_status = store.get(&g2.id).await.unwrap().unwrap().status;
        assert_eq!(g1_status, GoalStatus::Cancelled);
        assert_eq!(g2_status, GoalStatus::Cancelled);

        // No active goals for session
        let active = store.active_for_session("sess1").await.unwrap();
        assert!(active.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_latest_paused_for_session() {
        let pool = test_pool().await;
        ensure_test_session(&pool, "sess1", "proj1").await;
        let store = GoalStore::new(pool);

        // No paused goals initially
        let none = store.latest_paused_for_session("sess1").await.unwrap();
        assert!(none.is_none());

        // Create and pause a goal
        let g1 = store
            .create_active("sess1", "proj1", "Goal 1", "Obj 1", None, None, vec![])
            .await
            .unwrap();
        store
            .create_active("sess1", "proj1", "Goal 2", "Obj 2", None, None, vec![])
            .await
            .unwrap();

        // g1 should be paused now (first goal paused when second was created)
        let paused = store
            .latest_paused_for_session("sess1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(paused.id, g1.id);
        assert_eq!(paused.status, GoalStatus::Paused);
    }
}
