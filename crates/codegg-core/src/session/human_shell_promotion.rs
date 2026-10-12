//! Durable, bounded ownership for explicitly promoted human-shell projections.
//!
//! Only the already-redacted model-context projection is stored here. Raw shell
//! output remains in its existing ephemeral owner.

use sqlx::{FromRow, SqlitePool};
use thiserror::Error;

use crate::error::StorageError;

pub const HUMAN_SHELL_PROMOTION_MAX_BYTES: usize = 32 * 1024;
pub const HUMAN_SHELL_PROMOTION_MAX_PENDING: i64 = 32;
pub const HUMAN_SHELL_PROMOTION_TTL_MS: i64 = 24 * 60 * 60 * 1000;
pub const HUMAN_SHELL_PROMOTION_MAX_RETAINED_PER_SESSION: i64 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanShellPromotionStatus {
    Pending,
    Consumed,
    Expired,
}

impl HumanShellPromotionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Consumed => "consumed",
            Self::Expired => "expired",
        }
    }

    fn parse(value: &str) -> Result<Self, HumanShellPromotionError> {
        match value {
            "pending" => Ok(Self::Pending),
            "consumed" => Ok(Self::Consumed),
            "expired" => Ok(Self::Expired),
            _ => Err(HumanShellPromotionError::Invalid {
                field: "status",
                message: "unknown stored status".to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHumanShellPromotion {
    pub id: String,
    pub source_id: String,
    pub session_id: String,
    pub principal_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub projection: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanShellPromotionRecord {
    pub id: String,
    pub source_id: String,
    pub session_id: String,
    pub principal_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub projection: String,
    pub projection_bytes: usize,
    pub projection_sha256: String,
    pub status: HumanShellPromotionStatus,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub consumed_turn_id: Option<String>,
    pub consumed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageHumanShellPromotionOutcome {
    Staged(HumanShellPromotionRecord),
    Duplicate(HumanShellPromotionRecord),
}

#[derive(Debug, Error)]
pub enum HumanShellPromotionError {
    #[error("invalid human-shell promotion {field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
    #[error("human-shell promotion identity conflicts with an existing record")]
    IdentityConflict,
    #[error("human-shell promotion queue is full")]
    QueueFull,
    #[error("human-shell promotion was not found for this caller")]
    NotFound,
    #[error("human-shell promotion has expired")]
    Expired,
    #[error("human-shell promotion is already consumed")]
    AlreadyConsumed,
    #[error("human-shell promotion is bound to a stale session workspace")]
    StaleBinding,
    #[error("human-shell promotion is unavailable: {0}")]
    Unavailable(String),
    #[error("human-shell promotion storage error: {0}")]
    Storage(#[from] StorageError),
}

impl HumanShellPromotionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid { .. } => "human_shell_promotion_invalid",
            Self::IdentityConflict => "human_shell_promotion_identity_conflict",
            Self::QueueFull => "human_shell_promotion_queue_full",
            Self::NotFound => "human_shell_promotion_not_found",
            Self::Expired => "human_shell_promotion_expired",
            Self::AlreadyConsumed => "human_shell_promotion_consumed",
            Self::StaleBinding => "human_shell_promotion_stale_binding",
            Self::Unavailable(_) | Self::Storage(_) => "human_shell_promotion_unavailable",
        }
    }
}

#[derive(Debug, FromRow)]
struct PromotionRow {
    id: String,
    source_id: String,
    session_id: String,
    principal_id: String,
    project_id: String,
    workspace_id: String,
    projection: String,
    projection_bytes: i64,
    projection_sha256: String,
    status: String,
    created_at_ms: i64,
    expires_at_ms: i64,
    consumed_turn_id: Option<String>,
    consumed_at_ms: Option<i64>,
}

impl TryFrom<PromotionRow> for HumanShellPromotionRecord {
    type Error = HumanShellPromotionError;

    fn try_from(row: PromotionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            source_id: row.source_id,
            session_id: row.session_id,
            principal_id: row.principal_id,
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            projection: row.projection,
            projection_bytes: usize::try_from(row.projection_bytes).map_err(|_| {
                HumanShellPromotionError::Invalid {
                    field: "projection_bytes",
                    message: "stored byte count is negative".to_owned(),
                }
            })?,
            projection_sha256: row.projection_sha256,
            status: HumanShellPromotionStatus::parse(&row.status)?,
            created_at_ms: row.created_at_ms,
            expires_at_ms: row.expires_at_ms,
            consumed_turn_id: row.consumed_turn_id,
            consumed_at_ms: row.consumed_at_ms,
        })
    }
}

/// Session-owned store for bounded redacted human-shell projections.
#[derive(Debug, Clone)]
pub struct HumanShellPromotionStore {
    pool: SqlitePool,
}

impl HumanShellPromotionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Persist an idempotently identified projection for the authenticated
    /// session principal. Reusing an identity with different bytes or scope
    /// fails closed.
    pub async fn stage(
        &self,
        input: NewHumanShellPromotion,
    ) -> Result<StageHumanShellPromotionOutcome, HumanShellPromotionError> {
        validate_input(&input)?;
        let projection_bytes = input.projection.len();
        let digest = sha256_hex(input.projection.as_bytes());
        let mut tx = self.pool.begin().await.map_err(storage_error)?;

        sqlx::query(
            "UPDATE human_shell_promotion SET status = 'expired' \
             WHERE status = 'pending' AND expires_at_ms <= ?",
        )
        .bind(input.created_at_ms)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
        sqlx::query(
            "UPDATE human_shell_promotion SET status = 'expired' \
             WHERE session_id = ? AND principal_id = ? AND status = 'pending' \
             AND (project_id != ? OR workspace_id != ?)",
        )
        .bind(&input.session_id)
        .bind(&input.principal_id)
        .bind(&input.project_id)
        .bind(&input.workspace_id)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;

        // Keep idempotency tombstones for one additional TTL, then remove old
        // payloads. The retained-row cap bounds storage even under rapid
        // stage/consume cycles within that window.
        sqlx::query("DELETE FROM human_shell_promotion WHERE expires_at_ms <= ?")
            .bind(
                input
                    .created_at_ms
                    .saturating_sub(HUMAN_SHELL_PROMOTION_TTL_MS),
            )
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        let existing: Option<PromotionRow> = sqlx::query_as(
            "SELECT id, source_id, session_id, principal_id, project_id, workspace_id, \
             projection, projection_bytes, projection_sha256, status, created_at_ms, \
             expires_at_ms, consumed_turn_id, consumed_at_ms \
             FROM human_shell_promotion WHERE id = ?",
        )
        .bind(&input.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(storage_error)?;
        if let Some(existing) = existing {
            let existing = HumanShellPromotionRecord::try_from(existing)?;
            let same = existing.source_id == input.source_id
                && existing.session_id == input.session_id
                && existing.principal_id == input.principal_id
                && existing.project_id == input.project_id
                && existing.workspace_id == input.workspace_id
                && existing.projection == input.projection
                && existing.projection_sha256 == digest;
            if !same {
                return Err(HumanShellPromotionError::IdentityConflict);
            }
            tx.rollback().await.map_err(storage_error)?;
            return Ok(StageHumanShellPromotionOutcome::Duplicate(existing));
        }

        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM human_shell_promotion \
             WHERE session_id = ? AND principal_id = ? AND status = 'pending' \
             AND expires_at_ms > ?",
        )
        .bind(&input.session_id)
        .bind(&input.principal_id)
        .bind(input.created_at_ms)
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
        if pending >= HUMAN_SHELL_PROMOTION_MAX_PENDING {
            return Err(HumanShellPromotionError::QueueFull);
        }
        let retained: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM human_shell_promotion \
             WHERE session_id = ? AND principal_id = ?",
        )
        .bind(&input.session_id)
        .bind(&input.principal_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
        if retained >= HUMAN_SHELL_PROMOTION_MAX_RETAINED_PER_SESSION {
            return Err(HumanShellPromotionError::QueueFull);
        }

        let expires_at_ms = input
            .created_at_ms
            .checked_add(HUMAN_SHELL_PROMOTION_TTL_MS)
            .ok_or_else(|| HumanShellPromotionError::Invalid {
                field: "created_at_ms",
                message: "expiry overflows timestamp range".to_owned(),
            })?;
        sqlx::query(
            "INSERT INTO human_shell_promotion \
             (id, source_id, session_id, principal_id, project_id, workspace_id, projection, \
              projection_bytes, projection_sha256, status, created_at_ms, expires_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
        )
        .bind(&input.id)
        .bind(&input.source_id)
        .bind(&input.session_id)
        .bind(&input.principal_id)
        .bind(&input.project_id)
        .bind(&input.workspace_id)
        .bind(&input.projection)
        .bind(
            i64::try_from(projection_bytes).map_err(|_| HumanShellPromotionError::Invalid {
                field: "projection",
                message: "byte count overflows storage range".to_owned(),
            })?,
        )
        .bind(&digest)
        .bind(input.created_at_ms)
        .bind(expires_at_ms)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;

        Ok(StageHumanShellPromotionOutcome::Staged(
            HumanShellPromotionRecord {
                id: input.id,
                source_id: input.source_id,
                session_id: input.session_id,
                principal_id: input.principal_id,
                project_id: input.project_id,
                workspace_id: input.workspace_id,
                projection: input.projection,
                projection_bytes,
                projection_sha256: digest,
                status: HumanShellPromotionStatus::Pending,
                created_at_ms: input.created_at_ms,
                expires_at_ms,
                consumed_turn_id: None,
                consumed_at_ms: None,
            },
        ))
    }

    /// List only live pending rows owned by this exact principal and bound to
    /// the current project/workspace generation.
    pub async fn list_pending(
        &self,
        session_id: &str,
        principal_id: &str,
        project_id: &str,
        workspace_id: &str,
        now_ms: i64,
    ) -> Result<Vec<HumanShellPromotionRecord>, HumanShellPromotionError> {
        sqlx::query("DELETE FROM human_shell_promotion WHERE expires_at_ms <= ?")
            .bind(now_ms.saturating_sub(HUMAN_SHELL_PROMOTION_TTL_MS))
            .execute(&self.pool)
            .await
            .map_err(storage_error)?;
        sqlx::query(
            "UPDATE human_shell_promotion SET status = 'expired' \
             WHERE session_id = ? AND principal_id = ? AND status = 'pending' \
             AND (project_id != ? OR workspace_id != ?)",
        )
        .bind(session_id)
        .bind(principal_id)
        .bind(project_id)
        .bind(workspace_id)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        sqlx::query(
            "UPDATE human_shell_promotion SET status = 'expired' \
             WHERE session_id = ? AND principal_id = ? AND status = 'pending' \
             AND expires_at_ms <= ?",
        )
        .bind(session_id)
        .bind(principal_id)
        .bind(now_ms)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        let rows: Vec<PromotionRow> = sqlx::query_as(
            "SELECT id, source_id, session_id, principal_id, project_id, workspace_id, \
             projection, projection_bytes, projection_sha256, status, created_at_ms, \
             expires_at_ms, consumed_turn_id, consumed_at_ms \
             FROM human_shell_promotion WHERE session_id = ? AND principal_id = ? \
             AND project_id = ? AND workspace_id = ? AND status = 'pending' \
             AND expires_at_ms > ? ORDER BY created_at_ms, id LIMIT ?",
        )
        .bind(session_id)
        .bind(principal_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(now_ms)
        .bind(HUMAN_SHELL_PROMOTION_MAX_PENDING)
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        rows.into_iter()
            .map(HumanShellPromotionRecord::try_from)
            .collect()
    }
}

/// Claim pending promotions inside the same transaction that acquires the
/// durable session-turn controller. On any error, the caller's transaction
/// must be rolled back so all selected rows remain pending.
pub(crate) async fn claim_for_turn(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    promotion_ids: &[String],
    session_id: &str,
    principal_id: &str,
    project_id: &str,
    workspace_id: &str,
    turn_id: &str,
    now_ms: i64,
) -> Result<Vec<HumanShellPromotionRecord>, HumanShellPromotionError> {
    let mut unique = std::collections::HashSet::with_capacity(promotion_ids.len());
    let mut claimed = Vec::with_capacity(promotion_ids.len());
    for id in promotion_ids {
        if id.is_empty() || id.len() > 64 || !unique.insert(id.as_str()) {
            return Err(HumanShellPromotionError::Invalid {
                field: "promotion_ids",
                message: "promotion identities must be unique and bounded".to_owned(),
            });
        }
        let row: Option<PromotionRow> = sqlx::query_as(
            "SELECT id, source_id, session_id, principal_id, project_id, workspace_id, \
             projection, projection_bytes, projection_sha256, status, created_at_ms, \
             expires_at_ms, consumed_turn_id, consumed_at_ms \
             FROM human_shell_promotion WHERE id = ? AND session_id = ? AND principal_id = ?",
        )
        .bind(id)
        .bind(session_id)
        .bind(principal_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            return Err(HumanShellPromotionError::NotFound);
        };
        let record = HumanShellPromotionRecord::try_from(row)?;
        if record.project_id != project_id || record.workspace_id != workspace_id {
            return Err(HumanShellPromotionError::StaleBinding);
        }
        if record.status == HumanShellPromotionStatus::Expired || record.expires_at_ms <= now_ms {
            return Err(HumanShellPromotionError::Expired);
        }
        if record.status != HumanShellPromotionStatus::Pending {
            return Err(HumanShellPromotionError::AlreadyConsumed);
        }
        let result = sqlx::query(
            "UPDATE human_shell_promotion SET status = 'consumed', consumed_turn_id = ?, \
             consumed_at_ms = ? WHERE id = ? AND status = 'pending' AND expires_at_ms > ?",
        )
        .bind(turn_id)
        .bind(now_ms)
        .bind(id)
        .bind(now_ms)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
        if result.rows_affected() != 1 {
            return Err(HumanShellPromotionError::AlreadyConsumed);
        }
        claimed.push(HumanShellPromotionRecord {
            status: HumanShellPromotionStatus::Consumed,
            consumed_turn_id: Some(turn_id.to_owned()),
            consumed_at_ms: Some(now_ms),
            ..record
        });
    }
    Ok(claimed)
}

fn validate_input(input: &NewHumanShellPromotion) -> Result<(), HumanShellPromotionError> {
    for (field, value, max) in [
        ("id", input.id.as_str(), 64),
        ("source_id", input.source_id.as_str(), 128),
        ("session_id", input.session_id.as_str(), 128),
        ("principal_id", input.principal_id.as_str(), 128),
        ("project_id", input.project_id.as_str(), 128),
        ("workspace_id", input.workspace_id.as_str(), 128),
    ] {
        if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
            return Err(HumanShellPromotionError::Invalid {
                field,
                message: "value is empty, too long, or contains control characters".to_owned(),
            });
        }
    }
    if input.projection.trim().is_empty() {
        return Err(HumanShellPromotionError::Invalid {
            field: "projection",
            message: "projection must not be empty".to_owned(),
        });
    }
    if input.projection.len() > HUMAN_SHELL_PROMOTION_MAX_BYTES {
        return Err(HumanShellPromotionError::Invalid {
            field: "projection",
            message: format!("projection exceeds {HUMAN_SHELL_PROMOTION_MAX_BYTES} bytes"),
        });
    }
    if input.created_at_ms < 0 {
        return Err(HumanShellPromotionError::Invalid {
            field: "created_at_ms",
            message: "timestamp must be nonnegative".to_owned(),
        });
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn storage_error(error: sqlx::Error) -> HumanShellPromotionError {
    HumanShellPromotionError::Storage(StorageError::Database(error.to_string()))
}
