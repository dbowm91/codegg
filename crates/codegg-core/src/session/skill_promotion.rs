//! M007-I (work package B): the durable skill-promotion record.
//!
//! The promotion workflow used to be two writers of one JSON file, held
//! together by an `flock` sidecar. This module is the **one** writer: the
//! daemon owns the durable record, and every mutation happens inside the
//! daemon's session transaction.
//!
//! Two properties are load-bearing:
//!
//! * **The namespace is the canonical `WorkspaceId`.** The old namespace was
//!   a SHA-256 of a *path string*, derived differently on each side (the TUI
//!   hashed the workspace root; the daemon-side tool hashed the tool's
//!   `cwd`). The two derivations disagreed, and the disagreement surfaced as
//!   "promotion request not found". A `WorkspaceId` is one identity, so the
//!   divergence is closed by construction.
//! * **The record is bounded.** Every payload carries a byte bound and a
//!   digest, and the retained-row count is capped, so a store cannot grow
//!   without limit.
//!
//! The stored payload is **opaque** to this module: it is the workflow's own
//! serialized state, and this module neither parses nor interprets it. The
//! rules live with the workflow; the durability lives here. This is the
//! `HumanShellPromotionStore` shape.

use sqlx::{FromRow, SqlitePool};
use thiserror::Error;

/// The largest one stored payload may be, in bytes.
///
/// The proposal body is the model's own `SKILL.md` draft, so the bound is
/// the one the file-backed store used: a proposal that was writable before
/// is writable now.
pub const SKILL_PROMOTION_MAX_PAYLOAD_BYTES: usize = 256 * 1024;

/// The largest retained records one workspace may hold.
pub const SKILL_PROMOTION_MAX_RECORDS_PER_WORKSPACE: i64 = 64;

/// What a stored record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillPromotionKind {
    /// A staged request: the human asked for a draft.
    Request,
    /// A proposal: the model drafted one and the daemon validated it.
    Proposal,
}

impl SkillPromotionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Proposal => "proposal",
        }
    }

    fn parse(value: &str) -> Result<Self, SkillPromotionError> {
        match value {
            "request" => Ok(Self::Request),
            "proposal" => Ok(Self::Proposal),
            _ => Err(SkillPromotionError::Invalid {
                field: "kind",
                message: "unknown stored record kind".to_owned(),
            }),
        }
    }
}

/// One stored record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillPromotionRecord {
    /// The record's own identity.
    pub id: String,
    /// The canonical workspace the record belongs to.
    pub workspace_id: String,
    /// The session the record belongs to.
    pub session_id: String,
    /// What the record is.
    pub kind: SkillPromotionKind,
    /// The workflow's own serialized state.
    pub payload: String,
    /// The digest of the payload.
    pub payload_sha256: String,
    /// The record's status, in the workflow's own vocabulary.
    pub status: String,
    /// The record's current revision.
    pub revision: i64,
    /// When the record was created.
    pub created_at_ms: i64,
    /// When the record last changed.
    pub updated_at_ms: i64,
    /// When the record expires, when it is one that expires.
    pub expires_at_ms: Option<i64>,
}

/// Why a promotion write was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SkillPromotionError {
    #[error("promotion payload exceeds the size bound")]
    PayloadTooLarge,
    #[error("promotion capacity reached for this workspace")]
    CapacityReached,
    #[error("the promotion request was not found")]
    RequestNotFound,
    #[error("the promotion request has already been consumed")]
    RequestConsumed,
    #[error("the promotion request has expired")]
    RequestExpired,
    #[error("invalid promotion field: {field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
    #[error("promotion storage is unavailable: {0}")]
    Storage(String),
}

fn storage_error(error: sqlx::Error) -> SkillPromotionError {
    SkillPromotionError::Storage(error.to_string())
}

/// One record's next state.
///
/// The status and the payload are written **together**, because the
/// workflow's own state carries the same status as the indexed column.
#[derive(Debug, Clone, Copy)]
pub struct SkillPromotionUpdate<'a> {
    /// The record's new status.
    pub status: &'a str,
    /// The record's new payload.
    pub payload: &'a str,
    /// The record's new revision.
    pub next_revision: i64,
    /// When the record was written, in epoch milliseconds.
    pub now_ms: i64,
}

/// One stored row.
#[derive(Debug, Clone, FromRow)]
struct PromotionRow {
    id: String,
    workspace_id: String,
    session_id: String,
    kind: String,
    payload: String,
    payload_sha256: String,
    status: String,
    revision: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    expires_at_ms: Option<i64>,
}

const SELECT_COLUMNS: &str = "id, workspace_id, session_id, kind, payload, \
     payload_sha256, status, revision, created_at_ms, updated_at_ms, expires_at_ms";

/// The daemon's durable promotion store.
///
/// Every mutation happens here, inside the daemon's session database. There
/// is no sidecar lock and no second writer: the transaction *is* the
/// coordination.
#[derive(Debug, Clone)]
pub struct SkillPromotionStore {
    pool: SqlitePool,
}

impl SkillPromotionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Write one record, replacing any record with the same identity.
    ///
    /// The identity is `(workspace_id, kind, id)`, so one workspace's
    /// records never collide with another's.
    pub async fn put(
        &self,
        record: SkillPromotionRecord,
    ) -> Result<SkillPromotionRecord, SkillPromotionError> {
        validate_record(&record)?;
        let payload_bytes = record.payload.len();
        if payload_bytes > SKILL_PROMOTION_MAX_PAYLOAD_BYTES {
            return Err(SkillPromotionError::PayloadTooLarge);
        }
        let digest = crate::audit::sha256_hex(record.payload.as_bytes());
        let mut tx = self.pool.begin().await.map_err(storage_error)?;

        // Expire this workspace's stale expiring records, then enforce the
        // retained-row cap. The cap is per workspace, so one busy workspace
        // cannot crowd out another's records.
        sqlx::query(
            "UPDATE skill_promotion SET status = 'expired' \
             WHERE workspace_id = ? AND status = 'pending' AND expires_at_ms IS NOT NULL \
             AND expires_at_ms <= ?",
        )
        .bind(&record.workspace_id)
        .bind(record.updated_at_ms)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
        let retained: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM skill_promotion WHERE workspace_id = ?")
                .bind(&record.workspace_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(storage_error)?;
        if retained >= SKILL_PROMOTION_MAX_RECORDS_PER_WORKSPACE {
            tx.rollback().await.map_err(storage_error)?;
            return Err(SkillPromotionError::CapacityReached);
        }

        sqlx::query(
            "INSERT OR REPLACE INTO skill_promotion (id, workspace_id, session_id, kind, \
             payload, payload_bytes, payload_sha256, status, revision, created_at_ms, \
             updated_at_ms, expires_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&record.id)
        .bind(&record.workspace_id)
        .bind(&record.session_id)
        .bind(record.kind.as_str())
        .bind(&record.payload)
        .bind(payload_bytes as i64)
        .bind(&digest)
        .bind(&record.status)
        .bind(record.revision)
        .bind(record.created_at_ms)
        .bind(record.updated_at_ms)
        .bind(record.expires_at_ms)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;

        tx.commit().await.map_err(storage_error)?;
        Ok(SkillPromotionRecord {
            payload_sha256: digest,
            ..record
        })
    }

    /// Read one record.
    pub async fn get(
        &self,
        workspace_id: &str,
        kind: SkillPromotionKind,
        id: &str,
    ) -> Result<Option<SkillPromotionRecord>, SkillPromotionError> {
        let row = sqlx::query_as::<_, PromotionRow>(&format!(
            "SELECT {SELECT_COLUMNS} FROM skill_promotion \
             WHERE workspace_id = ? AND kind = ? AND id = ?"
        ))
        .bind(workspace_id)
        .bind(kind.as_str())
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        row.map(record_from_row).transpose()
    }

    /// The workspace's records of one kind, newest first.
    pub async fn list(
        &self,
        workspace_id: &str,
        kind: SkillPromotionKind,
        limit: usize,
    ) -> Result<Vec<SkillPromotionRecord>, SkillPromotionError> {
        let rows = sqlx::query_as::<_, PromotionRow>(&format!(
            "SELECT {SELECT_COLUMNS} FROM skill_promotion \
             WHERE workspace_id = ? AND kind = ? \
             ORDER BY created_at_ms DESC, id ASC LIMIT ?"
        ))
        .bind(workspace_id)
        .bind(kind.as_str())
        .bind(limit.min(SKILL_PROMOTION_MAX_RECORDS_PER_WORKSPACE as usize) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        rows.into_iter().map(record_from_row).collect()
    }

    /// Write one record's status **and** payload, when it still holds the
    /// expected revision.
    ///
    /// The expected revision is the compare-and-swap: a caller that read a
    /// record at revision *n* may only write it while it is still at *n*.
    ///
    /// The status and the payload are written **together**, because the
    /// workflow's own state carries the same status as the indexed column.
    /// Writing one without the other would let the two disagree, and a
    /// disagreement is a record that reads one way and behaves another.
    pub async fn compare_and_set_record(
        &self,
        workspace_id: &str,
        kind: SkillPromotionKind,
        id: &str,
        expected_revision: i64,
        update: SkillPromotionUpdate<'_>,
    ) -> Result<bool, SkillPromotionError> {
        let SkillPromotionUpdate {
            status,
            payload,
            next_revision,
            now_ms,
        } = update;
        let payload_bytes = payload.len();
        if payload_bytes > SKILL_PROMOTION_MAX_PAYLOAD_BYTES {
            return Err(SkillPromotionError::PayloadTooLarge);
        }
        let digest = crate::audit::sha256_hex(payload.as_bytes());
        let updated = sqlx::query(
            "UPDATE skill_promotion SET payload = ?, payload_bytes = ?, payload_sha256 = ?, \
             status = ?, revision = ?, updated_at_ms = ? \
             WHERE workspace_id = ? AND kind = ? AND id = ? AND revision = ?",
        )
        .bind(payload)
        .bind(payload_bytes as i64)
        .bind(&digest)
        .bind(status)
        .bind(next_revision)
        .bind(now_ms)
        .bind(workspace_id)
        .bind(kind.as_str())
        .bind(id)
        .bind(expected_revision)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?
        .rows_affected();
        Ok(updated > 0)
    }

    /// Consume one staged request and record the proposal it produced.
    ///
    /// The consume and the insert are **one** transaction, so a request is
    /// never consumed without its proposal, and a proposal is never recorded
    /// against a request that was not consumed.
    pub async fn consume_request_into_proposal(
        &self,
        workspace_id: &str,
        request_id: &str,
        proposal: SkillPromotionRecord,
        now_ms: i64,
    ) -> Result<SkillPromotionRecord, SkillPromotionError> {
        validate_record(&proposal)?;
        if proposal.payload.len() > SKILL_PROMOTION_MAX_PAYLOAD_BYTES {
            return Err(SkillPromotionError::PayloadTooLarge);
        }
        let digest = crate::audit::sha256_hex(proposal.payload.as_bytes());
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        let row = sqlx::query_as::<_, PromotionRow>(&format!(
            "SELECT {SELECT_COLUMNS} FROM skill_promotion \
             WHERE workspace_id = ? AND kind = 'request' AND id = ?"
        ))
        .bind(workspace_id)
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            tx.rollback().await.map_err(storage_error)?;
            return Err(SkillPromotionError::RequestNotFound);
        };
        if row.status == "consumed" {
            tx.rollback().await.map_err(storage_error)?;
            return Err(SkillPromotionError::RequestConsumed);
        }
        if row
            .expires_at_ms
            .map(|expiry| expiry <= now_ms)
            .unwrap_or(false)
        {
            sqlx::query(
                "UPDATE skill_promotion SET status = 'expired', updated_at_ms = ? \
                 WHERE workspace_id = ? AND kind = 'request' AND id = ?",
            )
            .bind(now_ms)
            .bind(workspace_id)
            .bind(request_id)
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
            tx.commit().await.map_err(storage_error)?;
            return Err(SkillPromotionError::RequestExpired);
        }

        sqlx::query(
            "INSERT OR REPLACE INTO skill_promotion (id, workspace_id, session_id, kind, \
             payload, payload_bytes, payload_sha256, status, revision, created_at_ms, \
             updated_at_ms, expires_at_ms) \
             VALUES (?, ?, ?, 'proposal', ?, ?, ?, ?, ?, ?, ?, NULL)",
        )
        .bind(&proposal.id)
        .bind(&proposal.workspace_id)
        .bind(&proposal.session_id)
        .bind(&proposal.payload)
        .bind(proposal.payload.len() as i64)
        .bind(&digest)
        .bind(&proposal.status)
        .bind(proposal.revision)
        .bind(proposal.created_at_ms)
        .bind(now_ms)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
        sqlx::query(
            "UPDATE skill_promotion SET status = 'consumed', updated_at_ms = ? \
             WHERE workspace_id = ? AND kind = 'request' AND id = ?",
        )
        .bind(now_ms)
        .bind(workspace_id)
        .bind(request_id)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;

        tx.commit().await.map_err(storage_error)?;
        Ok(SkillPromotionRecord {
            payload_sha256: digest,
            updated_at_ms: now_ms,
            ..proposal
        })
    }
}

fn validate_record(record: &SkillPromotionRecord) -> Result<(), SkillPromotionError> {
    if record.id.is_empty() {
        return Err(SkillPromotionError::Invalid {
            field: "id",
            message: "the record id is empty".to_owned(),
        });
    }
    if record.workspace_id.is_empty() {
        return Err(SkillPromotionError::Invalid {
            field: "workspace_id",
            message: "the record names no workspace".to_owned(),
        });
    }
    if record.session_id.is_empty() {
        return Err(SkillPromotionError::Invalid {
            field: "session_id",
            message: "the record names no session".to_owned(),
        });
    }
    Ok(())
}

fn record_from_row(row: PromotionRow) -> Result<SkillPromotionRecord, SkillPromotionError> {
    Ok(SkillPromotionRecord {
        id: row.id,
        workspace_id: row.workspace_id,
        session_id: row.session_id,
        kind: SkillPromotionKind::parse(&row.kind)?,
        payload: row.payload,
        payload_sha256: row.payload_sha256,
        status: row.status,
        revision: row.revision,
        created_at_ms: row.created_at_ms,
        updated_at_ms: row.updated_at_ms,
        expires_at_ms: row.expires_at_ms,
    })
}
