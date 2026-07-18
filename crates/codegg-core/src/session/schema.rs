use sqlx::SqlitePool;

use crate::error::StorageError;

pub async fn migrate(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS migration_version (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            version INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    let current_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    if current_version < 1 {
        migrate_and_record(pool, 1).await?;
    }
    if current_version < 2 {
        migrate_and_record(pool, 2).await?;
    }
    if current_version < 3 {
        migrate_and_record(pool, 3).await?;
    }
    if current_version < 4 {
        migrate_and_record(pool, 4).await?;
    }
    if current_version < 5 {
        migrate_and_record(pool, 5).await?;
    }
    if current_version < 6 {
        migrate_and_record(pool, 6).await?;
    }
    if current_version < 7 {
        migrate_and_record(pool, 7).await?;
    }
    if current_version < 8 {
        migrate_and_record(pool, 8).await?;
    }
    if current_version < 9 {
        migrate_and_record(pool, 9).await?;
    }
    if current_version < 10 {
        migrate_and_record(pool, 10).await?;
    }
    if current_version < 11 {
        migrate_and_record(pool, 11).await?;
    }
    if current_version < 12 {
        migrate_and_record(pool, 12).await?;
    }
    if current_version < 13 {
        migrate_and_record(pool, 13).await?;
    }
    if current_version < 14 {
        migrate_and_record(pool, 14).await?;
    }
    if current_version < 15 {
        migrate_and_record(pool, 15).await?;
    }
    if current_version < 16 {
        migrate_and_record(pool, 16).await?;
    }
    if current_version < 17 {
        migrate_and_record(pool, 17).await?;
    }
    if current_version < 18 {
        migrate_and_record(pool, 18).await?;
    }
    if current_version < 19 {
        migrate_and_record(pool, 19).await?;
    }
    if current_version < 20 {
        migrate_and_record(pool, 20).await?;
    }
    if current_version < 21 {
        migrate_and_record(pool, 21).await?;
    }
    if current_version < 22 {
        migrate_and_record(pool, 22).await?;
    }
    if current_version < 23 {
        migrate_and_record(pool, 23).await?;
    }
    if current_version < 24 {
        migrate_and_record(pool, 24).await?;
    }
    if current_version < 25 {
        migrate_and_record(pool, 25).await?;
    }
    if current_version < 26 {
        migrate_and_record(pool, 26).await?;
    }
    if current_version < 27 {
        migrate_and_record(pool, 27).await?;
    }
    if current_version < 28 {
        migrate_and_record(pool, 28).await?;
    }
    if current_version < 29 {
        migrate_and_record(pool, 29).await?;
    }
    if current_version < 30 {
        migrate_and_record(pool, 30).await?;
    }
    if current_version < 31 {
        migrate_and_record(pool, 31).await?;
    }

    Ok(())
}

async fn migrate_and_record(pool: &SqlitePool, version: i64) -> Result<(), StorageError> {
    sqlx::query("BEGIN IMMEDIATE")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    let result = async {
        match version {
            1 => migrate_v1(pool).await?,
            2 => migrate_v2(pool).await?,
            3 => migrate_v3(pool).await?,
            4 => migrate_v4(pool).await?,
            5 => migrate_v5(pool).await?,
            6 => migrate_v6(pool).await?,
            7 => migrate_v7(pool).await?,
            8 => migrate_v8(pool).await?,
            9 => migrate_v9(pool).await?,
            10 => migrate_v10(pool).await?,
            11 => migrate_v11(pool).await?,
            12 => migrate_v12(pool).await?,
            13 => migrate_v13(pool).await?,
            14 => migrate_v14(pool).await?,
            15 => migrate_v15(pool).await?,
            16 => migrate_v16(pool).await?,
            17 => migrate_v17(pool).await?,
            18 => migrate_v18(pool).await?,
            19 => migrate_v19(pool).await?,
            20 => migrate_v20(pool).await?,
            21 => migrate_v21(pool).await?,
            22 => migrate_v22(pool).await?,
            23 => migrate_v23(pool).await?,
            24 => migrate_v24(pool).await?,
            25 => migrate_v25(pool).await?,
            26 => migrate_v26(pool).await?,
            27 => migrate_v27(pool).await?,
            28 => migrate_v28(pool).await?,
            29 => migrate_v29(pool).await?,
            30 => migrate_v30(pool).await?,
            31 => migrate_v31(pool).await?,
            _ => {
                return Err(StorageError::Migration(format!(
                    "unknown migration version {}",
                    version
                )))
            }
        }
        sqlx::query(
            "INSERT INTO migration_version (id, version) VALUES (1, ?) \
             ON CONFLICT(id) DO UPDATE SET version = excluded.version",
        )
        .bind(version)
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
        Ok::<(), StorageError>(())
    }
    .await;

    match result {
        Ok(()) => {
            sqlx::query("COMMIT")
                .execute(pool)
                .await
                .map_err(|e| StorageError::Migration(e.to_string()))?;
            Ok(())
        }
        Err(e) => {
            let _ = sqlx::query("ROLLBACK").execute(pool).await;
            Err(e)
        }
    }
}

async fn migrate_v1(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS project (
            id TEXT PRIMARY KEY,
            worktree TEXT NOT NULL,
            vcs TEXT,
            name TEXT,
            icon_url TEXT,
            icon_color TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            time_initialized INTEGER,
            sandboxes TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            workspace_id TEXT,
            parent_id TEXT,
            slug TEXT NOT NULL,
            directory TEXT NOT NULL,
            title TEXT NOT NULL,
            version TEXT NOT NULL,
            share_url TEXT,
            summary_additions INTEGER,
            summary_deletions INTEGER,
            summary_files INTEGER,
            summary_diffs TEXT,
            revert TEXT,
            permission TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            time_compacting INTEGER,
            time_archived INTEGER,
            FOREIGN KEY (project_id) REFERENCES project(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS part (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL,
            FOREIGN KEY (message_id) REFERENCES message(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS todo (
            session_id TEXT NOT NULL,
            content TEXT NOT NULL,
            status TEXT NOT NULL,
            priority TEXT NOT NULL,
            position INTEGER NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            PRIMARY KEY (session_id, position),
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS permission (
            project_id TEXT PRIMARY KEY,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL,
            FOREIGN KEY (project_id) REFERENCES project(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session_share (
            session_id TEXT PRIMARY KEY,
            id TEXT NOT NULL,
            secret TEXT NOT NULL,
            url TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS session_project_idx ON session(project_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS session_workspace_idx ON session(workspace_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS session_parent_idx ON session(parent_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS todo_session_idx ON todo(session_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS message_session_time_created_id_idx ON message(session_id, time_created, id)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS part_message_id_id_idx ON part(message_id, id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS part_session_idx ON part(session_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v2(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("CREATE INDEX IF NOT EXISTS session_title_idx ON session(title)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS session_slug_idx ON session(slug)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v3(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS cached_models (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            name TEXT NOT NULL,
            context_window INTEGER,
            max_output_tokens INTEGER,
            supports_tools INTEGER NOT NULL DEFAULT 1,
            supports_vision INTEGER NOT NULL DEFAULT 0,
            fetched_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS cached_models_provider_idx ON cached_models(provider)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v4(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("CREATE INDEX IF NOT EXISTS session_time_updated_idx ON session(time_updated)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v5(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("ALTER TABLE session_share ADD COLUMN share_expires_at INTEGER")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v6(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS permission_time_idx ON permission(time_created, time_updated)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS session_project_archived_idx ON session(project_id, time_archived)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v7(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("ALTER TABLE session ADD COLUMN tags TEXT")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS session_tags_idx ON session(tags)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v8(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        "ALTER TABLE part ADD COLUMN part_type TEXT GENERATED ALWAYS AS (json_extract(data, '$.type')) STORED",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS part_type_idx ON part(part_type)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v9(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS task (
            id INTEGER PRIMARY KEY,
            parent_id TEXT,
            session_id TEXT NOT NULL,
            description TEXT NOT NULL,
            prompt TEXT NOT NULL,
            agent TEXT NOT NULL,
            status TEXT NOT NULL,
            result TEXT,
            denied_tools TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS task_session_idx ON task(session_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS task_parent_idx ON task(parent_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v10(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS checkpoints (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            state TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS checkpoint_session_idx ON checkpoints(session_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v11(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_session_directory ON session(directory)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v12(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("ALTER TABLE session ADD COLUMN time_deleted INTEGER DEFAULT NULL")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v13(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS snapshot (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            label TEXT,
            data TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS snapshot_session_idx ON snapshot(session_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v14(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query("ALTER TABLE task ADD COLUMN allowed_paths TEXT")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v15(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS usage (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cached_tokens INTEGER NOT NULL DEFAULT 0,
            cost_usd REAL NOT NULL,
            timestamp INTEGER NOT NULL,
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS usage_session_idx ON usage(session_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v16(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS goal (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            objective TEXT NOT NULL,
            status TEXT NOT NULL,

            plan_path TEXT,
            checkpoint_path TEXT,

            current_phase TEXT,
            progress_summary TEXT NOT NULL DEFAULT '',
            next_action TEXT,
            completion_criteria TEXT NOT NULL DEFAULT '[]',
            open_questions TEXT NOT NULL DEFAULT '[]',

            budget TEXT NOT NULL DEFAULT '{}',
            usage TEXT NOT NULL DEFAULT '{}',

            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            started_at INTEGER,
            completed_at INTEGER,

            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS goal_session_status_idx ON goal(session_id, status)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS goal_project_status_idx ON goal(project_id, status)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v17(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session_events (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_session_events_session_created ON session_events(session_id, created_at)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v18(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS research_run (
            run_id TEXT PRIMARY KEY,
            question TEXT NOT NULL,
            mode TEXT NOT NULL,
            depth TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            artifact_dir TEXT NOT NULL,
            error TEXT,
            sources_count INTEGER NOT NULL DEFAULT 0,
            evidence_count INTEGER NOT NULL DEFAULT 0,
            claims_count INTEGER NOT NULL DEFAULT 0,
            contradictions_count INTEGER NOT NULL DEFAULT 0,
            project_root TEXT NOT NULL DEFAULT ''
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_research_run_status ON research_run(status)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_research_run_started ON research_run(started_at)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_research_run_project ON research_run(project_root)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v19(pool: &SqlitePool) -> Result<(), StorageError> {
    // Generic key/value preferences for things that must outlive config
    // changes and survive a config file reset. Currently used for the
    // active theme id and the last-used model id.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_preferences (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v20(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS core_event_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_seq INTEGER NOT NULL,
            session_id TEXT,
            turn_id TEXT,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(event_seq)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_core_event_log_session ON core_event_log(session_id)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_core_event_log_seq ON core_event_log(event_seq)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

async fn migrate_v21(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notification_history (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            turn_id TEXT,
            kind TEXT NOT NULL,
            priority TEXT NOT NULL,
            message TEXT NOT NULL,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_notification_history_session ON notification_history(session_id)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_notification_history_kind ON notification_history(kind)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

/// Phase 2 of the single-daemon plan: introduce a typed `workspace` table
/// and backfill the previously-NULL `session.workspace_id` column.
///
/// Migration strategy:
/// 1. Create the `workspace` table (id PK, canonical_root UNIQUE,
///    display_name, time_created, time_last_opened, time_archived).
/// 2. Backfill one workspace per distinct, *canonicalizable* `directory`
///    referenced by at least one session. Sessions whose directory does
///    not resolve to a real directory are left unbound; their
///    `workspace_id` remains NULL and the daemon must reject turn
///    submission until an explicit workspace command rebinds them.
/// 3. Add the `idx_session_workspace_repair` index for cheap lookup of
///    unbound sessions.
///
/// We deliberately use only `directory` (filesystem path) and not
/// `project_id`: the latter is a stable string identity, while `directory`
/// carries the filesystem intent. Legacy compatibility fields stay in
/// place; new code reads `workspace_id`.
async fn migrate_v22(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS workspace (
            id TEXT PRIMARY KEY,
            canonical_root TEXT NOT NULL UNIQUE,
            display_name TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_last_opened INTEGER NOT NULL,
            time_archived INTEGER
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_workspace_archived ON workspace(time_archived)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_session_workspace_repair ON session(workspace_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

/// Phase 4 of the single-daemon plan: introduce durable jobs, attempts,
/// dependencies, schedules, and schedule occurrences. This migration
/// creates the full set of tables required by [`crate::jobs`].
async fn migrate_v23(pool: &SqlitePool) -> Result<(), StorageError> {
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
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

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
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

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
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

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
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

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
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    // Queue-scan indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_job_state ON job(state)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_job_priority ON job(priority)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_job_workspace ON job(workspace_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_job_session ON job(session_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_job_schedule ON job(schedule_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_job_not_before ON job(not_before)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_job_time_updated ON job(time_updated DESC)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    // Attempt indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_job_attempt_job ON job_attempt(job_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_job_attempt_state ON job_attempt(state)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_job_attempt_generation ON job_attempt(daemon_generation)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    // Dependency indexes
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_job_dependency_depends ON job_dependency(depends_on_job_id)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    // Schedule indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_schedule_workspace ON schedule(workspace_id)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_schedule_state ON schedule(state)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_schedule_next_run ON schedule(next_run_at)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

/// Durable daemon-owned provider connection metadata. Secret material is
/// deliberately absent; the three binding columns contain only opaque
/// references and account/provider locators.
async fn migrate_v24(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_connections (
            id TEXT PRIMARY KEY,
            provider_kind TEXT NOT NULL,
            display_name TEXT NOT NULL,
            endpoint TEXT NOT NULL,
            tls_policy TEXT NOT NULL,
            scope_kind TEXT NOT NULL CHECK (scope_kind IN ('personal', 'project', 'deployment')),
            scope_ref TEXT NOT NULL,
            secret_ref TEXT NOT NULL DEFAULT '',
            secret_provider_ref TEXT NOT NULL DEFAULT '',
            secret_account_ref TEXT NOT NULL DEFAULT '',
            state TEXT NOT NULL CHECK (state IN ('active', 'disabled', 'credential_missing')),
            revision INTEGER NOT NULL CHECK (revision > 0),
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            UNIQUE (
                scope_kind,
                scope_ref,
                provider_kind,
                endpoint,
                tls_policy,
                secret_provider_ref,
                secret_account_ref
            ),
            CHECK (
                (secret_ref = '' AND secret_provider_ref = '' AND secret_account_ref = '')
                OR
                (secret_ref <> '' AND secret_provider_ref <> '' AND secret_account_ref <> '')
            )
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_provider_connections_scope \
         ON provider_connections(scope_kind, scope_ref)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_provider_connections_state \
         ON provider_connections(state)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_provider_connections_updated \
         ON provider_connections(time_updated DESC)",
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}

/// Domain Identity Milestone 002: additive canonical project/repository
/// authority. The historical `project` table and string-backed session
/// fields intentionally remain untouched compatibility projections.
async fn migrate_v25(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in [
        r#"
        CREATE TABLE IF NOT EXISTS logical_project (
            id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            lifecycle TEXT NOT NULL CHECK (lifecycle IN ('active', 'archived')),
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS repository (
            id TEXT PRIMARY KEY,
            vcs_kind TEXT NOT NULL,
            lineage_key TEXT,
            remote_identity TEXT,
            common_directory TEXT,
            default_branch TEXT,
            head TEXT,
            provenance TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('resolved', 'unresolved', 'ambiguous', 'stale_locator', 'rebind_required')),
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            UNIQUE(vcs_kind, lineage_key)
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS project_repository (
            project_id TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            relation_kind TEXT NOT NULL CHECK (relation_kind IN ('primary')),
            time_created INTEGER NOT NULL,
            revision INTEGER NOT NULL CHECK (revision > 0),
            PRIMARY KEY(project_id, repository_id),
            FOREIGN KEY(project_id) REFERENCES logical_project(id) ON DELETE CASCADE,
            FOREIGN KEY(repository_id) REFERENCES repository(id) ON DELETE CASCADE
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS workspace_project_binding (
            workspace_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            repository_id TEXT,
            worktree_id TEXT,
            node_id TEXT,
            locator TEXT,
            status TEXT NOT NULL CHECK (status IN ('resolved', 'unresolved', 'ambiguous', 'stale_locator', 'rebind_required')),
            source TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK (revision > 0),
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            FOREIGN KEY(workspace_id) REFERENCES workspace(id) ON DELETE CASCADE,
            FOREIGN KEY(project_id) REFERENCES logical_project(id) ON DELETE RESTRICT,
            FOREIGN KEY(repository_id) REFERENCES repository(id) ON DELETE RESTRICT
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS session_project_binding (
            session_id TEXT PRIMARY KEY,
            project_id TEXT,
            workspace_id TEXT,
            status TEXT NOT NULL CHECK (status IN ('resolved', 'unresolved', 'ambiguous', 'stale_locator', 'rebind_required')),
            source TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK (revision > 0),
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            FOREIGN KEY(session_id) REFERENCES session(id) ON DELETE CASCADE,
            FOREIGN KEY(project_id) REFERENCES logical_project(id) ON DELETE RESTRICT,
            FOREIGN KEY(workspace_id) REFERENCES workspace(id) ON DELETE RESTRICT
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS identity_diagnostic (
            id TEXT PRIMARY KEY,
            workspace_id TEXT,
            session_id TEXT,
            project_id TEXT,
            code TEXT NOT NULL,
            status TEXT NOT NULL CHECK (status IN ('resolved', 'unresolved', 'ambiguous', 'stale_locator', 'rebind_required')),
            message TEXT NOT NULL,
            source TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            FOREIGN KEY(workspace_id) REFERENCES workspace(id) ON DELETE CASCADE,
            FOREIGN KEY(session_id) REFERENCES session(id) ON DELETE CASCADE,
            FOREIGN KEY(project_id) REFERENCES logical_project(id) ON DELETE CASCADE
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_logical_project_lifecycle ON logical_project(lifecycle)",
        "CREATE INDEX IF NOT EXISTS idx_logical_project_updated ON logical_project(time_updated DESC)",
        "CREATE INDEX IF NOT EXISTS idx_repository_lineage ON repository(vcs_kind, lineage_key)",
        "CREATE INDEX IF NOT EXISTS idx_repository_status ON repository(status)",
        "CREATE INDEX IF NOT EXISTS idx_project_repository_repository ON project_repository(repository_id)",
        "CREATE INDEX IF NOT EXISTS idx_workspace_project_binding_project ON workspace_project_binding(project_id)",
        "CREATE INDEX IF NOT EXISTS idx_workspace_project_binding_repository ON workspace_project_binding(repository_id)",
        "CREATE INDEX IF NOT EXISTS idx_workspace_project_binding_status ON workspace_project_binding(status)",
        "CREATE INDEX IF NOT EXISTS idx_session_project_binding_project ON session_project_binding(project_id)",
        "CREATE INDEX IF NOT EXISTS idx_session_project_binding_workspace ON session_project_binding(workspace_id)",
        "CREATE INDEX IF NOT EXISTS idx_session_project_binding_status ON session_project_binding(status)",
        "CREATE INDEX IF NOT EXISTS idx_identity_diagnostic_workspace ON identity_diagnostic(workspace_id, time_updated DESC)",
        "CREATE INDEX IF NOT EXISTS idx_identity_diagnostic_session ON identity_diagnostic(session_id, time_updated DESC)",
        "CREATE INDEX IF NOT EXISTS idx_identity_diagnostic_status ON identity_diagnostic(status, time_updated DESC)",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Migration(e.to_string()))?;
    }

    Ok(())
}

/// Provider Connections Milestone 2: crash-recoverable provisioning state
/// and bounded health/model catalog metadata. No credential material is
/// stored in these tables.
async fn migrate_v26(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in [
        r#"
        CREATE TABLE IF NOT EXISTS provider_provisioning (
            operation_id TEXT PRIMARY KEY,
            connection_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            provider_kind TEXT NOT NULL,
            display_name TEXT NOT NULL,
            endpoint TEXT NOT NULL,
            tls_policy TEXT NOT NULL,
            scope_kind TEXT NOT NULL,
            scope_ref TEXT NOT NULL,
            secret_ref TEXT NOT NULL,
            secret_provider_ref TEXT NOT NULL,
            secret_account_ref TEXT NOT NULL,
            state TEXT NOT NULL CHECK (state IN ('staged', 'probing', 'committed', 'failed', 'cancelled')),
            failure_code TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            UNIQUE(idempotency_key)
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_provider_provisioning_state ON provider_provisioning(state, time_updated DESC)",
        "CREATE INDEX IF NOT EXISTS idx_provider_provisioning_connection ON provider_provisioning(connection_id)",
        r#"
        CREATE TABLE IF NOT EXISTS provider_connection_health (
            connection_id TEXT PRIMARY KEY,
            revision INTEGER NOT NULL CHECK (revision > 0),
            status TEXT NOT NULL CHECK (status IN ('healthy', 'unhealthy')),
            reason_code TEXT,
            duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
            checked_at INTEGER NOT NULL,
            catalog_revision TEXT
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_provider_connection_health_status ON provider_connection_health(status, checked_at DESC)",
        r#"
        CREATE TABLE IF NOT EXISTS provider_connection_models (
            connection_id TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK (revision > 0),
            model_id TEXT NOT NULL,
            model_name TEXT NOT NULL,
            context_window INTEGER NOT NULL CHECK (context_window >= 0),
            max_output_tokens INTEGER,
            supports_tools INTEGER NOT NULL CHECK (supports_tools IN (0, 1)),
            supports_vision INTEGER NOT NULL CHECK (supports_vision IN (0, 1)),
            PRIMARY KEY(connection_id, revision, model_id),
            FOREIGN KEY(connection_id) REFERENCES provider_connections(id) ON DELETE CASCADE
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_provider_connection_models_lookup ON provider_connection_models(connection_id, revision, model_id)",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Migration(e.to_string()))?;
    }
    Ok(())
}

/// Provider Connections Milestone 3: additive session selection fields.
/// The legacy `agent`/`model` strings are now persisted alongside the
/// authoritative connection ID + revision + model catalog revision + model
/// ID. All columns are nullable; existing rows migrate unchanged and the
/// legacy compatibility adapter resolves them lazily on read.
async fn migrate_v27(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in [
        "ALTER TABLE session ADD COLUMN provider_connection_id TEXT",
        "ALTER TABLE session ADD COLUMN provider_connection_revision INTEGER",
        "ALTER TABLE session ADD COLUMN model_catalog_revision TEXT",
        "ALTER TABLE session ADD COLUMN selected_model_id TEXT",
        "ALTER TABLE session ADD COLUMN agent TEXT",
        "ALTER TABLE session ADD COLUMN model TEXT",
        "CREATE INDEX IF NOT EXISTS idx_session_provider_connection ON session(provider_connection_id)",
        "CREATE INDEX IF NOT EXISTS idx_session_selected_model ON session(selected_model_id)",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Migration(e.to_string()))?;
    }
    Ok(())
}

/// Project Catalog Milestone 1: catalog-specific tables, archive timestamp,
/// description/tags/registration-source columns on logical_project, and the
/// legacy catalog association marker table. All tables and columns are
/// additive and idempotent across restart.
async fn migrate_v28(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in [
        "ALTER TABLE logical_project ADD COLUMN archived_at INTEGER",
        "ALTER TABLE logical_project ADD COLUMN description TEXT",
        "ALTER TABLE logical_project ADD COLUMN tags TEXT",
        "ALTER TABLE logical_project ADD COLUMN registration_source TEXT NOT NULL DEFAULT 'unknown'",
        "ALTER TABLE logical_project ADD COLUMN time_last_opened INTEGER",
        r#"
        CREATE TABLE IF NOT EXISTS project_locator (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            locator_kind TEXT NOT NULL CHECK (locator_kind IN ('local','ssh','linked_node')),
            workspace_id TEXT,
            canonical_root TEXT,
            ssh_host TEXT,
            ssh_port INTEGER,
            ssh_user TEXT,
            ssh_path TEXT,
            ssh_label TEXT,
            linked_node_id TEXT,
            linked_node_alias TEXT,
            linked_node_path_hint TEXT,
            display_summary TEXT NOT NULL,
            source TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            FOREIGN KEY(project_id) REFERENCES logical_project(id) ON DELETE CASCADE,
            FOREIGN KEY(workspace_id) REFERENCES workspace(id) ON DELETE CASCADE
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS project_health (
            project_id TEXT PRIMARY KEY,
            status TEXT NOT NULL CHECK (status IN ('unknown','available','unavailable','unsupported','stale','error')),
            error_code TEXT,
            error_message TEXT,
            source TEXT NOT NULL,
            time_evaluated INTEGER NOT NULL,
            notes TEXT,
            FOREIGN KEY(project_id) REFERENCES logical_project(id) ON DELETE CASCADE
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS legacy_catalog_association_marker (
            source TEXT PRIMARY KEY,
            completed_at INTEGER NOT NULL,
            projects_associated INTEGER NOT NULL,
            diagnostics_recorded INTEGER NOT NULL
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_project_locator_project ON project_locator(project_id)",
        "CREATE INDEX IF NOT EXISTS idx_project_locator_kind ON project_locator(locator_kind)",
        "CREATE INDEX IF NOT EXISTS idx_project_health_status ON project_health(status)",
    ] {
        if statement.starts_with("ALTER TABLE") {
            if let Err(e) = sqlx::query(statement).execute(pool).await {
                let msg = e.to_string();
                if !msg.contains("duplicate column name") {
                    return Err(StorageError::Migration(msg));
                }
            }
        } else {
            sqlx::query(statement)
                .execute(pool)
                .await
                .map_err(|e| StorageError::Migration(e.to_string()))?;
        }
    }

    Ok(())
}

/// Project Catalog Milestone 2: bounded discovery roots, scan generations,
/// and metadata-only observations. The tables are additive and retain catalog
/// authority when a root becomes unavailable or a scan is cancelled.
async fn migrate_v29(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in [
        r#"
        CREATE TABLE IF NOT EXISTS discovery_root (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            canonical_root TEXT NOT NULL,
            mode TEXT NOT NULL CHECK (mode IN ('git', 'directory', 'mixed')),
            enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
            revision INTEGER NOT NULL CHECK (revision > 0),
            max_depth INTEGER NOT NULL CHECK (max_depth >= 0 AND max_depth <= 64),
            max_entries INTEGER NOT NULL CHECK (max_entries > 0 AND max_entries <= 1000000),
            max_candidates INTEGER NOT NULL CHECK (max_candidates > 0 AND max_candidates <= 100000),
            max_duration_ms INTEGER NOT NULL CHECK (max_duration_ms > 0 AND max_duration_ms <= 3600000),
            stat_concurrency INTEGER NOT NULL CHECK (stat_concurrency > 0 AND stat_concurrency <= 256),
            git_probe_concurrency INTEGER NOT NULL CHECK (git_probe_concurrency > 0 AND git_probe_concurrency <= 64),
            include_hidden INTEGER NOT NULL CHECK (include_hidden IN (0, 1)),
            follow_symlinks INTEGER NOT NULL CHECK (follow_symlinks IN (0, 1)),
            ignore_names_json TEXT NOT NULL DEFAULT '[]',
            directory_markers_json TEXT NOT NULL DEFAULT '[]',
            direct_child_only INTEGER NOT NULL CHECK (direct_child_only IN (0, 1)),
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS discovery_scan (
            id TEXT PRIMARY KEY,
            root_id TEXT NOT NULL,
            generation INTEGER NOT NULL CHECK (generation > 0),
            status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'cancelled', 'failed', 'truncated', 'interrupted')),
            visited_entries INTEGER NOT NULL CHECK (visited_entries >= 0),
            ignored_entries INTEGER NOT NULL CHECK (ignored_entries >= 0),
            candidate_count INTEGER NOT NULL CHECK (candidate_count >= 0),
            reconciled_count INTEGER NOT NULL CHECK (reconciled_count >= 0),
            ambiguous_count INTEGER NOT NULL CHECK (ambiguous_count >= 0),
            unavailable_count INTEGER NOT NULL CHECK (unavailable_count >= 0),
            error_count INTEGER NOT NULL CHECK (error_count >= 0),
            duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
            truncated INTEGER NOT NULL CHECK (truncated IN (0, 1)),
            diagnostics_json TEXT NOT NULL DEFAULT '[]',
            started_at INTEGER NOT NULL,
            completed_at INTEGER,
            FOREIGN KEY(root_id) REFERENCES discovery_root(id) ON DELETE CASCADE,
            UNIQUE(root_id, generation)
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS discovery_observation (
            id TEXT PRIMARY KEY,
            root_id TEXT NOT NULL,
            scan_id TEXT NOT NULL,
            generation INTEGER NOT NULL CHECK (generation > 0),
            canonical_locator TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            candidate_kind TEXT NOT NULL CHECK (candidate_kind IN ('git_repository', 'directory')),
            lineage_key TEXT,
            project_id TEXT,
            workspace_id TEXT,
            status TEXT NOT NULL CHECK (status IN ('present', 'moved', 'missing', 'ambiguous', 'inaccessible', 'ignored', 'stale')),
            outcome TEXT NOT NULL,
            diagnostic TEXT,
            time_observed INTEGER NOT NULL,
            FOREIGN KEY(root_id) REFERENCES discovery_root(id) ON DELETE CASCADE,
            FOREIGN KEY(scan_id) REFERENCES discovery_scan(id) ON DELETE CASCADE,
            FOREIGN KEY(project_id) REFERENCES logical_project(id) ON DELETE SET NULL,
            FOREIGN KEY(workspace_id) REFERENCES workspace(id) ON DELETE SET NULL
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_discovery_root_path ON discovery_root(canonical_root)",
        "CREATE INDEX IF NOT EXISTS idx_discovery_scan_root_generation ON discovery_scan(root_id, generation DESC)",
        "CREATE INDEX IF NOT EXISTS idx_discovery_scan_status ON discovery_scan(status, started_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_discovery_observation_root_generation ON discovery_observation(root_id, generation DESC)",
        "CREATE INDEX IF NOT EXISTS idx_discovery_observation_project ON discovery_observation(project_id, status)",
        "CREATE INDEX IF NOT EXISTS idx_discovery_observation_workspace ON discovery_observation(workspace_id, status)",
        "CREATE INDEX IF NOT EXISTS idx_discovery_observation_status ON discovery_observation(status, time_observed DESC)",
        "CREATE INDEX IF NOT EXISTS idx_discovery_observation_lineage ON discovery_observation(lineage_key)",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Migration(e.to_string()))?;
    }
    Ok(())
}

/// Runtime Assets Milestone 3: bounded publication metadata. Snapshot bodies
/// remain reconstructible from an explicit workspace context; this table only
/// preserves the last successful generation, fingerprint, and diagnostics
/// needed for restart/operator continuity.
async fn migrate_v30(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in [
        r#"
        CREATE TABLE IF NOT EXISTS runtime_asset_refresh (
            project_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            generation INTEGER NOT NULL CHECK (generation >= 0),
            fingerprint TEXT,
            last_success_at INTEGER,
            diagnostics_json TEXT NOT NULL DEFAULT '[]',
            time_updated INTEGER NOT NULL,
            PRIMARY KEY (project_id, workspace_id)
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_runtime_asset_refresh_updated ON runtime_asset_refresh(time_updated DESC)",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Migration(e.to_string()))?;
    }
    Ok(())
}

/// Provider Connections Milestone 5: additive lifecycle, reference,
/// tombstone, and audit metadata. The historical provider_connections.state
/// CHECK constraint remains a compatibility projection; extended lifecycle
/// states are authoritative in provider_connection_lifecycle.
async fn migrate_v31(pool: &SqlitePool) -> Result<(), StorageError> {
    for statement in [
        r#"
        CREATE TABLE IF NOT EXISTS provider_connection_lifecycle (
            connection_id TEXT PRIMARY KEY,
            state TEXT NOT NULL CHECK (state IN ('active','disabled','credential_missing','provisioning_rotating','tombstoned','error','stale')),
            revision INTEGER NOT NULL CHECK (revision > 0),
            time_updated INTEGER NOT NULL,
            FOREIGN KEY(connection_id) REFERENCES provider_connections(id) ON DELETE CASCADE
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS provider_connection_references (
            connection_id TEXT NOT NULL,
            reference_kind TEXT NOT NULL CHECK (reference_kind IN ('selected_session','provisioning_operation','active_runtime')),
            reference_id TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY(connection_id, reference_kind, reference_id)
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_provider_connection_references_connection ON provider_connection_references(connection_id, reference_kind)",
        r#"
        CREATE TABLE IF NOT EXISTS provider_connection_tombstones (
            connection_id TEXT PRIMARY KEY,
            tombstoned_at INTEGER NOT NULL,
            tombstoned_by_actor TEXT NOT NULL,
            last_known_revision INTEGER NOT NULL,
            last_known_catalog_revision TEXT,
            last_known_endpoint_authority TEXT NOT NULL,
            FOREIGN KEY(connection_id) REFERENCES provider_connections(id) ON DELETE CASCADE
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS provider_connection_audit_events (
            event_id TEXT PRIMARY KEY,
            connection_id TEXT NOT NULL,
            action TEXT NOT NULL,
            actor_seam TEXT NOT NULL,
            old_revision INTEGER,
            new_revision INTEGER,
            endpoint_authority TEXT,
            outcome TEXT NOT NULL,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            time_created INTEGER NOT NULL,
            FOREIGN KEY(connection_id) REFERENCES provider_connections(id) ON DELETE CASCADE
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_provider_connection_audit_connection ON provider_connection_audit_events(connection_id, time_created DESC)",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| StorageError::Migration(e.to_string()))?;
    }
    Ok(())
}
