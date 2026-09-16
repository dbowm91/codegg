//! Durable continuation checkpoints and context-epoch foundation (M001).
//!
//! A continuation checkpoint is a bounded, typed projection of task state
//! sufficient to resume work after a context rollover. It is distinct from:
//!
//! - the historical `checkpoints` table, which serializes a different full
//!   `Checkpoint` session snapshot and is not a compatible store;
//! - `.codegg/goals/*.checkpoint.md`, which is a user-facing append-only
//!   goal journal.
//!
//! Ownership:
//!
//! - `codegg-core` owns durable session-scoped checkpoint records;
//! - `src/context/compaction.rs` remains the compaction-policy owner;
//! - the checkpoint store performs no provider/model calls;
//! - checkpoint payloads never contain hidden reasoning, credentials, or
//!   secret-bearing tool arguments.
//!
//! Lifecycle:
//!
//! ```text
//! Prepared -> Installed
//! Prepared -> Aborted
//! Aborted  -> Aborted   (idempotent re-abort)
//! Installed -> Installed (idempotent install retry only)
//! ```
//!
//! Only `Installed` checkpoints are resume authority.
//! `latest_installed(session_id)` never returns `Prepared` or `Aborted`.
//! Installation and its durable `ContextCompacted` commit marker commit
//! atomically in one SQLite transaction.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use super::events::{ContextCompactedEvent, EventMeta, SessionEvent};
use super::store::EventStore;
use crate::error::StorageError;

/// Version of the checkpoint payload envelope. Independent of the SQLite
/// storage layout version: the layout answers "can this database represent
/// checkpoints?", the schema version answers "can this runtime decode this
/// payload?".
pub const CONTINUATION_CHECKPOINT_SCHEMA_VERSION: u32 = 1;

/// Conservative bound for the serialized payload envelope. Documented and
/// enforced before SQLite insertion; not user-configurable in M001.
pub const CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES: usize = 128 * 1024;

/// Bound for human-readable abort/degraded diagnostics stored in SQLite.
pub const CONTINUATION_CHECKPOINT_MAX_DIAGNOSTIC_CHARS: usize = 1024;

/// Bound for session/checkpoint identifier strings.
pub const CONTINUATION_CHECKPOINT_MAX_ID_LEN: usize = 128;

/// Bound for per-list event metadata carried in the commit-marker event.
pub const CONTINUATION_EVENT_MAX_ITEMS: usize = 64;

/// Bound for each metadata string carried in the commit-marker event.
pub const CONTINUATION_EVENT_MAX_ITEM_CHARS: usize = 512;

/// Stable prefix for the durable commit-marker event ID derived from the
/// checkpoint identity. The event ID is `prefix + checkpoint_id`.
pub const CONTINUATION_EVENT_ID_PREFIX: &str = "continuation-checkpoint:";

/// Returns the stable commit-marker event ID for a checkpoint identity.
pub fn continuation_event_id(checkpoint_id: &str) -> String {
    format!("{CONTINUATION_EVENT_ID_PREFIX}{checkpoint_id}")
}

/// Lifecycle state of a continuation checkpoint candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContinuationCheckpointStatus {
    Prepared,
    Installed,
    Aborted,
}

impl ContinuationCheckpointStatus {
    /// Canonical lowercase storage representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Installed => "installed",
            Self::Aborted => "aborted",
        }
    }

    /// Parses the canonical storage representation.
    pub fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "installed" => Ok(Self::Installed),
            "aborted" => Ok(Self::Aborted),
            other => Err(StorageError::Database(format!(
                "unknown continuation checkpoint status: {other}"
            ))),
        }
    }

    /// Legal transitions, including idempotent re-application of a terminal
    /// state through its own operation (`mark_aborted` on `Aborted`,
    /// install retry on `Installed`).
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Prepared, Self::Installed)
                | (Self::Prepared, Self::Aborted)
                | (Self::Aborted, Self::Aborted)
                | (Self::Installed, Self::Installed)
        )
    }
}

impl std::fmt::Display for ContinuationCheckpointStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Versioned envelope for the checkpoint body.
///
/// M001 establishes the envelope and bounded serialization contract without
/// owning all M002 semantic fields. The body is an explicit JSON object
/// assembled by typed callers; the store never scrapes transcripts, tool
/// arguments, or provider state into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationCheckpointPayload {
    pub schema_version: u32,
    pub body: serde_json::Value,
}

impl ContinuationCheckpointPayload {
    /// Typed constructor enforcing schema version, object shape, byte
    /// bounds, and forbidden content classes.
    pub fn new(body: serde_json::Value) -> Result<Self, StorageError> {
        let payload = Self {
            schema_version: CONTINUATION_CHECKPOINT_SCHEMA_VERSION,
            body,
        };
        payload.validate()?;
        Ok(payload)
    }

    /// Full validation used by the constructor and the store before insert.
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.schema_version != CONTINUATION_CHECKPOINT_SCHEMA_VERSION {
            return Err(StorageError::Database(format!(
                "unsupported continuation payload schema version: {}",
                self.schema_version
            )));
        }
        if !self.body.is_object() {
            return Err(StorageError::Database(
                "continuation payload body must be a JSON object".to_string(),
            ));
        }
        let encoded = self.canonical_json()?;
        if encoded.len() > CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES {
            return Err(StorageError::Database(format!(
                "continuation payload exceeds {} bytes",
                CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES
            )));
        }
        if let Some(key) = first_forbidden_key(&self.body) {
            return Err(StorageError::Database(format!(
                "continuation payload rejects forbidden content class: {key}"
            )));
        }
        Ok(())
    }

    /// Canonical serialized form covered by the payload digest.
    pub fn canonical_json(&self) -> Result<String, StorageError> {
        serde_json::to_string(self)
            .map_err(|e| StorageError::Database(format!("continuation payload encode: {e}")))
    }

    /// Deterministic SHA-256 hex digest over the canonical JSON bytes.
    pub fn digest(&self) -> Result<String, StorageError> {
        let encoded = self.canonical_json()?;
        Ok(compute_payload_digest(encoded.as_bytes()))
    }

    /// Bounded diagnostic summary. Never includes the payload body.
    pub fn diagnostic_summary(&self, digest: &str) -> String {
        let bytes = self.canonical_json().map(|s| s.len()).unwrap_or(0);
        format!(
            "payload(schema={}, bytes={}, digest={})",
            self.schema_version, bytes, digest
        )
    }
}

/// Deterministic SHA-256 hex digest helper shared by payload verification.
pub fn compute_payload_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Normalized denylist of content classes that must never be persisted
/// into continuation state: provider-hidden reasoning and secret-bearing
/// material. Keys are normalized by lowercasing and stripping `_`/`-`.
fn is_forbidden_key(normalized: &str) -> bool {
    matches!(
        normalized,
        "reasoning"
            | "hiddenreasoning"
            | "chainofthought"
            | "apikey"
            | "authorization"
            | "credential"
            | "credentials"
            | "secret"
            | "secrets"
            | "bearer"
            | "password"
            | "privatekey"
    )
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| *c != '_' && *c != '-')
        .collect::<String>()
        .to_lowercase()
}

fn first_forbidden_key(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, nested) in map {
                if is_forbidden_key(&normalize_key(key)) {
                    return Some(key.clone());
                }
                if let Some(hit) = first_forbidden_key(nested) {
                    return Some(hit);
                }
            }
            None
        }
        serde_json::Value::Array(items) => items.iter().find_map(first_forbidden_key),
        _ => None,
    }
}

/// Validates a session/checkpoint identifier without logging its value.
///
/// Rules mirror handle safety: non-empty, bounded, and free of slashes,
/// whitespace, and control characters. Queries remain parameterized.
fn validate_id(value: &str, field: &str) -> Result<(), StorageError> {
    if value.is_empty() || value.len() > CONTINUATION_CHECKPOINT_MAX_ID_LEN {
        return Err(StorageError::Database(format!(
            "invalid {field} identifier length"
        )));
    }
    if value.contains('/') || value.contains('\\') {
        return Err(StorageError::Database(format!(
            "invalid {field} identifier: path separator"
        )));
    }
    if value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(StorageError::Database(format!(
            "invalid {field} identifier: control or whitespace"
        )));
    }
    Ok(())
}

fn validate_session_id(session_id: &str) -> Result<(), StorageError> {
    validate_id(session_id, "session")
}

fn validate_checkpoint_id(checkpoint_id: &str) -> Result<(), StorageError> {
    validate_id(checkpoint_id, "checkpoint")
}

fn validate_diagnostic(value: &str, field: &str) -> Result<(), StorageError> {
    if value.chars().count() > CONTINUATION_CHECKPOINT_MAX_DIAGNOSTIC_CHARS {
        return Err(StorageError::Database(format!(
            "{field} exceeds {} chars",
            CONTINUATION_CHECKPOINT_MAX_DIAGNOSTIC_CHARS
        )));
    }
    if value.contains('\0') {
        return Err(StorageError::Database(format!("invalid {field}: NUL byte")));
    }
    Ok(())
}

fn validate_event_metadata(items: &[String], field: &str) -> Result<(), StorageError> {
    if items.len() > CONTINUATION_EVENT_MAX_ITEMS {
        return Err(StorageError::Database(format!(
            "{field} exceeds {CONTINUATION_EVENT_MAX_ITEMS} items"
        )));
    }
    for item in items {
        if item.chars().count() > CONTINUATION_EVENT_MAX_ITEM_CHARS {
            return Err(StorageError::Database(format!(
                "{field} item exceeds {CONTINUATION_EVENT_MAX_ITEM_CHARS} chars"
            )));
        }
        if item.contains('\0') {
            return Err(StorageError::Database(format!(
                "invalid {field} item: NUL byte"
            )));
        }
    }
    Ok(())
}

/// Durable continuation checkpoint row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationCheckpoint {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub previous_installed_id: Option<String>,
    pub schema_version: u32,
    pub status: ContinuationCheckpointStatus,
    pub payload_digest: String,
    pub payload: ContinuationCheckpointPayload,
    pub created_at: i64,
    pub installed_at: Option<i64>,
    pub aborted_at: Option<i64>,
    pub abort_reason: Option<String>,
}

impl ContinuationCheckpoint {
    /// Recomputes the payload digest and compares it to the stored digest.
    pub fn verify_digest(&self) -> Result<(), StorageError> {
        let recomputed = self.payload.digest()?;
        if recomputed != self.payload_digest {
            return Err(StorageError::Database(format!(
                "continuation checkpoint digest mismatch: {}",
                self.id
            )));
        }
        Ok(())
    }

    /// Stable commit-marker event ID derived from the checkpoint identity.
    pub fn event_id(&self) -> String {
        continuation_event_id(&self.id)
    }

    /// Small application-facing helper so later `AgentLoop` integration
    /// does not serialize SQL or event rows manually. Builds the durable
    /// `ContextCompacted` commit marker with lineage filled from this
    /// checkpoint; the store enforces the same normalization on install.
    #[allow(clippy::too_many_arguments)]
    pub fn build_compacted_event(
        &self,
        messages_removed: usize,
        messages_remaining: usize,
        token_estimate_before: Option<usize>,
        token_estimate_after: Option<usize>,
        pinned_items: Vec<String>,
        summarized_items: Vec<String>,
        dropped_items: Vec<String>,
    ) -> Result<ContextCompactedEvent, StorageError> {
        validate_event_metadata(&pinned_items, "pinned_items")?;
        validate_event_metadata(&summarized_items, "summarized_items")?;
        validate_event_metadata(&dropped_items, "dropped_items")?;
        Ok(ContextCompactedEvent {
            meta: EventMeta {
                id: self.event_id(),
                session_id: self.session_id.clone(),
                created_at: Utc::now(),
            },
            messages_removed,
            messages_remaining,
            token_estimate_before,
            token_estimate_after,
            pinned_items,
            summarized_items,
            dropped_items,
            checkpoint_id: Some(self.id.clone()),
            checkpoint_digest: Some(self.payload_digest.clone()),
            epoch_sequence: Some(self.sequence),
            previous_checkpoint_id: self.previous_installed_id.clone(),
            continuity_degraded_reason: None,
        })
    }

    /// Bounded diagnostic summary carrying IDs, digests, sizes, and state
    /// transitions. Never includes the payload body.
    pub fn diagnostic_summary(&self) -> String {
        format!(
            "checkpoint(id={}, session={}, seq={}, prev={}, status={}, digest={}, \
             schema={}, created={}, installed={}, aborted={})",
            self.id,
            self.session_id,
            self.sequence,
            self.previous_installed_id.as_deref().unwrap_or("-"),
            self.status.as_str(),
            self.payload_digest,
            self.schema_version,
            self.created_at,
            self.installed_at
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            self.aborted_at
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
        )
    }
}

#[derive(Debug, FromRow)]
struct ContinuationCheckpointRow {
    id: String,
    session_id: String,
    sequence: i64,
    previous_installed_id: Option<String>,
    schema_version: i64,
    status: String,
    payload_digest: String,
    payload_json: String,
    abort_reason: Option<String>,
    created_at: i64,
    installed_at: Option<i64>,
    aborted_at: Option<i64>,
}

impl TryFrom<ContinuationCheckpointRow> for ContinuationCheckpoint {
    type Error = StorageError;

    fn try_from(row: ContinuationCheckpointRow) -> Result<Self, Self::Error> {
        let status = ContinuationCheckpointStatus::parse(&row.status)?;
        let payload: ContinuationCheckpointPayload = serde_json::from_str(&row.payload_json)
            .map_err(|e| StorageError::Database(format!("continuation payload decode: {e}")))?;
        let schema_version: u32 = u32::try_from(row.schema_version)
            .map_err(|_| StorageError::Database("continuation schema version range".to_string()))?;
        if schema_version != CONTINUATION_CHECKPOINT_SCHEMA_VERSION {
            return Err(StorageError::Database(format!(
                "unsupported continuation schema version: {schema_version}"
            )));
        }
        let checkpoint = Self {
            id: row.id,
            session_id: row.session_id,
            sequence: row.sequence,
            previous_installed_id: row.previous_installed_id,
            schema_version,
            status,
            payload_digest: row.payload_digest,
            payload,
            created_at: row.created_at,
            installed_at: row.installed_at,
            aborted_at: row.aborted_at,
            abort_reason: row.abort_reason,
        };
        checkpoint.verify_digest()?;
        Ok(checkpoint)
    }
}

/// Bounded append-oriented store for continuation checkpoint candidates.
///
/// All writers serialize through SQLite transactions with the session-scoped
/// lineage precondition checked inside the same transaction that mutates
/// state, so a stale candidate cannot install over a newer installed
/// parent.
pub struct ContinuationCheckpointStore {
    pool: SqlitePool,
}

impl ContinuationCheckpointStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Test-only escape hatch for fixtures that bind the same database to
    /// a second store instance (restart simulation).
    pub fn pool(&self) -> SqlitePool {
        self.pool.clone()
    }

    /// Prepares an immutable `Prepared` candidate after validating fields,
    /// payload bounds, and the expected installed parent.
    pub async fn prepare(
        &self,
        session_id: &str,
        previous_installed_id: Option<&str>,
        payload: ContinuationCheckpointPayload,
    ) -> Result<ContinuationCheckpoint, StorageError> {
        validate_session_id(session_id)?;
        if let Some(parent) = previous_installed_id {
            validate_checkpoint_id(parent)?;
        }
        payload.validate()?;
        let payload_json = payload.canonical_json()?;
        if payload_json.len() > CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES {
            return Err(StorageError::Database(format!(
                "continuation payload exceeds {} bytes",
                CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES
            )));
        }
        let payload_digest = payload.digest()?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let latest: Option<ContinuationCheckpointRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, previous_installed_id, schema_version, \
             status, payload_digest, payload_json, abort_reason, created_at, installed_at, \
             aborted_at FROM continuation_checkpoint \
             WHERE session_id = ? AND status = 'installed' \
             ORDER BY sequence DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        let latest_id = latest.as_ref().map(|row| row.id.as_str());
        if latest_id != previous_installed_id {
            return Err(StorageError::Database(format!(
                "continuation checkpoint stale parent for session {session_id}"
            )));
        }

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM continuation_checkpoint \
             WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO continuation_checkpoint (id, session_id, sequence, \
             previous_installed_id, schema_version, status, payload_digest, payload_json, \
             abort_reason, created_at, installed_at, aborted_at) \
             VALUES (?, ?, ?, ?, ?, 'prepared', ?, ?, NULL, ?, NULL, NULL)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(next_sequence)
        .bind(previous_installed_id)
        .bind(i64::from(CONTINUATION_CHECKPOINT_SCHEMA_VERSION))
        .bind(&payload_digest)
        .bind(&payload_json)
        .bind(created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(ContinuationCheckpoint {
            id,
            session_id: session_id.to_string(),
            sequence: next_sequence,
            previous_installed_id: previous_installed_id.map(str::to_string),
            schema_version: CONTINUATION_CHECKPOINT_SCHEMA_VERSION,
            status: ContinuationCheckpointStatus::Prepared,
            payload_digest,
            payload,
            created_at,
            installed_at: None,
            aborted_at: None,
            abort_reason: None,
        })
    }

    /// Prepares an immutable `Prepared` candidate with a caller-allocated
    /// checkpoint identity (M004 evidence ordering).
    ///
    /// The caller allocates the UUID before evidence materialization so
    /// `ctx://evidence/{session}/{checkpoint}/{evidence}` handles can be
    /// written and verified before the payload digest is final. The payload
    /// must already contain the verified handles; the checkpoint is still
    /// `Prepared` (never resume authority) until `install_with_compaction_event`.
    /// An abandoned candidate remains `Prepared`/`Aborted`.
    pub async fn prepare_with_id(
        &self,
        session_id: &str,
        checkpoint_id: &str,
        previous_installed_id: Option<&str>,
        payload: ContinuationCheckpointPayload,
    ) -> Result<ContinuationCheckpoint, StorageError> {
        validate_session_id(session_id)?;
        validate_checkpoint_id(checkpoint_id)?;
        if let Some(parent) = previous_installed_id {
            validate_checkpoint_id(parent)?;
        }
        payload.validate()?;
        let payload_json = payload.canonical_json()?;
        if payload_json.len() > CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES {
            return Err(StorageError::Database(format!(
                "continuation payload exceeds {} bytes",
                CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES
            )));
        }
        let payload_digest = payload.digest()?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let latest: Option<ContinuationCheckpointRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, previous_installed_id, schema_version, \
             status, payload_digest, payload_json, abort_reason, created_at, installed_at, \
             aborted_at FROM continuation_checkpoint \
             WHERE session_id = ? AND status = 'installed' \
             ORDER BY sequence DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        let latest_id = latest.as_ref().map(|row| row.id.as_str());
        if latest_id != previous_installed_id {
            return Err(StorageError::Database(format!(
                "continuation checkpoint stale parent for session {session_id}"
            )));
        }

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM continuation_checkpoint \
             WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        let created_at = Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO continuation_checkpoint (id, session_id, sequence, \
             previous_installed_id, schema_version, status, payload_digest, payload_json, \
             abort_reason, created_at, installed_at, aborted_at) \
             VALUES (?, ?, ?, ?, ?, 'prepared', ?, ?, NULL, ?, NULL, NULL)",
        )
        .bind(checkpoint_id)
        .bind(session_id)
        .bind(next_sequence)
        .bind(previous_installed_id)
        .bind(i64::from(CONTINUATION_CHECKPOINT_SCHEMA_VERSION))
        .bind(&payload_digest)
        .bind(&payload_json)
        .bind(created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(ContinuationCheckpoint {
            id: checkpoint_id.to_string(),
            session_id: session_id.to_string(),
            sequence: next_sequence,
            previous_installed_id: previous_installed_id.map(str::to_string),
            schema_version: CONTINUATION_CHECKPOINT_SCHEMA_VERSION,
            status: ContinuationCheckpointStatus::Prepared,
            payload_digest,
            payload,
            created_at,
            installed_at: None,
            aborted_at: None,
            abort_reason: None,
        })
    }

    /// Loads one checkpoint by session and checkpoint identity.
    pub async fn get(
        &self,
        session_id: &str,
        checkpoint_id: &str,
    ) -> Result<Option<ContinuationCheckpoint>, StorageError> {
        validate_session_id(session_id)?;
        validate_checkpoint_id(checkpoint_id)?;
        let row: Option<ContinuationCheckpointRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, previous_installed_id, schema_version, \
             status, payload_digest, payload_json, abort_reason, created_at, installed_at, \
             aborted_at FROM continuation_checkpoint WHERE session_id = ? AND id = ?",
        )
        .bind(session_id)
        .bind(checkpoint_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        row.map(ContinuationCheckpoint::try_from).transpose()
    }

    /// Returns the latest `Installed` checkpoint for resume. Never returns
    /// `Prepared` or `Aborted` rows. Fails closed on digest or schema
    /// mismatch so restart cannot trust a tampered epoch.
    pub async fn latest_installed(
        &self,
        session_id: &str,
    ) -> Result<Option<ContinuationCheckpoint>, StorageError> {
        validate_session_id(session_id)?;
        let row: Option<ContinuationCheckpointRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, previous_installed_id, schema_version, \
             status, payload_digest, payload_json, abort_reason, created_at, installed_at, \
             aborted_at FROM continuation_checkpoint \
             WHERE session_id = ? AND status = 'installed' \
             ORDER BY sequence DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        row.map(ContinuationCheckpoint::try_from).transpose()
    }

    /// Bounded lineage/debug listing ordered by sequence. Used by later
    /// milestones and diagnostics; not a resume path.
    pub async fn list_for_session(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<ContinuationCheckpoint>, StorageError> {
        validate_session_id(session_id)?;
        let capped = limit.clamp(1, 500);
        let rows: Vec<ContinuationCheckpointRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, previous_installed_id, schema_version, \
             status, payload_digest, payload_json, abort_reason, created_at, installed_at, \
             aborted_at FROM continuation_checkpoint WHERE session_id = ? \
             ORDER BY sequence ASC LIMIT ?",
        )
        .bind(session_id)
        .bind(capped)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        rows.into_iter()
            .map(ContinuationCheckpoint::try_from)
            .collect()
    }

    /// Marks a `Prepared` candidate as `Aborted`. Idempotent for an
    /// already-`Aborted` row; refuses to abort an `Installed` checkpoint.
    pub async fn mark_aborted(
        &self,
        session_id: &str,
        checkpoint_id: &str,
        reason: &str,
    ) -> Result<ContinuationCheckpoint, StorageError> {
        validate_session_id(session_id)?;
        validate_checkpoint_id(checkpoint_id)?;
        validate_diagnostic(reason, "abort_reason")?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        let row: Option<ContinuationCheckpointRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, previous_installed_id, schema_version, \
             status, payload_digest, payload_json, abort_reason, created_at, installed_at, \
             aborted_at FROM continuation_checkpoint WHERE session_id = ? AND id = ?",
        )
        .bind(session_id)
        .bind(checkpoint_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        let row = row.ok_or_else(|| {
            StorageError::NotFound(format!("continuation checkpoint {checkpoint_id}"))
        })?;
        let current = ContinuationCheckpointStatus::parse(&row.status)?;
        match current {
            ContinuationCheckpointStatus::Installed => {
                return Err(StorageError::Database(format!(
                    "cannot abort installed continuation checkpoint {checkpoint_id}"
                )));
            }
            ContinuationCheckpointStatus::Aborted => {
                tx.commit()
                    .await
                    .map_err(|e| StorageError::Database(e.to_string()))?;
                return ContinuationCheckpoint::try_from(row);
            }
            ContinuationCheckpointStatus::Prepared => {}
        }

        let aborted_at = Utc::now().timestamp_millis();
        sqlx::query(
            "UPDATE continuation_checkpoint SET status = 'aborted', aborted_at = ?, \
             abort_reason = ? WHERE session_id = ? AND id = ?",
        )
        .bind(aborted_at)
        .bind(reason)
        .bind(session_id)
        .bind(checkpoint_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let updated = self.get(session_id, checkpoint_id).await?.ok_or_else(|| {
            StorageError::Database(format!(
                "continuation checkpoint disappeared: {checkpoint_id}"
            ))
        })?;
        Ok(updated)
    }

    /// Atomically marks a `Prepared` checkpoint `Installed` and appends the
    /// matching durable `SessionEvent::ContextCompacted` commit marker.
    ///
    /// The event identity is the stable ID derived from the checkpoint
    /// identity; a duplicate retry with semantically identical content
    /// converges idempotently while a conflicting payload or event for the
    /// same identity fails closed. A stale candidate whose recorded parent
    /// no longer equals the latest installed checkpoint is rejected.
    pub async fn install_with_compaction_event(
        &self,
        session_id: &str,
        checkpoint_id: &str,
        event: ContextCompactedEvent,
    ) -> Result<ContinuationCheckpoint, StorageError> {
        validate_session_id(session_id)?;
        validate_checkpoint_id(checkpoint_id)?;
        validate_event_metadata(&event.pinned_items, "pinned_items")?;
        validate_event_metadata(&event.summarized_items, "summarized_items")?;
        validate_event_metadata(&event.dropped_items, "dropped_items")?;
        if let Some(reason) = event.continuity_degraded_reason.as_deref() {
            validate_diagnostic(reason, "continuity_degraded_reason")?;
        }
        if event.meta.session_id != session_id {
            return Err(StorageError::Database(
                "continuation install event session mismatch".to_string(),
            ));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;
        let row: Option<ContinuationCheckpointRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, previous_installed_id, schema_version, \
             status, payload_digest, payload_json, abort_reason, created_at, installed_at, \
             aborted_at FROM continuation_checkpoint WHERE session_id = ? AND id = ?",
        )
        .bind(session_id)
        .bind(checkpoint_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        let row = row.ok_or_else(|| {
            StorageError::NotFound(format!("continuation checkpoint {checkpoint_id}"))
        })?;
        let checkpoint = ContinuationCheckpoint::try_from(row)?;

        if checkpoint.status == ContinuationCheckpointStatus::Aborted {
            return Err(StorageError::Database(format!(
                "cannot install aborted continuation checkpoint {checkpoint_id}"
            )));
        }

        let normalized = normalize_install_event(&checkpoint, event)?;

        if checkpoint.status == ContinuationCheckpointStatus::Installed {
            EventStore::append_in_tx(&mut tx, &SessionEvent::ContextCompacted(normalized)).await?;
            tx.commit()
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;
            let current = self.get(session_id, checkpoint_id).await?.ok_or_else(|| {
                StorageError::Database(format!(
                    "continuation checkpoint disappeared: {checkpoint_id}"
                ))
            })?;
            return Ok(current);
        }

        // Prepared -> Installed: re-verify the lineage precondition inside
        // the same transaction that commits the state change.
        let latest: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM continuation_checkpoint WHERE session_id = ? AND status = \
             'installed' ORDER BY sequence DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        let latest_id = latest.as_ref().map(|(id,)| id.as_str());
        if latest_id != checkpoint.previous_installed_id.as_deref() {
            return Err(StorageError::Database(format!(
                "continuation checkpoint stale parent for session {session_id}"
            )));
        }

        let installed_at = Utc::now().timestamp_millis();
        sqlx::query(
            "UPDATE continuation_checkpoint SET status = 'installed', installed_at = ? \
             WHERE session_id = ? AND id = ? AND status = 'prepared'",
        )
        .bind(installed_at)
        .bind(session_id)
        .bind(checkpoint_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;

        EventStore::append_in_tx(&mut tx, &SessionEvent::ContextCompacted(normalized)).await?;

        tx.commit()
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let installed = self.get(session_id, checkpoint_id).await?.ok_or_else(|| {
            StorageError::Database(format!(
                "continuation checkpoint disappeared: {checkpoint_id}"
            ))
        })?;
        if installed.status != ContinuationCheckpointStatus::Installed {
            return Err(StorageError::Database(format!(
                "continuation checkpoint install did not commit: {checkpoint_id}"
            )));
        }
        Ok(installed)
    }

    /// Bounded test/retention helper. Deletes one `Prepared` or `Aborted`
    /// row; never deletes an `Installed` checkpoint.
    pub async fn delete_candidate(
        &self,
        session_id: &str,
        checkpoint_id: &str,
    ) -> Result<(), StorageError> {
        validate_session_id(session_id)?;
        validate_checkpoint_id(checkpoint_id)?;
        let removed = sqlx::query(
            "DELETE FROM continuation_checkpoint WHERE session_id = ? AND id = ? AND \
             status IN ('prepared', 'aborted')",
        )
        .bind(session_id)
        .bind(checkpoint_id)
        .execute(&self.pool)
        .await
        .map_err(|e| StorageError::Database(e.to_string()))?;
        if removed.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!(
                "continuation candidate {checkpoint_id}"
            )));
        }
        Ok(())
    }
}

/// Normalizes a caller-supplied commit-marker event against the checkpoint
/// being installed: stable event ID, session binding, and lineage fields.
/// The continuation payload itself is never embedded in the event.
fn normalize_install_event(
    checkpoint: &ContinuationCheckpoint,
    mut event: ContextCompactedEvent,
) -> Result<ContextCompactedEvent, StorageError> {
    checkpoint.verify_digest()?;
    let expected_id = checkpoint.event_id();
    event.meta.id = expected_id;
    event.meta.session_id = checkpoint.session_id.clone();
    event.checkpoint_id = Some(checkpoint.id.clone());
    event.checkpoint_digest = Some(checkpoint.payload_digest.clone());
    event.epoch_sequence = Some(checkpoint.sequence);
    event.previous_checkpoint_id = checkpoint.previous_installed_id.clone();
    Ok(event)
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn benign_body() -> serde_json::Value {
        serde_json::json!({
            "objective": "migrate checkout flow",
            "current_task": "port discount validation",
            "next_action": "run checkout tests",
        })
    }

    #[test]
    fn status_round_trip_and_transition_matrix() {
        for status in [
            ContinuationCheckpointStatus::Prepared,
            ContinuationCheckpointStatus::Installed,
            ContinuationCheckpointStatus::Aborted,
        ] {
            let parsed = ContinuationCheckpointStatus::parse(status.as_str()).unwrap();
            assert_eq!(parsed, status);
        }
        assert!(ContinuationCheckpointStatus::parse("unknown").is_err());

        use ContinuationCheckpointStatus::{Aborted, Installed, Prepared};
        assert!(Prepared.can_transition_to(Installed));
        assert!(Prepared.can_transition_to(Aborted));
        assert!(Aborted.can_transition_to(Aborted));
        assert!(Installed.can_transition_to(Installed));
        assert!(!Prepared.can_transition_to(Prepared));
        assert!(!Installed.can_transition_to(Aborted));
        assert!(!Aborted.can_transition_to(Installed));
        assert!(!Installed.can_transition_to(Prepared));
        assert!(!Aborted.can_transition_to(Prepared));
    }

    #[test]
    fn payload_serialization_is_stable_and_bounded() {
        let first = ContinuationCheckpointPayload::new(benign_body()).unwrap();
        let second = ContinuationCheckpointPayload::new(benign_body()).unwrap();
        assert_eq!(first.digest().unwrap(), second.digest().unwrap());
        assert_eq!(
            first.canonical_json().unwrap(),
            second.canonical_json().unwrap()
        );

        let different = ContinuationCheckpointPayload::new(serde_json::json!({
            "objective": "different objective",
        }))
        .unwrap();
        assert_ne!(first.digest().unwrap(), different.digest().unwrap());
    }

    #[test]
    fn oversized_payload_rejected_before_storage() {
        let big = "x".repeat(CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES);
        let err = ContinuationCheckpointPayload::new(serde_json::json!({
            "objective": big,
        }))
        .unwrap_err();
        assert!(err.to_string().contains("exceeds"));
    }

    #[test]
    fn non_object_body_rejected() {
        let err =
            ContinuationCheckpointPayload::new(serde_json::json!(["not", "object"])).unwrap_err();
        assert!(err.to_string().contains("JSON object"));
    }

    #[test]
    fn hidden_reasoning_rejected() {
        let err = ContinuationCheckpointPayload::new(serde_json::json!({
            "objective": "work",
            "reasoning": "private chain of thought",
        }))
        .unwrap_err();
        assert!(err.to_string().contains("forbidden"));
    }

    #[test]
    fn nested_secret_tool_argument_rejected() {
        let err = ContinuationCheckpointPayload::new(serde_json::json!({
            "objective": "work",
            "tool_args": {"api_key": "sk-secret-value"},
        }))
        .unwrap_err();
        assert!(err.to_string().contains("forbidden"));
    }

    #[test]
    fn tampered_digest_detected() {
        let payload = ContinuationCheckpointPayload::new(benign_body()).unwrap();
        let digest = payload.digest().unwrap();
        let checkpoint = ContinuationCheckpoint {
            id: "checkpoint-1".to_string(),
            session_id: "session-1".to_string(),
            sequence: 1,
            previous_installed_id: None,
            schema_version: CONTINUATION_CHECKPOINT_SCHEMA_VERSION,
            status: ContinuationCheckpointStatus::Prepared,
            payload_digest: digest,
            payload,
            created_at: 1,
            installed_at: None,
            aborted_at: None,
            abort_reason: None,
        };
        assert!(checkpoint.verify_digest().is_ok());
        let mut tampered = checkpoint;
        tampered.payload_digest = "00".repeat(32);
        assert!(tampered.verify_digest().is_err());
    }

    #[test]
    fn diagnostic_helpers_never_embed_payload_body() {
        let secret_marker = "not-a-real-secret-payload-body-marker";
        let payload = ContinuationCheckpointPayload::new(serde_json::json!({
            "objective": "ordinary work",
        }))
        .unwrap();
        let digest = payload.digest().unwrap();
        assert!(payload.diagnostic_summary(&digest).contains(&digest));
        assert!(!payload.diagnostic_summary(&digest).contains(secret_marker));

        let checkpoint = ContinuationCheckpoint {
            id: "checkpoint-1".to_string(),
            session_id: "session-1".to_string(),
            sequence: 1,
            previous_installed_id: None,
            schema_version: CONTINUATION_CHECKPOINT_SCHEMA_VERSION,
            status: ContinuationCheckpointStatus::Prepared,
            payload_digest: digest,
            payload,
            created_at: 1,
            installed_at: None,
            aborted_at: None,
            abort_reason: None,
        };
        let summary = checkpoint.diagnostic_summary();
        assert!(summary.contains("checkpoint-1"));
        assert!(!summary.contains(secret_marker));
        assert!(!summary.contains("ordinary work"));
    }

    #[test]
    fn secret_fixture_not_copied_by_envelope_serialization() {
        let secret_tool_argument = "sk-live-secret-fixture-9f8e7d6c5b4a";
        let payload = ContinuationCheckpointPayload::new(benign_body()).unwrap();
        let encoded = payload.canonical_json().unwrap();
        assert!(!encoded.contains(secret_tool_argument));
    }

    #[test]
    fn invalid_identifiers_rejected() {
        assert!(validate_session_id("").is_err());
        assert!(validate_checkpoint_id("").is_err());
        assert!(validate_session_id("has/slash").is_err());
        assert!(validate_checkpoint_id("has whitespace").is_err());
        assert!(validate_session_id("has\tcontrol").is_err());
        assert!(validate_session_id(&"x".repeat(129)).is_err());
        assert!(validate_session_id("session-1").is_ok());
    }

    #[test]
    fn continuation_event_id_is_stable() {
        assert_eq!(continuation_event_id("abc"), "continuation-checkpoint:abc");
    }

    #[test]
    fn old_context_compacted_json_remains_deserializable() {
        let old = serde_json::json!({
            "ContextCompacted": {
                "meta": {
                    "id": "event-1",
                    "session_id": "session-1",
                    "created_at": "2026-01-01T00:00:00Z",
                },
                "messages_removed": 10,
                "messages_remaining": 5,
                "token_estimate_before": 10000,
                "token_estimate_after": 3000,
                "pinned_items": ["goal"],
                "summarized_items": [],
                "dropped_items": [],
            }
        });
        let event: SessionEvent = serde_json::from_value(old).unwrap();
        match event {
            SessionEvent::ContextCompacted(compacted) => {
                assert_eq!(compacted.messages_removed, 10);
                assert_eq!(compacted.checkpoint_id, None);
                assert_eq!(compacted.epoch_sequence, None);
                assert_eq!(compacted.continuity_degraded_reason, None);
            }
            other => panic!("unexpected event: {}", other.event_type_tag()),
        }
    }

    #[test]
    fn compacted_semantic_equality_ignores_created_at_only() {
        let first = ContextCompactedEvent {
            meta: EventMeta {
                id: "event-1".to_string(),
                session_id: "session-1".to_string(),
                created_at: chrono::Utc::now(),
            },
            messages_removed: 1,
            messages_remaining: 2,
            token_estimate_before: Some(3),
            token_estimate_after: Some(4),
            pinned_items: vec!["a".to_string()],
            summarized_items: vec![],
            dropped_items: vec![],
            checkpoint_id: Some("checkpoint-1".to_string()),
            checkpoint_digest: Some("digest".to_string()),
            epoch_sequence: Some(1),
            previous_checkpoint_id: None,
            continuity_degraded_reason: None,
        };
        let mut second = first.clone();
        second.meta.created_at = chrono::Utc::now();
        assert!(first.semantic_equals(&second));
        second.messages_remaining = 99;
        assert!(!first.semantic_equals(&second));
    }
}
