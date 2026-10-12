//! Durable, bounded, redacted human-shell promotion state.
//!
//! This store owns only model-target projections. Raw shell output remains in
//! its execution-local owner and never enters this table.

use sqlx::{Row, SqlitePool};
use thiserror::Error;

pub const HUMAN_SHELL_PROMOTION_MAX_BYTES: usize = 32 * 1024;
pub const HUMAN_SHELL_PROMOTION_MAX_PENDING: usize = 16;
pub const HUMAN_SHELL_PROMOTION_MAX_ROWS: usize = 64;
pub const HUMAN_SHELL_PROMOTION_TTL_MS: i64 = 60 * 60 * 1000;
pub const HUMAN_SHELL_PROMOTION_RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanShellPromotionState {
    Pending,
    Reserved,
    Consumed,
    Expired,
}

impl HumanShellPromotionState {
    fn parse(value: &str) -> Result<Self, HumanShellPromotionError> {
        match value {
            "pending" => Ok(Self::Pending),
            "reserved" => Ok(Self::Reserved),
            "consumed" => Ok(Self::Consumed),
            "expired" => Ok(Self::Expired),
            _ => Err(HumanShellPromotionError::CorruptState),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanShellPromotionInput {
    pub promotion_id: String,
    pub session_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub principal_id: String,
    pub source_command_id: String,
    pub projection: String,
    pub source_exit_code: Option<i32>,
    pub source_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanShellPromotion {
    pub promotion_id: String,
    pub session_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub principal_id: String,
    pub source_command_id: String,
    /// Present only while pending or reserved. Expired/consumed rows retain
    /// lifecycle metadata but clear the model-facing payload.
    pub projection: Option<String>,
    pub projection_bytes: usize,
    pub source_exit_code: Option<i32>,
    pub source_truncated: bool,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub state: HumanShellPromotionState,
    pub consumed_turn_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanShellPromotionIdempotency {
    Staged,
    Duplicate,
}

#[derive(Debug, Error)]
pub enum HumanShellPromotionError {
    #[error("invalid human-shell promotion input")]
    InvalidInput,
    #[error("promotion identity was reused with different content or scope")]
    IdentityConflict,
    #[error("promotion capacity is full for this session")]
    Capacity,
    #[error("promotion is missing, expired, or not pending")]
    Unavailable,
    #[error("promotion binding does not match this session authority")]
    StaleBinding,
    #[error("promotion store contains an unknown lifecycle state")]
    CorruptState,
    #[error("promotion storage failed: {0}")]
    Storage(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct HumanShellPromotionStore {
    pool: SqlitePool,
}

impl HumanShellPromotionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Stage one redacted projection. Reusing an ID with identical payload and
    /// authority is an idempotent duplicate; a different payload is rejected.
    pub async fn stage(
        &self,
        input: HumanShellPromotionInput,
        now_ms: i64,
    ) -> Result<(HumanShellPromotionIdempotency, HumanShellPromotion), HumanShellPromotionError>
    {
        validate_input(&input)?;
        let mut tx = self.pool.begin().await?;
        expire_and_prune(&mut tx, &input.session_id, now_ms).await?;

        if let Some(row) = sqlx::query("SELECT * FROM human_shell_promotion WHERE promotion_id = ?")
            .bind(&input.promotion_id)
            .fetch_optional(&mut *tx)
            .await?
        {
            let stored = decode_row(row)?;
            if !same_input(&stored, &input) {
                return Err(HumanShellPromotionError::IdentityConflict);
            }
            tx.commit().await?;
            return Ok((HumanShellPromotionIdempotency::Duplicate, stored));
        }

        let (row_count, pending_count): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN state IN ('pending','reserved') THEN 1 ELSE 0 END),0) \
             FROM human_shell_promotion WHERE session_id = ?",
        )
        .bind(&input.session_id)
        .fetch_one(&mut *tx)
        .await?;
        if pending_count as usize >= HUMAN_SHELL_PROMOTION_MAX_PENDING
            || row_count as usize >= HUMAN_SHELL_PROMOTION_MAX_ROWS
        {
            return Err(HumanShellPromotionError::Capacity);
        }

        let expires_at_ms = now_ms.saturating_add(HUMAN_SHELL_PROMOTION_TTL_MS);
        let projection_bytes = input.projection.len();
        sqlx::query(
            "INSERT INTO human_shell_promotion \
             (promotion_id,session_id,project_id,workspace_id,principal_id,source_command_id, \
              projection,projection_bytes,source_exit_code,source_truncated,created_at_ms, \
              expires_at_ms,state,consumed_turn_id) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?, 'pending', NULL)",
        )
        .bind(&input.promotion_id)
        .bind(&input.session_id)
        .bind(&input.project_id)
        .bind(&input.workspace_id)
        .bind(&input.principal_id)
        .bind(&input.source_command_id)
        .bind(&input.projection)
        .bind(projection_bytes as i64)
        .bind(input.source_exit_code)
        .bind(if input.source_truncated { 1_i64 } else { 0_i64 })
        .bind(now_ms)
        .bind(expires_at_ms)
        .execute(&mut *tx)
        .await?;

        let stored = sqlx::query("SELECT * FROM human_shell_promotion WHERE promotion_id = ?")
            .bind(&input.promotion_id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok((HumanShellPromotionIdempotency::Staged, decode_row(stored)?))
    }

    /// List bounded session state after attach/reconnect. Expired rows retain
    /// identity metadata for a finite window but never return their payload.
    pub async fn list(
        &self,
        session_id: &str,
        project_id: &str,
        workspace_id: &str,
        principal_id: &str,
        now_ms: i64,
    ) -> Result<Vec<HumanShellPromotion>, HumanShellPromotionError> {
        let mut tx = self.pool.begin().await?;
        expire_and_prune(&mut tx, session_id, now_ms).await?;
        let rows = sqlx::query(
            "SELECT * FROM human_shell_promotion \
             WHERE session_id = ? AND project_id = ? AND workspace_id = ? AND principal_id = ? \
             ORDER BY created_at_ms DESC LIMIT ?",
        )
        .bind(session_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(principal_id)
        .bind(HUMAN_SHELL_PROMOTION_MAX_ROWS as i64)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        rows.into_iter().map(decode_row).collect()
    }

    /// Reserve all requested records for one admitted turn. A single SQLite
    /// transaction prevents partial reservation and duplicate consumption.
    pub async fn reserve_for_turn(
        &self,
        session_id: &str,
        project_id: &str,
        workspace_id: &str,
        principal_id: &str,
        promotion_ids: &[String],
        turn_id: &str,
        now_ms: i64,
    ) -> Result<Vec<HumanShellPromotion>, HumanShellPromotionError> {
        if promotion_ids.len() > HUMAN_SHELL_PROMOTION_MAX_PENDING
            || promotion_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != promotion_ids.len()
        {
            return Err(HumanShellPromotionError::InvalidInput);
        }
        if promotion_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.pool.begin().await?;
        expire_and_prune(&mut tx, session_id, now_ms).await?;
        let mut records = Vec::with_capacity(promotion_ids.len());
        for id in promotion_ids {
            let row = sqlx::query("SELECT * FROM human_shell_promotion WHERE promotion_id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(HumanShellPromotionError::Unavailable)?;
            let record = decode_row(row)?;
            if record.session_id != session_id
                || record.project_id != project_id
                || record.workspace_id != workspace_id
                || record.principal_id != principal_id
            {
                return Err(HumanShellPromotionError::StaleBinding);
            }
            if record.state != HumanShellPromotionState::Pending || record.expires_at_ms <= now_ms {
                return Err(HumanShellPromotionError::Unavailable);
            }
            records.push(record);
        }
        for id in promotion_ids {
            let result = sqlx::query(
                "UPDATE human_shell_promotion SET state = 'reserved', consumed_turn_id = ? \
                 WHERE promotion_id = ? AND state = 'pending'",
            )
            .bind(turn_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
            if result.rows_affected() != 1 {
                return Err(HumanShellPromotionError::Unavailable);
            }
        }
        tx.commit().await?;
        Ok(records)
    }

    /// Complete reservations after the turn runtime accepted the input.
    pub async fn consume_for_turn(
        &self,
        promotion_ids: &[String],
        turn_id: &str,
    ) -> Result<(), HumanShellPromotionError> {
        self.transition_reservations(promotion_ids, turn_id, "consumed")
            .await
    }

    /// Return reservations to pending when the turn fails before runtime
    /// acceptance. On daemon restart, all old reservations are similarly
    /// restored because active turns are process-local and do not survive.
    pub async fn release_for_turn(
        &self,
        promotion_ids: &[String],
        turn_id: &str,
    ) -> Result<(), HumanShellPromotionError> {
        self.transition_reservations(promotion_ids, turn_id, "pending")
            .await
    }

    pub async fn recover_inflight(&self) -> Result<u64, HumanShellPromotionError> {
        let result = sqlx::query(
            "UPDATE human_shell_promotion SET state = 'pending', consumed_turn_id = NULL \
             WHERE state = 'reserved'",
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Release reservations left by a process-local turn that is no longer
    /// active. A live matching turn keeps its reservations hidden from reuse.
    pub async fn recover_orphaned_for_session(
        &self,
        session_id: &str,
        active_turn_id: Option<&str>,
    ) -> Result<u64, HumanShellPromotionError> {
        let result = if let Some(active_turn_id) = active_turn_id {
            sqlx::query(
                "UPDATE human_shell_promotion SET state = 'pending', consumed_turn_id = NULL \
                 WHERE session_id = ? AND state = 'reserved' AND consumed_turn_id <> ?",
            )
            .bind(session_id)
            .bind(active_turn_id)
            .execute(&self.pool)
            .await?
        } else {
            sqlx::query(
                "UPDATE human_shell_promotion SET state = 'pending', consumed_turn_id = NULL \
                 WHERE session_id = ? AND state = 'reserved'",
            )
            .bind(session_id)
            .execute(&self.pool)
            .await?
        };
        Ok(result.rows_affected())
    }

    async fn transition_reservations(
        &self,
        promotion_ids: &[String],
        turn_id: &str,
        state: &str,
    ) -> Result<(), HumanShellPromotionError> {
        if promotion_ids.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for id in promotion_ids {
            let (next_state, consumed_turn_id) = match state {
                "consumed" => ("consumed", Some(turn_id)),
                "pending" => ("pending", None),
                _ => return Err(HumanShellPromotionError::CorruptState),
            };
            let result = sqlx::query(
                "UPDATE human_shell_promotion SET state = ?, consumed_turn_id = ?, \
                 projection = CASE WHEN ? = 'consumed' THEN '' ELSE projection END, \
                 projection_bytes = CASE WHEN ? = 'consumed' THEN 0 ELSE projection_bytes END \
                 WHERE promotion_id = ? AND state = 'reserved' AND consumed_turn_id = ?",
            )
            .bind(next_state)
            .bind(consumed_turn_id)
            .bind(next_state)
            .bind(next_state)
            .bind(id)
            .bind(turn_id)
            .execute(&mut *tx)
            .await?;
            if result.rows_affected() != 1 {
                return Err(HumanShellPromotionError::Unavailable);
            }
        }
        tx.commit().await?;
        Ok(())
    }
}

async fn expire_and_prune(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session_id: &str,
    now_ms: i64,
) -> Result<(), HumanShellPromotionError> {
    sqlx::query(
        "UPDATE human_shell_promotion SET state = 'expired', projection = '', projection_bytes = 0 \
         WHERE session_id = ? AND state = 'pending' AND expires_at_ms <= ?",
    )
    .bind(session_id)
    .bind(now_ms)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "DELETE FROM human_shell_promotion WHERE session_id = ? \
         AND state IN ('consumed','expired') AND created_at_ms < ?",
    )
    .bind(session_id)
    .bind(now_ms.saturating_sub(HUMAN_SHELL_PROMOTION_RETENTION_MS))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn validate_input(input: &HumanShellPromotionInput) -> Result<(), HumanShellPromotionError> {
    if input.promotion_id.is_empty()
        || input.promotion_id.len() > 128
        || input.source_command_id.is_empty()
        || input.source_command_id.len() > 64
        || input.session_id.is_empty()
        || input.project_id.is_empty()
        || input.workspace_id.is_empty()
        || input.principal_id.is_empty()
        || input.projection.is_empty()
        || input.projection.len() > HUMAN_SHELL_PROMOTION_MAX_BYTES
    {
        return Err(HumanShellPromotionError::InvalidInput);
    }
    Ok(())
}

fn same_input(record: &HumanShellPromotion, input: &HumanShellPromotionInput) -> bool {
    record.session_id == input.session_id
        && record.project_id == input.project_id
        && record.workspace_id == input.workspace_id
        && record.principal_id == input.principal_id
        && record.source_command_id == input.source_command_id
        && record.projection.as_deref() == Some(input.projection.as_str())
        && record.source_exit_code == input.source_exit_code
        && record.source_truncated == input.source_truncated
}

fn decode_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<HumanShellPromotion, HumanShellPromotionError> {
    let state: String = row.try_get("state")?;
    let projection: String = row.try_get("projection")?;
    let projection_bytes: i64 = row.try_get("projection_bytes")?;
    Ok(HumanShellPromotion {
        promotion_id: row.try_get("promotion_id")?,
        session_id: row.try_get("session_id")?,
        project_id: row.try_get("project_id")?,
        workspace_id: row.try_get("workspace_id")?,
        principal_id: row.try_get("principal_id")?,
        source_command_id: row.try_get("source_command_id")?,
        projection: (projection_bytes > 0).then_some(projection),
        projection_bytes: projection_bytes.max(0) as usize,
        source_exit_code: row.try_get("source_exit_code")?,
        source_truncated: row.try_get::<i64, _>("source_truncated")? != 0,
        created_at_ms: row.try_get("created_at_ms")?,
        expires_at_ms: row.try_get("expires_at_ms")?,
        state: HumanShellPromotionState::parse(&state)?,
        consumed_turn_id: row.try_get("consumed_turn_id")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::init_pool_at;
    use tempfile::tempdir;

    async fn store() -> (tempfile::TempDir, HumanShellPromotionStore) {
        let dir = tempdir().expect("storage dir");
        let pool = init_pool_at(&dir.path().join("session.db"))
            .await
            .expect("pool");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrations");
        sqlx::query("INSERT INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES ('p','/workspace','[]',1,1)")
            .execute(&pool).await.expect("project fixture");
        sqlx::query("INSERT INTO session (id, project_id, workspace_id, slug, directory, title, time_created, time_updated, version) VALUES ('s','p','w','test','/workspace','test',1,1,'1')")
            .execute(&pool).await.expect("session fixture");
        (dir, HumanShellPromotionStore::new(pool))
    }

    fn input(id: &str) -> HumanShellPromotionInput {
        HumanShellPromotionInput {
            promotion_id: id.to_owned(),
            session_id: "s".to_owned(),
            project_id: "p".to_owned(),
            workspace_id: "w".to_owned(),
            principal_id: "local-owner".to_owned(),
            source_command_id: "12".to_owned(),
            projection: "redacted projection".to_owned(),
            source_exit_code: Some(0),
            source_truncated: false,
        }
    }

    #[tokio::test]
    async fn stage_is_idempotent_and_conflicting_reuse_is_rejected() {
        let (_dir, store) = store().await;
        let (first, _) = store.stage(input("promotion-1"), 100).await.expect("stage");
        let (duplicate, _) = store.stage(input("promotion-1"), 200).await.expect("retry");
        assert_eq!(first, HumanShellPromotionIdempotency::Staged);
        assert_eq!(duplicate, HumanShellPromotionIdempotency::Duplicate);

        let mut conflict = input("promotion-1");
        conflict.projection.push_str(" changed");
        assert!(matches!(
            store.stage(conflict, 300).await,
            Err(HumanShellPromotionError::IdentityConflict)
        ));
    }

    #[tokio::test]
    async fn concurrent_stage_and_consume_attempts_have_one_winner() {
        let (_dir, store) = store().await;
        let now = 2_000_000;
        let (left, right) = tokio::join!(
            store.stage(input("concurrent"), now),
            store.stage(input("concurrent"), now),
        );
        let outcomes = [left.unwrap().0, right.unwrap().0];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == HumanShellPromotionIdempotency::Staged)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == HumanShellPromotionIdempotency::Duplicate)
                .count(),
            1
        );

        store
            .reserve_for_turn(
                "s",
                "p",
                "w",
                "local-owner",
                &["concurrent".into()],
                "turn-c",
                now + 1,
            )
            .await
            .unwrap();
        let promotion_ids = vec!["concurrent".to_string()];
        let (left, right) = tokio::join!(
            store.consume_for_turn(&promotion_ids, "turn-c"),
            store.consume_for_turn(&promotion_ids, "turn-c"),
        );
        assert_ne!(left.is_ok(), right.is_ok());
        assert!(matches!(
            left.err().or_else(|| right.err()),
            Some(HumanShellPromotionError::Unavailable)
        ));
    }

    #[tokio::test]
    async fn reservations_are_atomic_scoped_and_consumed_once() {
        let (_dir, store) = store().await;
        store.stage(input("promotion-1"), 100).await.expect("stage");
        let records = store
            .reserve_for_turn(
                "s",
                "p",
                "w",
                "local-owner",
                &["promotion-1".into()],
                "turn-1",
                200,
            )
            .await
            .expect("reserve");
        assert_eq!(
            records[0].projection.as_deref(),
            Some("redacted projection")
        );
        assert!(matches!(
            store
                .reserve_for_turn(
                    "s",
                    "p",
                    "w",
                    "other",
                    &["promotion-1".into()],
                    "turn-2",
                    201
                )
                .await,
            Err(HumanShellPromotionError::StaleBinding)
        ));
        store
            .consume_for_turn(&["promotion-1".into()], "turn-1")
            .await
            .expect("consume");
        assert!(matches!(
            store
                .reserve_for_turn(
                    "s",
                    "p",
                    "w",
                    "local-owner",
                    &["promotion-1".into()],
                    "turn-2",
                    202
                )
                .await,
            Err(HumanShellPromotionError::Unavailable)
        ));
        let listed = store
            .list("s", "p", "w", "local-owner", 203)
            .await
            .expect("list");
        assert_eq!(listed[0].state, HumanShellPromotionState::Consumed);
        assert!(listed[0].projection.is_none());
        assert_eq!(listed[0].consumed_turn_id.as_deref(), Some("turn-1"));
    }

    #[tokio::test]
    async fn expiration_clears_payload_and_restart_releases_inflight_reservation() {
        let (_dir, store) = store().await;
        store.stage(input("promotion-1"), 100).await.expect("stage");
        store
            .reserve_for_turn(
                "s",
                "p",
                "w",
                "local-owner",
                &["promotion-1".into()],
                "turn-1",
                200,
            )
            .await
            .expect("reserve");
        assert_eq!(store.recover_inflight().await.expect("recover"), 1);
        let pending = store
            .list("s", "p", "w", "local-owner", 300)
            .await
            .expect("list");
        assert_eq!(pending[0].state, HumanShellPromotionState::Pending);
        let expired = store
            .list(
                "s",
                "p",
                "w",
                "local-owner",
                100 + HUMAN_SHELL_PROMOTION_TTL_MS + 1,
            )
            .await
            .expect("expire");
        assert_eq!(expired[0].state, HumanShellPromotionState::Expired);
        assert!(expired[0].projection.is_none());
    }
}
