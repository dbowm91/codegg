use chrono::Utc;
use sqlx::Row;
use sqlx::SqlitePool;
use uuid::Uuid;

use codegg_protocol::projection::event::ProjectionEnvelope;
use codegg_protocol::projection::replay::{
    ProjectionStreamDescriptor, ProjectionStreamId, ProjectionStreamKind,
};

use crate::error::StorageError;

#[derive(Debug, Clone)]
pub struct ProjectionReplayRow {
    pub stream_id: String,
    pub event_seq: u64,
    pub projection_version: u32,
    pub timestamp_ms: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub event_kind: String,
    pub visibility_class: String,
    pub payload_json: String,
    pub payload_bytes: i64,
    pub source_core_seq: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CheckpointRow {
    pub stream_id: String,
    pub checkpoint_seq: u64,
    pub projection_version: u32,
    pub snapshot_json: String,
    pub snapshot_bytes: i64,
    pub created_at: i64,
}

pub struct ProjectionReplayStore {
    pool: SqlitePool,
}

impl ProjectionReplayStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn get_or_create_session_stream(
        &self,
        session_id: &str,
        project_id: &str,
        workspace_id: Option<&str>,
    ) -> Result<(ProjectionStreamDescriptor, bool), StorageError> {
        let now = Utc::now().timestamp_millis();
        let id = Uuid::new_v4().to_string();

        sqlx::query(
            r#"INSERT INTO projection_stream (id, kind, project_id, workspace_id, session_id, binding_revision, projection_version, next_seq, retention_floor_seq, high_water_seq, latest_checkpoint_seq, created_at, updated_at, lifecycle)
            VALUES (?, 'session', ?, ?, ?, 1, 1, 1, 0, 0, NULL, ?, ?, 'active')
            ON CONFLICT(kind, project_id, session_id, lifecycle) DO NOTHING"#,
        )
        .bind(&id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(session_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let row = sqlx::query(
            r#"SELECT id, kind, project_id, workspace_id, session_id, projection_version, retention_floor_seq, high_water_seq, latest_checkpoint_seq
            FROM projection_stream WHERE kind = 'session' AND project_id = ? AND session_id = ? AND lifecycle = 'active'"#,
        )
        .bind(project_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        match row {
            Some(r) => {
                let created = r.get::<String, _>("id") != id;
                Ok((Self::row_to_descriptor(&r)?, !created))
            }
            None => Err(StorageError::Database(
                "stream creation failed unexpectedly".into(),
            )),
        }
    }

    pub async fn get_or_create_project_stream(
        &self,
        project_id: &str,
    ) -> Result<(ProjectionStreamDescriptor, bool), StorageError> {
        let now = Utc::now().timestamp_millis();
        let id = Uuid::new_v4().to_string();

        sqlx::query(
            r#"INSERT INTO projection_stream (id, kind, project_id, workspace_id, session_id, binding_revision, projection_version, next_seq, retention_floor_seq, high_water_seq, latest_checkpoint_seq, created_at, updated_at, lifecycle)
            VALUES (?, 'project', ?, NULL, NULL, 1, 1, 1, 0, 0, NULL, ?, ?, 'active')
            ON CONFLICT(kind, project_id, session_id, lifecycle) DO NOTHING"#,
        )
        .bind(&id)
        .bind(project_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let row = sqlx::query(
            r#"SELECT id, kind, project_id, workspace_id, session_id, projection_version, retention_floor_seq, high_water_seq, latest_checkpoint_seq
            FROM projection_stream WHERE kind = 'project' AND project_id = ? AND session_id IS NULL AND lifecycle = 'active'"#,
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        match row {
            Some(r) => {
                let created = r.get::<String, _>("id") != id;
                Ok((Self::row_to_descriptor(&r)?, !created))
            }
            None => Err(StorageError::Database(
                "stream creation failed unexpectedly".into(),
            )),
        }
    }

    pub async fn lookup_stream_by_id(
        &self,
        stream_id: &str,
    ) -> Result<Option<ProjectionStreamDescriptor>, StorageError> {
        let row = sqlx::query(
            r#"SELECT id, kind, project_id, workspace_id, session_id, projection_version, retention_floor_seq, high_water_seq, latest_checkpoint_seq
            FROM projection_stream WHERE id = ? AND lifecycle = 'active'"#,
        )
        .bind(stream_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        row.map(|r| Self::row_to_descriptor(&r)).transpose()
    }

    pub async fn invalidate_stream(
        &self,
        stream_id: &str,
    ) -> Result<(), StorageError> {
        let now = Utc::now().timestamp_millis();
        sqlx::query(
            r#"UPDATE projection_stream SET lifecycle = 'invalidated', updated_at = ? WHERE id = ?"#,
        )
        .bind(now)
        .bind(stream_id)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn next_event_seq(
        &self,
        stream_id: &str,
    ) -> Result<u64, StorageError> {
        let mut tx = self.pool.begin().await.map_err(|e| StorageError::Database(e.to_string()))?;

        let row = sqlx::query(
            "SELECT next_seq FROM projection_stream WHERE id = ?",
        )
        .bind(stream_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let seq: i64 = row.get("next_seq");
        let new_seq = seq + 1;

        sqlx::query("UPDATE projection_stream SET next_seq = ? WHERE id = ?")
            .bind(new_seq)
            .bind(stream_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        tx.commit().await.map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(seq as u64)
    }

    pub async fn insert_event(
        &self,
        stream_id: &str,
        event_seq: u64,
        envelope: &ProjectionEnvelope,
    ) -> Result<(), StorageError> {
        let payload_json =
            serde_json::to_string(envelope).map_err(|e| StorageError::Database(e.to_string()))?;
        let payload_bytes = payload_json.len() as i64;
        let now = Utc::now().timestamp_millis();
        let event_kind = format!("{:?}", envelope.payload);

        sqlx::query(
            r#"INSERT INTO projection_event (stream_id, event_seq, projection_version, timestamp_ms, session_id, turn_id, event_kind, visibility_class, payload_json, payload_bytes, source_core_seq, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(stream_id)
        .bind(event_seq as i64)
        .bind(envelope.protocol_version)
        .bind(envelope.timestamp_ms)
        .bind(&envelope.session_id)
        .bind(&envelope.turn_id)
        .bind(&event_kind)
        .bind("public")
        .bind(&payload_json)
        .bind(payload_bytes)
        .bind(None::<i64>)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn insert_event_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        stream_id: &str,
        event_seq: u64,
        envelope: &ProjectionEnvelope,
    ) -> Result<(), StorageError> {
        let payload_json =
            serde_json::to_string(envelope).map_err(|e| StorageError::Database(e.to_string()))?;
        let payload_bytes = payload_json.len() as i64;
        let now = Utc::now().timestamp_millis();
        let event_kind = format!("{:?}", envelope.payload);

        sqlx::query(
            r#"INSERT INTO projection_event (stream_id, event_seq, projection_version, timestamp_ms, session_id, turn_id, event_kind, visibility_class, payload_json, payload_bytes, source_core_seq, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(stream_id)
        .bind(event_seq as i64)
        .bind(envelope.protocol_version)
        .bind(envelope.timestamp_ms)
        .bind(&envelope.session_id)
        .bind(&envelope.turn_id)
        .bind(&event_kind)
        .bind("public")
        .bind(&payload_json)
        .bind(payload_bytes)
        .bind(None::<i64>)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn update_high_water(
        &self,
        stream_id: &str,
        event_seq: u64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE projection_stream SET high_water_seq = ? WHERE id = ?",
        )
        .bind(event_seq as i64)
        .bind(stream_id)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn update_high_water_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        stream_id: &str,
        event_seq: u64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE projection_stream SET high_water_seq = ? WHERE id = ?",
        )
        .bind(event_seq as i64)
        .bind(stream_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn events_after(
        &self,
        stream_id: &str,
        from_seq: u64,
        max_count: usize,
        max_bytes: u64,
    ) -> Result<Vec<ProjectionReplayRow>, StorageError> {
        let mut rows = sqlx::query(
            r#"SELECT stream_id, event_seq, projection_version, timestamp_ms, session_id, turn_id, event_kind, visibility_class, payload_json, payload_bytes, source_core_seq
            FROM projection_event WHERE stream_id = ? AND event_seq > ? ORDER BY event_seq ASC LIMIT ?"#,
        )
        .bind(stream_id)
        .bind(from_seq as i64)
        .bind(max_count as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut total_bytes: u64 = 0;
        let mut result = Vec::new();
        for row in rows.drain(..) {
            let bytes: i64 = row.get("payload_bytes");
            total_bytes += bytes as u64;
            if total_bytes > max_bytes && !result.is_empty() {
                break;
            }
            result.push(ProjectionReplayRow {
                stream_id: row.get("stream_id"),
                event_seq: row.get::<i64, _>("event_seq") as u64,
                projection_version: row.get::<i64, _>("projection_version") as u32,
                timestamp_ms: row.get("timestamp_ms"),
                session_id: row.get("session_id"),
                turn_id: row.get("turn_id"),
                event_kind: row.get("event_kind"),
                visibility_class: row.get("visibility_class"),
                payload_json: row.get("payload_json"),
                payload_bytes: bytes,
                source_core_seq: row.get("source_core_seq"),
            });
        }
        Ok(result)
    }

    pub async fn event_exists(
        &self,
        stream_id: &str,
        seq: u64,
    ) -> Result<bool, StorageError> {
        let row = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM projection_event WHERE stream_id = ? AND event_seq = ?)",
        )
        .bind(stream_id)
        .bind(seq as i64)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(row)
    }

    pub async fn write_checkpoint(
        &self,
        stream_id: &str,
        checkpoint_seq: u64,
        projection_version: u32,
        snapshot_json: &str,
    ) -> Result<(), StorageError> {
        let bytes = snapshot_json.len() as i64;
        let now = Utc::now().timestamp_millis();
        sqlx::query(
            r#"INSERT INTO projection_checkpoint (stream_id, checkpoint_seq, projection_version, snapshot_json, snapshot_bytes, created_at)
            VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(stream_id)
        .bind(checkpoint_seq as i64)
        .bind(projection_version as i64)
        .bind(snapshot_json)
        .bind(bytes)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            "UPDATE projection_stream SET latest_checkpoint_seq = ? WHERE id = ?",
        )
        .bind(checkpoint_seq as i64)
        .bind(stream_id)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn load_checkpoint_at_or_before(
        &self,
        stream_id: &str,
        at_seq: u64,
    ) -> Result<Option<CheckpointRow>, StorageError> {
        let row = sqlx::query(
            r#"SELECT stream_id, checkpoint_seq, projection_version, snapshot_json, snapshot_bytes, created_at
            FROM projection_checkpoint WHERE stream_id = ? AND checkpoint_seq <= ? ORDER BY checkpoint_seq DESC LIMIT 1"#,
        )
        .bind(stream_id)
        .bind(at_seq as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(row.map(|r| CheckpointRow {
            stream_id: r.get("stream_id"),
            checkpoint_seq: r.get::<i64, _>("checkpoint_seq") as u64,
            projection_version: r.get::<i64, _>("projection_version") as u32,
            snapshot_json: r.get("snapshot_json"),
            snapshot_bytes: r.get("snapshot_bytes"),
            created_at: r.get("created_at"),
        }))
    }

    pub async fn prune_before(
        &self,
        stream_id: &str,
        retention_floor_seq: u64,
    ) -> Result<usize, StorageError> {
        let result = sqlx::query(
            "DELETE FROM projection_event WHERE stream_id = ? AND event_seq < ?",
        )
        .bind(stream_id)
        .bind(retention_floor_seq as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        sqlx::query(
            "UPDATE projection_stream SET retention_floor_seq = ? WHERE id = ?",
        )
        .bind(retention_floor_seq as i64)
        .bind(stream_id)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(result.rows_affected() as usize)
    }

    pub async fn prune_old_checkpoints(
        &self,
        stream_id: &str,
        keep_count: usize,
    ) -> Result<usize, StorageError> {
        let ids_to_delete: Vec<(String, i64)> = sqlx::query_as(
            r#"SELECT stream_id, checkpoint_seq FROM projection_checkpoint WHERE stream_id = ? ORDER BY checkpoint_seq DESC LIMIT -1 OFFSET ?"#,
        )
        .bind(stream_id)
        .bind(keep_count as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut deleted = 0;
        for (sid, seq) in &ids_to_delete {
            sqlx::query("DELETE FROM projection_checkpoint WHERE stream_id = ? AND checkpoint_seq = ?")
                .bind(sid)
                .bind(seq)
                .execute(&self.pool)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
            deleted += 1;
        }
        Ok(deleted)
    }

    pub async fn stream_count(&self) -> Result<u64, StorageError> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM projection_stream WHERE lifecycle = 'active'")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(row.0 as u64)
    }

    pub async fn total_event_bytes(&self) -> Result<u64, StorageError> {
        let row: (i64,) = sqlx::query_as("SELECT COALESCE(SUM(payload_bytes), 0) FROM projection_event")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        Ok(row.0 as u64)
    }

    pub async fn load_active_streams(
        &self,
    ) -> Result<Vec<ProjectionStreamDescriptor>, StorageError> {
        let rows = sqlx::query(
            r#"SELECT id, kind, project_id, workspace_id, session_id, projection_version, retention_floor_seq, high_water_seq, latest_checkpoint_seq
            FROM projection_stream WHERE lifecycle = 'active'"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        rows.iter()
            .map(Self::row_to_descriptor)
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn begin_tx(
        &self,
    ) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, StorageError> {
        self.pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))
    }

    fn row_to_descriptor(
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<ProjectionStreamDescriptor, StorageError> {
        let kind_str: String = row.get("kind");
        let kind = match kind_str.as_str() {
            "session" => ProjectionStreamKind::Session,
            "project" => ProjectionStreamKind::Project,
            _ => return Err(StorageError::Database(format!("unknown stream kind: {kind_str}"))),
        };
        Ok(ProjectionStreamDescriptor {
            stream_id: ProjectionStreamId(row.get("id")),
            kind,
            project_id: row.get("project_id"),
            workspace_id: row.get("workspace_id"),
            session_id: row.get("session_id"),
            projection_version: row.get::<i64, _>("projection_version") as u32,
            retention_floor_seq: row.get::<i64, _>("retention_floor_seq") as u64,
            high_water_seq: row.get::<i64, _>("high_water_seq") as u64,
            latest_checkpoint_seq: row.get::<Option<i64>, _>("latest_checkpoint_seq")
                .map(|v| v as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn test_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str(":memory:")
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        crate::session::schema::migrate(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn get_or_create_session_stream_idempotent() {
        let pool = test_pool().await;
        let store = ProjectionReplayStore::new(pool);
        let (d1, _created1) = store
            .get_or_create_session_stream("s1", "p1", Some("w1"))
            .await
            .unwrap();
        let (d2, created2) = store
            .get_or_create_session_stream("s1", "p1", Some("w1"))
            .await
            .unwrap();
        assert_eq!(d1.stream_id, d2.stream_id);
        assert!(!created2);
        assert_eq!(d1.kind, ProjectionStreamKind::Session);
    }

    #[tokio::test]
    async fn get_or_create_project_stream_idempotent() {
        let pool = test_pool().await;
        let store = ProjectionReplayStore::new(pool);
        let (d1, _) = store.get_or_create_project_stream("p1").await.unwrap();
        let (d2, created2) = store.get_or_create_project_stream("p1").await.unwrap();
        assert_eq!(d1.stream_id, d2.stream_id);
        assert!(!created2);
    }

    #[tokio::test]
    async fn next_event_seq_allocates_contiguously() {
        let pool = test_pool().await;
        let store = ProjectionReplayStore::new(pool);
        let (desc, _) = store
            .get_or_create_session_stream("s1", "p1", None)
            .await
            .unwrap();
        let s = desc.stream_id.as_str();
        assert_eq!(store.next_event_seq(s).await.unwrap(), 1);
        assert_eq!(store.next_event_seq(s).await.unwrap(), 2);
        assert_eq!(store.next_event_seq(s).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn events_after_returns_ordered() {
        let pool = test_pool().await;
        let store = ProjectionReplayStore::new(pool);
        let (desc, _) = store
            .get_or_create_session_stream("s1", "p1", None)
            .await
            .unwrap();
        let s = desc.stream_id.as_str();

        for i in 0..5 {
            let env = ProjectionEnvelope::session_event(
                i,
                i as i64 * 1000,
                "s1",
                None,
                codegg_protocol::projection::event::ProjectionEvent::Diagnostic {
                    code: format!("c{i}"),
                    message: format!("m{i}"),
                },
            );
            store.insert_event(s, i, &env).await.unwrap();
        }

        let rows = store.events_after(s, 2, 10, 1024 * 1024).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].event_seq, 3);
        assert_eq!(rows[1].event_seq, 4);
    }
}
