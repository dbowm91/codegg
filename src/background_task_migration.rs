use std::time::Duration;

use sqlx::{Row, SqlitePool};
use tracing::warn;

use codegg_core::error::StorageError;
use codegg_core::jobs::schedule::{JobTemplate, MissedRunPolicy, OverlapPolicy, ScheduleTemplate};
use codegg_core::jobs::{JobKind, ScheduleId, ScheduleKind, ScheduleStore};
use codegg_core::workspace::WorkspaceId;

use crate::agent::task::parse_duration;

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("schedule store error: {0}")]
    Schedule(#[from] codegg_core::jobs::ScheduleError),
}

#[derive(Debug, Clone)]
pub struct MigratedTask {
    pub db_id: i64,
    pub schedule_id: ScheduleId,
    pub interval: Duration,
}

pub async fn migrate_legacy_background_tasks(
    pool: &SqlitePool,
    schedule_store: &dyn ScheduleStore,
    workspace_id: &WorkspaceId,
) -> Result<Vec<MigratedTask>, MigrationError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS migration_marker (\
            source_path TEXT PRIMARY KEY, \
            imported_at INTEGER NOT NULL, \
            sessions_count INTEGER NOT NULL, \
            messages_count INTEGER NOT NULL, \
            storage_layout_version INTEGER NOT NULL\
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    let rows = sqlx::query(
        r#"
        SELECT id, session_id, description, prompt, time_created
        FROM task
        WHERE status IN ('pending', 'running')
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?;

    let mut migrated = Vec::new();

    for row in rows {
        let db_id: i64 = row.get("id");
        let marker_id = format!("legacy_background_task:{}", db_id);

        let already: Option<(String,)> =
            sqlx::query_as("SELECT source_path FROM migration_marker WHERE source_path = ?")
                .bind(&marker_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
        if already.is_some() {
            continue;
        }

        let prompt: String = row.get("prompt");
        let session_id: Option<String> = row.get("session_id");
        let description: String = row.get("description");
        let time_created: i64 = row.get("time_created");

        let interval = match parse_duration(&prompt) {
            Some(d) => d,
            None => {
                warn!(
                    db_id,
                    prompt, "Skipping background task migration: cannot parse duration from prompt"
                );
                let now_ms = chrono::Utc::now().timestamp_millis();
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO migration_marker
                        (source_path, imported_at, sessions_count, messages_count, storage_layout_version)
                    VALUES (?, ?, 0, 0, 0)
                    "#,
                )
                .bind(&marker_id)
                .bind(now_ms)
                .execute(pool)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
                sqlx::query("UPDATE task SET status = 'interrupted' WHERE id = ?")
                    .bind(db_id)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::Database(e.to_string()))?;
                continue;
            }
        };

        let anchor = chrono::DateTime::<chrono::Utc>::from_timestamp(time_created, 0)
            .unwrap_or_else(chrono::Utc::now);

        let kind = ScheduleKind::Interval {
            every: interval,
            anchor,
        };
        let job_template = JobTemplate::for_subagent(
            JobKind::Subagent,
            description.clone(),
            "build".to_string(),
            session_id.clone(),
        );
        let template = ScheduleTemplate {
            workspace_id: workspace_id.clone(),
            session_id: session_id.clone(),
            kind,
            job_template,
            overlap_policy: OverlapPolicy::SkipIfRunning,
            missed_run_policy: MissedRunPolicy::RunOnceNow,
            next_run_at: None,
            labels: std::collections::HashMap::new(),
        };

        let record = schedule_store.create(template).await?;

        let now_ms = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO migration_marker
                (source_path, imported_at, sessions_count, messages_count, storage_layout_version)
            VALUES (?, ?, 0, 0, 0)
            "#,
        )
        .bind(&marker_id)
        .bind(now_ms)
        .execute(pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query("UPDATE task SET status = 'interrupted' WHERE id = ?")
            .bind(db_id)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        migrated.push(MigratedTask {
            db_id,
            schedule_id: record.schedule_id,
            interval,
        });
    }

    Ok(migrated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Arc;

    async fn setup_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE task (
                id INTEGER PRIMARY KEY,
                parent_id TEXT,
                session_id TEXT NOT NULL,
                description TEXT NOT NULL,
                prompt TEXT NOT NULL,
                agent TEXT NOT NULL DEFAULT 'background',
                status TEXT NOT NULL DEFAULT 'pending',
                result TEXT,
                denied_tools TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS schedule (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                session_id TEXT,
                kind_json TEXT NOT NULL,
                job_template_json TEXT NOT NULL,
                state TEXT NOT NULL,
                overlap_policy TEXT NOT NULL,
                missed_run_policy_json TEXT NOT NULL,
                next_run_at INTEGER,
                last_occurrence_at INTEGER,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                labels_json TEXT NOT NULL DEFAULT '{}'
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS schedule_occurrence (
                schedule_id TEXT NOT NULL,
                scheduled_for INTEGER NOT NULL,
                job_id TEXT,
                status TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                PRIMARY KEY(schedule_id, scheduled_for)
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS job (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                session_id TEXT,
                turn_id TEXT,
                kind TEXT NOT NULL,
                source_json TEXT NOT NULL,
                priority TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                resource_json TEXT NOT NULL,
                retry_json TEXT NOT NULL,
                idempotency TEXT NOT NULL,
                state TEXT NOT NULL,
                current_attempt_id TEXT,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                not_before INTEGER,
                deadline INTEGER,
                schedule_id TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                time_terminal INTEGER,
                cancel_requested_at INTEGER,
                cancel_reason TEXT,
                labels_json TEXT NOT NULL DEFAULT '{}'
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS job_attempt (
                id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                state TEXT NOT NULL,
                daemon_generation TEXT NOT NULL,
                executor TEXT,
                run_id TEXT,
                heartbeat_at INTEGER,
                time_started INTEGER,
                time_completed INTEGER,
                error_json TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                UNIQUE(job_id, sequence)
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS job_dependency (
                id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                depends_on_job_id TEXT NOT NULL,
                condition TEXT NOT NULL DEFAULT 'completed',
                UNIQUE(job_id, depends_on_job_id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migrate_well_formed_tasks() {
        let pool = setup_db().await;
        let job_store = Arc::new(codegg_core::jobs::SqliteJobStore::new(pool.clone()));
        let schedule_store = codegg_core::jobs::SqliteScheduleStore::new(pool.clone(), job_store);
        let ws = WorkspaceId::new_unchecked("test-ws");

        sqlx::query(
            r#"
            INSERT INTO task (id, session_id, description, prompt, agent, status, time_created, time_updated)
            VALUES (1, 'sess1', 'Build check', '30min', 'background', 'pending', 1000, 1000)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO task (id, session_id, description, prompt, agent, status, time_created, time_updated)
            VALUES (2, 'sess2', 'Lint run', '1h', 'background', 'running', 2000, 2000)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let migrated = migrate_legacy_background_tasks(&pool, &schedule_store, &ws)
            .await
            .unwrap();
        assert_eq!(migrated.len(), 2);
        assert_eq!(migrated[0].db_id, 1);
        assert_eq!(migrated[0].interval, Duration::from_secs(1800));
        assert_eq!(migrated[1].db_id, 2);
        assert_eq!(migrated[1].interval, Duration::from_secs(3600));

        let schedules = schedule_store
            .list(codegg_core::jobs::ScheduleQuery::default())
            .await
            .unwrap();
        assert_eq!(schedules.len(), 2);

        let markers: Vec<(String,)> =
            sqlx::query_as("SELECT source_path FROM migration_marker ORDER BY source_path")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0].0, "legacy_background_task:1");
        assert_eq!(markers[1].0, "legacy_background_task:2");

        let statuses: Vec<(String,)> = sqlx::query_as("SELECT status FROM task ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(statuses[0].0, "interrupted");
        assert_eq!(statuses[1].0, "interrupted");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migrate_malformed_prompt_skipped() {
        let pool = setup_db().await;
        let job_store = Arc::new(codegg_core::jobs::SqliteJobStore::new(pool.clone()));
        let schedule_store = codegg_core::jobs::SqliteScheduleStore::new(pool.clone(), job_store);
        let ws = WorkspaceId::new_unchecked("test-ws");

        sqlx::query(
            r#"
            INSERT INTO task (id, session_id, description, prompt, agent, status, time_created, time_updated)
            VALUES (1, 'sess1', 'Bad task', 'this is not a duration', 'background', 'pending', 1000, 1000)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let migrated = migrate_legacy_background_tasks(&pool, &schedule_store, &ws)
            .await
            .unwrap();
        assert!(migrated.is_empty());

        let schedules = schedule_store
            .list(codegg_core::jobs::ScheduleQuery::default())
            .await
            .unwrap();
        assert!(schedules.is_empty());

        let status: (String,) = sqlx::query_as("SELECT status FROM task WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status.0, "interrupted");

        let marker: Option<(String,)> = sqlx::query_as(
            "SELECT source_path FROM migration_marker WHERE source_path = 'legacy_background_task:1'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(marker.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migrate_is_idempotent() {
        let pool = setup_db().await;
        let job_store = Arc::new(codegg_core::jobs::SqliteJobStore::new(pool.clone()));
        let schedule_store = codegg_core::jobs::SqliteScheduleStore::new(pool.clone(), job_store);
        let ws = WorkspaceId::new_unchecked("test-ws");

        sqlx::query(
            r#"
            INSERT INTO task (id, session_id, description, prompt, agent, status, time_created, time_updated)
            VALUES (1, 'sess1', 'Check', '5m', 'background', 'pending', 1000, 1000)
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let m1 = migrate_legacy_background_tasks(&pool, &schedule_store, &ws)
            .await
            .unwrap();
        assert_eq!(m1.len(), 1);

        let m2 = migrate_legacy_background_tasks(&pool, &schedule_store, &ws)
            .await
            .unwrap();
        assert!(m2.is_empty(), "second invocation should be idempotent");

        let schedules = schedule_store
            .list(codegg_core::jobs::ScheduleQuery::default())
            .await
            .unwrap();
        assert_eq!(schedules.len(), 1);

        let markers: Vec<(String,)> = sqlx::query_as("SELECT source_path FROM migration_marker")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(markers.len(), 1);
    }
}
