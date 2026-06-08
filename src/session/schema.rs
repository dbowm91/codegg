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
