//! The durable transcript of a delegated run (ADR-0051, option B).
//!
//! A delegated run is **not** a session: it has no `session` row, it never
//! enters session enumeration, and it is never compacted by the parent
//! session's policy. Its conversation therefore cannot live in the `message`
//! table, and this module owns the store that does instead — keyed by
//! [`AgentRunId`], the same durable identity the run's result already uses.
//!
//! Three properties are load-bearing and are enforced here rather than
//! assumed: **redaction happens at write** (the body is redacted before it
//! reaches the store, so a later bug in a reader cannot expose a secret that
//! was already durable, and the recogniser is the repository's existing
//! [`ProjectionFieldRedactor`] — there is no second secret recogniser here);
//! **bounds are enforced at write** (a truncated transcript says so instead of
//! reading as complete); and **the store is append-only and ordinal-stable**
//! (a row is never rewritten, and a concurrent append for one run receives a
//! distinct ordinal).

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::identity::AgentRunId;
use crate::projection_replay::redactor::{FieldName, ProjectionFieldRedactor, RedactionResult};

/// The largest body one transcript row may hold, after redaction.
pub const MAX_TRANSCRIPT_ROW_BYTES: usize = 8 * 1024;

/// The largest total body one run's transcript may hold, after redaction.
pub const MAX_TRANSCRIPT_RUN_BYTES: usize = 256 * 1024;

/// The largest page one read may return. A read is always bounded.
pub const MAX_TRANSCRIPT_READ_LIMIT: usize = 256;

/// Appended to a row whose body exceeded [`MAX_TRANSCRIPT_ROW_BYTES`], so the
/// truncation is visible in the row rather than inferred from its length.
pub const TRANSCRIPT_ROW_TRUNCATION_MARKER: &str =
    "\n[transcript-truncated: per-row byte bound reached]";

/// The body of the marker row recorded when the per-run bound is reached.
pub const TRANSCRIPT_RUN_TRUNCATION_MARKER: &str =
    "[transcript-truncated: per-run byte bound reached]";

/// Bytes held back from the per-run budget so the truncation marker always
/// fits inside it.
const RUN_TRUNCATION_MARKER_RESERVE: usize = 128;

/// The body stored when the redactor refuses a value. The original text is
/// never returned: a failed scan is a denial, not a pass-through.
const REDACTION_REFUSED_MARKER: &str = "[REDACTED:scan-refused]";

/// Which channel a transcript row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    /// The orchestrator-visible channel: what a parent needs to know.
    Orchestrator,
    /// The child's internal channel: the messages the child alone acted on.
    Internal,
}

impl TranscriptRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Internal => "internal",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "orchestrator" => Some(Self::Orchestrator),
            "internal" => Some(Self::Internal),
            _ => None,
        }
    }
}

/// What kind of transcript row this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptKind {
    /// A message the model exchanged with the delegated run.
    ModelMessage,
    ToolCall,
    ToolResult,
    /// A terminal note: cancellation, failure, or a truncation marker.
    TerminalNote,
}

impl TranscriptKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModelMessage => "model_message",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::TerminalNote => "terminal_note",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "model_message" => Some(Self::ModelMessage),
            "tool_call" => Some(Self::ToolCall),
            "tool_result" => Some(Self::ToolResult),
            "terminal_note" => Some(Self::TerminalNote),
            _ => None,
        }
    }
}

/// One durable transcript row. `redacted_body` is the **post-redaction** text
/// and `byte_len` is its post-redaction length, so a bound check reads the
/// stored size rather than re-deriving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunTranscriptEntry {
    pub run_id: AgentRunId,
    pub ordinal: i64,
    pub role: TranscriptRole,
    pub kind: TranscriptKind,
    pub redacted_body: String,
    pub byte_len: i64,
    pub created_at: i64,
}

/// How an append was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptAppendStatus {
    /// The body was stored as submitted, after redaction.
    Stored,
    /// The body exceeded the per-row bound; the stored row is truncated and
    /// carries the in-row marker.
    RowTruncated,
    /// The per-run bound is reached; the body was refused and a truncation
    /// marker row was recorded instead.
    RunTruncated,
}

/// The result of one append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptAppendOutcome {
    pub status: TranscriptAppendStatus,
    /// The ordinal the row was stored at; for a refused body, the marker's.
    pub ordinal: i64,
    /// The stored size of the row, in bytes.
    pub byte_len: i64,
}

/// One bounded page of a run's transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunTranscriptPage {
    pub entries: Vec<AgentRunTranscriptEntry>,
    /// Whether rows exist beyond this page.
    pub has_more: bool,
    /// Whether the run's transcript hit the per-run bound.
    pub truncated: bool,
    /// The run's total stored transcript bytes and rows.
    pub total_bytes: i64,
    pub total_entries: i64,
}

/// The read result for one run. [`AgentRunTranscriptRead::Absent`] is a
/// **typed** "no transcript recorded", not an empty transcript: a run
/// recorded before this milestone has no transcript, and a caller must be
/// able to tell that apart from a transcript that exists and holds nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRunTranscriptRead {
    /// No transcript was ever recorded for this run.
    Absent,
    Present(AgentRunTranscriptPage),
}

#[derive(Debug, Error)]
pub enum AgentRunTranscriptError {
    #[error("storage failure: {0}")]
    Storage(String),
}

fn storage_error(error: impl std::fmt::Display) -> AgentRunTranscriptError {
    AgentRunTranscriptError::Storage(error.to_string())
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// The durable store of delegated-run transcripts.
#[async_trait]
// Stable >=1.99 flags the #[must_use] injected by the async-trait expansion
// (the desugared future is already must_use). Suppressed at the macro site;
// no behavior change.
#[allow(clippy::double_must_use)]
pub trait AgentRunTranscriptStore: Send + Sync {
    /// Append one row for `run_id`. The body is the **unredacted** text;
    /// redaction and both bounds are applied here, at write, before anything
    /// is durable.
    async fn append(
        &self,
        run_id: &AgentRunId,
        role: TranscriptRole,
        kind: TranscriptKind,
        body: &str,
    ) -> Result<TranscriptAppendOutcome, AgentRunTranscriptError>;

    /// Read at most `limit` rows for `run_id`, starting at `from_ordinal`.
    /// The read is always bounded: `limit` is clamped to
    /// [`MAX_TRANSCRIPT_READ_LIMIT`], and there is no unbounded read.
    async fn read(
        &self,
        run_id: &AgentRunId,
        from_ordinal: i64,
        limit: usize,
    ) -> Result<AgentRunTranscriptRead, AgentRunTranscriptError>;
}

/// Redact one body with the repository's existing recogniser.
///
/// The recogniser is [`ProjectionFieldRedactor`], the same one the projection
/// seam and the durable run failure message already use. Two passes are
/// applied to every body: the kind's own rule set, and the environment rule
/// set, so an environment assignment has its **value** replaced wherever it
/// appears rather than only where it is spelled `KEY=value` at the start of a
/// line. The pass fails closed: a refusal or an over-long input yields a
/// marker, never the original text.
pub fn redact_transcript_body(kind: TranscriptKind, body: &str) -> String {
    let redactor = ProjectionFieldRedactor::new();
    let primary = match kind {
        TranscriptKind::ToolCall => FieldName::ToolArgument,
        TranscriptKind::ToolResult => FieldName::ToolOutput,
        TranscriptKind::ModelMessage | TranscriptKind::TerminalNote => FieldName::Text,
    };
    let mut current = body.to_owned();
    for field in [primary, FieldName::Environment] {
        match redactor.redact_text(field, &current) {
            RedactionResult::Unchanged => {}
            RedactionResult::Redacted { text, .. } => current = text,
            RedactionResult::Downgraded { marker, .. } => return marker,
            RedactionResult::Failed { .. } => return REDACTION_REFUSED_MARKER.to_owned(),
        }
    }
    current
}

/// A body prepared for storage: redacted, bounded, and sized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTranscriptRow {
    pub body: String,
    pub byte_len: i64,
    /// Whether the row bound truncated the body.
    pub truncated: bool,
}

/// The shared preparation every store implementation applies before it
/// writes: redact, then bound. The order matters — redaction runs over a
/// prefix large enough to hold a credential whole, and the row bound is
/// applied to the **redacted** text, so the stored prefix can never contain
/// a half-redacted secret that the row bound cut in half.
pub fn prepare_transcript_row(kind: TranscriptKind, body: &str) -> PreparedTranscriptRow {
    let redacted = redact_transcript_body(kind, scan_prefix(body));
    let (bounded, truncated) = bound_row_body(&redacted);
    PreparedTranscriptRow {
        byte_len: bounded.len() as i64,
        body: bounded,
        truncated,
    }
}

/// The prefix the recogniser scans: at most the redactor's own input bound,
/// cut on a character boundary.
fn scan_prefix(body: &str) -> &str {
    let limit = crate::projection_replay::redactor::MAX_REDACTION_INPUT_BYTES;
    if body.len() <= limit {
        return body;
    }
    let mut end = limit;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    &body[..end]
}

/// Bound one redacted body to the per-row limit, marking the truncation in
/// the row.
fn bound_row_body(redacted: &str) -> (String, bool) {
    if redacted.len() <= MAX_TRANSCRIPT_ROW_BYTES {
        return (redacted.to_owned(), false);
    }
    let keep = MAX_TRANSCRIPT_ROW_BYTES.saturating_sub(TRANSCRIPT_ROW_TRUNCATION_MARKER.len());
    let mut end = keep;
    while end > 0 && !redacted.is_char_boundary(end) {
        end -= 1;
    }
    let mut body = String::with_capacity(MAX_TRANSCRIPT_ROW_BYTES);
    body.push_str(&redacted[..end]);
    body.push_str(TRANSCRIPT_ROW_TRUNCATION_MARKER);
    (body, true)
}

/// The SQLite-backed store.
pub struct SqliteAgentRunTranscriptStore {
    pool: SqlitePool,
}

impl SqliteAgentRunTranscriptStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Insert one row and advance the run's aggregate in the same transaction.
async fn insert_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: &AgentRunId,
    ordinal: i64,
    role: TranscriptRole,
    kind: TranscriptKind,
    body: &str,
    byte_len: i64,
    created_at: i64,
) -> Result<(), AgentRunTranscriptError> {
    sqlx::query(
        "INSERT INTO agent_run_transcript \
         (run_id, ordinal, role, kind, redacted_body, byte_len, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(run_id.as_str())
    .bind(ordinal)
    .bind(role.as_str())
    .bind(kind.as_str())
    .bind(body)
    .bind(byte_len)
    .bind(created_at)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

/// The per-run aggregate a read reports: the next ordinal, the stored bytes
/// and the truncation flag.
struct RunAggregate {
    next_ordinal: i64,
    total_bytes: i64,
    truncated: bool,
}

async fn load_aggregate(
    pool: &SqlitePool,
    run_id: &AgentRunId,
) -> Result<Option<RunAggregate>, AgentRunTranscriptError> {
    let row = sqlx::query(
        "SELECT next_ordinal, total_bytes, truncated FROM agent_run_transcript_state \
         WHERE run_id = ?",
    )
    .bind(run_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(storage_error)?;
    Ok(row.map(|row| RunAggregate {
        next_ordinal: row.get("next_ordinal"),
        total_bytes: row.get("total_bytes"),
        truncated: row.get::<i64, _>("truncated") != 0,
    }))
}

async fn load_entries(
    pool: &SqlitePool,
    run_id: &AgentRunId,
    from_ordinal: i64,
    limit: i64,
) -> Result<Vec<AgentRunTranscriptEntry>, AgentRunTranscriptError> {
    let rows = sqlx::query(
        "SELECT ordinal, role, kind, redacted_body, byte_len, created_at \
         FROM agent_run_transcript WHERE run_id = ? AND ordinal >= ? ORDER BY ordinal ASC LIMIT ?",
    )
    .bind(run_id.as_str())
    .bind(from_ordinal)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(storage_error)?;
    rows.into_iter()
        .map(|row| {
            let role = row.get::<String, _>("role");
            let kind = row.get::<String, _>("kind");
            Ok(AgentRunTranscriptEntry {
                run_id: run_id.clone(),
                ordinal: row.get("ordinal"),
                role: TranscriptRole::from_str(&role)
                    .ok_or_else(|| storage_error(format!("unknown transcript role '{role}'")))?,
                kind: TranscriptKind::from_str(&kind)
                    .ok_or_else(|| storage_error(format!("unknown transcript kind '{kind}'")))?,
                redacted_body: row.get("redacted_body"),
                byte_len: row.get("byte_len"),
                created_at: row.get("created_at"),
            })
        })
        .collect()
}

#[async_trait]
impl AgentRunTranscriptStore for SqliteAgentRunTranscriptStore {
    async fn append(
        &self,
        run_id: &AgentRunId,
        role: TranscriptRole,
        kind: TranscriptKind,
        body: &str,
    ) -> Result<TranscriptAppendOutcome, AgentRunTranscriptError> {
        let prepared = prepare_transcript_row(kind, body);
        // The ordinal and the per-run byte total are read and advanced in one
        // writer-reserved transaction, so two concurrent appends for one run
        // cannot receive the same ordinal and neither row is lost.
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(storage_error)?;
        sqlx::query(
            "INSERT INTO agent_run_transcript_state (run_id, next_ordinal, total_bytes, truncated) \
             VALUES (?, 0, 0, 0) ON CONFLICT(run_id) DO NOTHING",
        )
        .bind(run_id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
        // Read through the transaction, on the connection that holds the
        // writer reservation, so it cannot be stale.
        let row = sqlx::query(
            "SELECT next_ordinal, total_bytes, truncated FROM agent_run_transcript_state \
             WHERE run_id = ?",
        )
        .bind(run_id.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
        let next_ordinal: i64 = row.get("next_ordinal");
        let total_bytes: i64 = row.get("total_bytes");
        let truncated: i64 = row.get("truncated");

        if truncated != 0 {
            tx.rollback().await.ok();
            return Ok(TranscriptAppendOutcome {
                status: TranscriptAppendStatus::RunTruncated,
                ordinal: next_ordinal - 1,
                byte_len: 0,
            });
        }

        let content_budget =
            (MAX_TRANSCRIPT_RUN_BYTES.saturating_sub(RUN_TRUNCATION_MARKER_RESERVE)) as i64;
        if total_bytes + prepared.byte_len > content_budget {
            // The run's bound is reached: the body is refused and a
            // truncation marker is recorded, so the tail is never lost.
            let marker_len = TRANSCRIPT_RUN_TRUNCATION_MARKER.len() as i64;
            insert_row(
                &mut tx,
                run_id,
                next_ordinal,
                TranscriptRole::Orchestrator,
                TranscriptKind::TerminalNote,
                TRANSCRIPT_RUN_TRUNCATION_MARKER,
                marker_len,
                now_ms(),
            )
            .await?;
            sqlx::query(
                "UPDATE agent_run_transcript_state \
                 SET next_ordinal = next_ordinal + 1, total_bytes = total_bytes + ?, truncated = 1 \
                 WHERE run_id = ?",
            )
            .bind(marker_len)
            .bind(run_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
            tx.commit().await.map_err(storage_error)?;
            return Ok(TranscriptAppendOutcome {
                status: TranscriptAppendStatus::RunTruncated,
                ordinal: next_ordinal,
                byte_len: marker_len,
            });
        }

        insert_row(
            &mut tx,
            run_id,
            next_ordinal,
            role,
            kind,
            &prepared.body,
            prepared.byte_len,
            now_ms(),
        )
        .await?;
        sqlx::query(
            "UPDATE agent_run_transcript_state \
             SET next_ordinal = next_ordinal + 1, total_bytes = total_bytes + ? WHERE run_id = ?",
        )
        .bind(prepared.byte_len)
        .bind(run_id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(TranscriptAppendOutcome {
            status: if prepared.truncated {
                TranscriptAppendStatus::RowTruncated
            } else {
                TranscriptAppendStatus::Stored
            },
            ordinal: next_ordinal,
            byte_len: prepared.byte_len,
        })
    }

    async fn read(
        &self,
        run_id: &AgentRunId,
        from_ordinal: i64,
        limit: usize,
    ) -> Result<AgentRunTranscriptRead, AgentRunTranscriptError> {
        let limit = limit.min(MAX_TRANSCRIPT_READ_LIMIT) as i64;
        let Some(aggregate) = load_aggregate(&self.pool, run_id).await? else {
            return Ok(AgentRunTranscriptRead::Absent);
        };
        let entries = load_entries(&self.pool, run_id, from_ordinal, limit).await?;
        let has_more = aggregate.next_ordinal > from_ordinal + entries.len() as i64;
        Ok(AgentRunTranscriptRead::Present(AgentRunTranscriptPage {
            entries,
            has_more,
            truncated: aggregate.truncated,
            total_bytes: aggregate.total_bytes,
            total_entries: aggregate.next_ordinal,
        }))
    }
}

#[derive(Default)]
struct MemoryTranscriptState {
    next_ordinal: i64,
    total_bytes: i64,
    truncated: bool,
    entries: Vec<AgentRunTranscriptEntry>,
}

/// The in-memory store, for tests and for callers that hold no database.
#[derive(Default)]
pub struct InMemoryAgentRunTranscriptStore {
    state: Mutex<HashMap<AgentRunId, MemoryTranscriptState>>,
}

impl InMemoryAgentRunTranscriptStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentRunTranscriptStore for InMemoryAgentRunTranscriptStore {
    async fn append(
        &self,
        run_id: &AgentRunId,
        role: TranscriptRole,
        kind: TranscriptKind,
        body: &str,
    ) -> Result<TranscriptAppendOutcome, AgentRunTranscriptError> {
        let prepared = prepare_transcript_row(kind, body);
        let mut state = self.state.lock().await;
        let run = state.entry(run_id.clone()).or_default();
        if run.truncated {
            return Ok(TranscriptAppendOutcome {
                status: TranscriptAppendStatus::RunTruncated,
                ordinal: run.next_ordinal - 1,
                byte_len: 0,
            });
        }
        let content_budget =
            (MAX_TRANSCRIPT_RUN_BYTES.saturating_sub(RUN_TRUNCATION_MARKER_RESERVE)) as i64;
        if run.total_bytes + prepared.byte_len > content_budget {
            let marker_len = TRANSCRIPT_RUN_TRUNCATION_MARKER.len() as i64;
            let ordinal = run.next_ordinal;
            run.entries.push(AgentRunTranscriptEntry {
                run_id: run_id.clone(),
                ordinal,
                role: TranscriptRole::Orchestrator,
                kind: TranscriptKind::TerminalNote,
                redacted_body: TRANSCRIPT_RUN_TRUNCATION_MARKER.to_owned(),
                byte_len: marker_len,
                created_at: now_ms(),
            });
            run.next_ordinal += 1;
            run.total_bytes += marker_len;
            run.truncated = true;
            return Ok(TranscriptAppendOutcome {
                status: TranscriptAppendStatus::RunTruncated,
                ordinal,
                byte_len: marker_len,
            });
        }
        let ordinal = run.next_ordinal;
        run.entries.push(AgentRunTranscriptEntry {
            run_id: run_id.clone(),
            ordinal,
            role,
            kind,
            redacted_body: prepared.body.clone(),
            byte_len: prepared.byte_len,
            created_at: now_ms(),
        });
        run.next_ordinal += 1;
        run.total_bytes += prepared.byte_len;
        Ok(TranscriptAppendOutcome {
            status: if prepared.truncated {
                TranscriptAppendStatus::RowTruncated
            } else {
                TranscriptAppendStatus::Stored
            },
            ordinal,
            byte_len: prepared.byte_len,
        })
    }

    async fn read(
        &self,
        run_id: &AgentRunId,
        from_ordinal: i64,
        limit: usize,
    ) -> Result<AgentRunTranscriptRead, AgentRunTranscriptError> {
        let limit = limit.min(MAX_TRANSCRIPT_READ_LIMIT) as i64;
        let state = self.state.lock().await;
        let Some(run) = state.get(run_id) else {
            return Ok(AgentRunTranscriptRead::Absent);
        };
        let entries: Vec<AgentRunTranscriptEntry> = run
            .entries
            .iter()
            .filter(|entry| entry.ordinal >= from_ordinal)
            .take(limit.max(0) as usize)
            .cloned()
            .collect();
        let has_more = run.next_ordinal > from_ordinal + entries.len() as i64;
        Ok(AgentRunTranscriptRead::Present(AgentRunTranscriptPage {
            entries,
            has_more,
            truncated: run.truncated,
            total_bytes: run.total_bytes,
            total_entries: run.next_ordinal,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{AgentTaskId, ProjectId};
    use crate::workspace::WorkspaceId;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    use super::super::agent_run::AgentRunStore;

    /// A migrated in-memory database with one connection.
    async fn isolated_pool() -> SqlitePool {
        let name = format!("transcript_{}", uuid::Uuid::new_v4().simple());
        let url = format!("file:{name}?mode=memory&cache=shared");
        let options = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(10))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("connect in-memory sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrations run inside the helper");
        pool
    }

    /// A file-backed database with several connections, so two writers can
    /// contend for the same table the way two daemon tasks do.
    async fn file_pool(directory: &std::path::Path) -> SqlitePool {
        let url = format!("sqlite://{}/transcript.db", directory.display());
        let options = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(10))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("connect file-backed sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrations run inside the helper");
        pool
    }

    /// Create a real delegated run, so a transcript row's foreign key has a
    /// target.
    async fn create_run(pool: &SqlitePool) -> AgentRunId {
        let store = super::super::agent_run::SqliteAgentRunStore::new(pool.clone());
        let submission = store
            .create_or_get(
                super::super::agent_run::NewAgentTask {
                    task_id: AgentTaskId::new(),
                    parent_task_id: None,
                    originating_session_id: "session".into(),
                    originating_turn_id: Some("turn".into()),
                    project_id: ProjectId::new(),
                    repository_id: None,
                    workspace_id: WorkspaceId::new_unchecked("workspace"),
                    requested_agent: "general".into(),
                    delegation_key: format!("delegation-{}", uuid::Uuid::new_v4().simple()),
                    request_fingerprint: "fingerprint".into(),
                    description: "bounded task".into(),
                },
                super::super::agent_run::NewAgentRun {
                    run_id: AgentRunId::new(),
                    parent_run_id: None,
                    depth: 1,
                    workspace_id: WorkspaceId::new_unchecked("workspace"),
                    agent_name: "general".into(),
                    agent_digest: None,
                    provider: "openai".into(),
                    model: "model".into(),
                    authority_digest: "authority".into(),
                    budget: Default::default(),
                    shared_scratch: Vec::new(),
                },
            )
            .await
            .expect("create the delegated run");
        submission.run.run_id
    }

    /// The stored row, read straight out of SQLite rather than through the
    /// store's read API, so an assertion about it is about the durable bytes.
    async fn stored_body(pool: &SqlitePool, run_id: &AgentRunId, ordinal: i64) -> String {
        sqlx::query_scalar(
            "SELECT redacted_body FROM agent_run_transcript WHERE run_id = ? AND ordinal = ?",
        )
        .bind(run_id.as_str())
        .bind(ordinal)
        .fetch_one(pool)
        .await
        .expect("the stored row")
    }

    fn page(read: AgentRunTranscriptRead) -> AgentRunTranscriptPage {
        match read {
            AgentRunTranscriptRead::Present(page) => page,
            AgentRunTranscriptRead::Absent => panic!("expected a transcript to be present"),
        }
    }

    #[tokio::test]
    async fn appended_rows_read_back_in_ordinal_order() {
        let pool = isolated_pool().await;
        let run_id = create_run(&pool).await;
        let store = SqliteAgentRunTranscriptStore::new(pool.clone());

        let mut ordinals = Vec::new();
        for index in 0..4 {
            let outcome = store
                .append(
                    &run_id,
                    TranscriptRole::Orchestrator,
                    TranscriptKind::ModelMessage,
                    &format!("message {index}"),
                )
                .await
                .unwrap();
            ordinals.push(outcome.ordinal);
        }

        let read = page(store.read(&run_id, 0, 16).await.unwrap());
        assert_eq!(
            read.entries
                .iter()
                .map(|entry| entry.ordinal)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3],
            "rows must read back in the order they were appended"
        );
        assert_eq!(
            read.entries
                .iter()
                .map(|entry| entry.redacted_body.as_str())
                .collect::<Vec<_>>(),
            vec!["message 0", "message 1", "message 2", "message 3"],
            "each row must hold the body that was appended"
        );
        assert_eq!(ordinals, vec![0, 1, 2, 3]);
        assert!(!read.has_more);
        assert!(!read.truncated);
    }

    #[tokio::test]
    async fn the_per_row_bound_truncates_and_marks_the_stored_row() {
        let pool = isolated_pool().await;
        let run_id = create_run(&pool).await;
        let store = SqliteAgentRunTranscriptStore::new(pool.clone());
        let oversized = "x".repeat(MAX_TRANSCRIPT_ROW_BYTES + 512);

        let outcome = store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ToolResult,
                &oversized,
            )
            .await
            .unwrap();

        assert_eq!(outcome.status, TranscriptAppendStatus::RowTruncated);
        assert_eq!(
            outcome.byte_len, MAX_TRANSCRIPT_ROW_BYTES as i64,
            "the stored row must be bounded by the per-row limit"
        );
        let stored = stored_body(&pool, &run_id, 0).await;
        assert!(
            stored.ends_with(TRANSCRIPT_ROW_TRUNCATION_MARKER),
            "a truncated row must carry the truncation marker, got: {stored:?}"
        );
    }

    #[tokio::test]
    async fn the_per_run_bound_stops_appending_and_records_a_truncation_marker() {
        let pool = isolated_pool().await;
        let run_id = create_run(&pool).await;
        let store = SqliteAgentRunTranscriptStore::new(pool.clone());
        let full_row = "y".repeat(MAX_TRANSCRIPT_ROW_BYTES);

        let mut outcomes = Vec::new();
        for _ in 0..128 {
            outcomes.push(
                store
                    .append(
                        &run_id,
                        TranscriptRole::Orchestrator,
                        TranscriptKind::ToolResult,
                        &full_row,
                    )
                    .await
                    .unwrap(),
            );
            if outcomes
                .last()
                .is_some_and(|outcome| outcome.status == TranscriptAppendStatus::RunTruncated)
            {
                break;
            }
        }

        let refused = outcomes.last().expect("the run bound must be reached");
        assert_eq!(
            refused.status,
            TranscriptAppendStatus::RunTruncated,
            "the append past the per-run bound must be refused, not stored"
        );
        let read = page(
            store
                .read(&run_id, 0, MAX_TRANSCRIPT_READ_LIMIT)
                .await
                .unwrap(),
        );
        assert!(
            read.truncated,
            "the transcript must state that it is truncated"
        );
        let last = read.entries.last().expect("a truncation marker row");
        assert_eq!(last.kind, TranscriptKind::TerminalNote);
        assert_eq!(last.redacted_body, TRANSCRIPT_RUN_TRUNCATION_MARKER);
        assert!(
            read.total_bytes <= MAX_TRANSCRIPT_RUN_BYTES as i64,
            "the run must never store more than the per-run bound"
        );

        let after = store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                "one more",
            )
            .await
            .unwrap();
        assert_eq!(after.status, TranscriptAppendStatus::RunTruncated);
        let reread = page(
            store
                .read(&run_id, 0, MAX_TRANSCRIPT_READ_LIMIT)
                .await
                .unwrap(),
        );
        assert_eq!(
            reread.entries.len(),
            read.entries.len(),
            "a refused append must not add a row"
        );
    }

    #[tokio::test]
    async fn a_known_secret_is_absent_from_the_stored_row() {
        let pool = isolated_pool().await;
        let run_id = create_run(&pool).await;
        let store = SqliteAgentRunTranscriptStore::new(pool.clone());
        let api_secret = "fixture-api-key-value";
        let bearer_secret = "fixture-bearer-token-0123456789";
        // The AWS documentation example key. It is the repository's existing
        // fixture for a credential-shaped value (`AKIAIOSFODNN7EXAMPLE` is
        // used the same way in `collaboration.rs`, `eggsentry` and
        // `shell/redactor.rs`), and it is not a live credential. The
        // redaction rules match the *assignment* form, not the value's
        // entropy, so the assertion below is exactly as strong with it.
        let env_secret = "AKIAIOSFODNN7EXAMPLE";
        let body = format!(
            "api_key={api_secret}\nAuthorization: Bearer {bearer_secret}\n\
             AWS_SECRET_ACCESS_KEY={env_secret}\n"
        );

        store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ToolResult,
                &body,
            )
            .await
            .unwrap();

        let stored = stored_body(&pool, &run_id, 0).await;
        for secret in [api_secret, bearer_secret, env_secret] {
            assert!(
                !stored.contains(secret),
                "the secret must be absent from the stored row, got: {stored:?}"
            );
        }
        assert!(
            stored.contains("[REDACTED"),
            "the stored row must show that a redaction happened, got: {stored:?}"
        );
    }

    #[tokio::test]
    async fn concurrent_appends_receive_distinct_ordinals_and_lose_no_row() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let pool = file_pool(directory.path()).await;
        let run_id = create_run(&pool).await;
        let store = std::sync::Arc::new(SqliteAgentRunTranscriptStore::new(pool.clone()));

        let mut tasks = Vec::new();
        for worker in 0..2 {
            let store = store.clone();
            let run_id = run_id.clone();
            tasks.push(tokio::spawn(async move {
                let mut ordinals = Vec::new();
                for index in 0..25 {
                    let outcome = store
                        .append(
                            &run_id,
                            TranscriptRole::Orchestrator,
                            TranscriptKind::ModelMessage,
                            &format!("worker {worker} row {index}"),
                        )
                        .await
                        .unwrap();
                    ordinals.push(outcome.ordinal);
                }
                ordinals
            }));
        }
        let mut ordinals = Vec::new();
        for task in tasks {
            ordinals.extend(task.await.expect("the append task completed"));
        }

        let mut sorted = ordinals.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            50,
            "every append must receive a distinct ordinal"
        );
        assert_eq!(
            sorted.first().copied(),
            Some(0),
            "the ordinals must form one unbroken sequence"
        );
        assert_eq!(sorted.last().copied(), Some(49));

        let read = page(
            store
                .read(&run_id, 0, MAX_TRANSCRIPT_READ_LIMIT)
                .await
                .unwrap(),
        );
        assert_eq!(read.entries.len(), 50, "no row may be lost");
        assert_eq!(read.total_entries, 50);
    }

    #[tokio::test]
    async fn a_read_is_bounded_and_reports_whether_more_rows_exist() {
        let pool = isolated_pool().await;
        let run_id = create_run(&pool).await;
        let store = SqliteAgentRunTranscriptStore::new(pool.clone());
        for index in 0..5 {
            store
                .append(
                    &run_id,
                    TranscriptRole::Orchestrator,
                    TranscriptKind::ModelMessage,
                    &format!("row {index}"),
                )
                .await
                .unwrap();
        }

        let first = page(store.read(&run_id, 0, 2).await.unwrap());
        assert_eq!(first.entries.len(), 2, "the limit must be honoured");
        assert!(first.has_more, "the page must report that more rows exist");

        let whole = page(store.read(&run_id, 0, 4096).await.unwrap());
        assert_eq!(whole.entries.len(), 5);
        assert!(
            !whole.has_more,
            "the last page must report that no more rows exist"
        );

        let tail = page(store.read(&run_id, 3, 10).await.unwrap());
        assert_eq!(
            tail.entries
                .iter()
                .map(|entry| entry.ordinal)
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert!(!tail.has_more);

        let past = page(store.read(&run_id, 99, 10).await.unwrap());
        assert!(past.entries.is_empty());
        assert!(!past.has_more);
    }

    #[tokio::test]
    async fn a_run_without_a_transcript_reads_as_absent_not_as_an_empty_transcript() {
        let pool = isolated_pool().await;
        let run_id = create_run(&pool).await;
        let store = SqliteAgentRunTranscriptStore::new(pool.clone());

        let read = store.read(&run_id, 0, 16).await.unwrap();
        assert!(
            matches!(read, AgentRunTranscriptRead::Absent),
            "a run recorded before this milestone must read as 'no transcript recorded'"
        );

        store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                "recorded",
            )
            .await
            .unwrap();
        let present = store.read(&run_id, 0, 16).await.unwrap();
        assert!(
            matches!(present, AgentRunTranscriptRead::Present(_)),
            "a run with a transcript must not read as absent"
        );
    }

    #[tokio::test]
    async fn a_crash_mid_append_leaves_no_partial_row_and_keeps_the_recorded_result() {
        let pool = isolated_pool().await;
        let run_id = create_run(&pool).await;
        let store = SqliteAgentRunTranscriptStore::new(pool.clone());
        store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                "the first message",
            )
            .await
            .unwrap();
        let result = crate::run_result::AgentRunResult {
            run_id: run_id.clone(),
            status: crate::run_result::AgentRunResultStatus::Succeeded,
            summary: "the recorded result".into(),
            worktree_id: None,
            base_commit: None,
            result_commit: None,
            changed_paths: Vec::new(),
            validation: Vec::new(),
            findings: Vec::new(),
            artifacts: Vec::new(),
            repository_state: crate::run_result::RepositoryState::Clean,
            retryability: crate::run_result::Retryability::NotRetryable,
            recovery_hint: None,
        };
        let run_store = super::super::agent_run::SqliteAgentRunStore::new(pool.clone());
        run_store.save_result(result.clone()).await.unwrap();

        // The crash: the transaction that would have written the next row is
        // dropped without committing.
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        sqlx::query(
            "INSERT INTO agent_run_transcript \
             (run_id, ordinal, role, kind, redacted_body, byte_len, created_at) \
             VALUES (?, 1, 'orchestrator', 'model_message', 'partial', 7, 0)",
        )
        .bind(run_id.as_str())
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE agent_run_transcript_state \
             SET next_ordinal = 2, total_bytes = total_bytes + 7 WHERE run_id = ?",
        )
        .bind(run_id.as_str())
        .execute(&mut *tx)
        .await
        .unwrap();
        drop(tx);

        let read = page(store.read(&run_id, 0, 16).await.unwrap());
        assert_eq!(
            read.entries.len(),
            1,
            "the crashed append must leave no readable row"
        );
        assert_eq!(read.entries[0].redacted_body, "the first message");
        let next = store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                "after the crash",
            )
            .await
            .unwrap();
        assert_eq!(
            next.ordinal, 1,
            "the crashed append must not consume an ordinal"
        );
        assert_eq!(
            run_store.get_result(&run_id).await.unwrap(),
            Some(result),
            "the run's recorded result must survive the crash untouched"
        );
    }

    #[tokio::test]
    async fn a_restarted_store_reads_a_previously_written_transcript_back_intact() {
        let pool = isolated_pool().await;
        let run_id = create_run(&pool).await;
        let store = SqliteAgentRunTranscriptStore::new(pool.clone());
        store
            .append(
                &run_id,
                TranscriptRole::Internal,
                TranscriptKind::ModelMessage,
                "the child's own message",
            )
            .await
            .unwrap();
        store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ToolResult,
                "the tool result",
            )
            .await
            .unwrap();

        // A restart: a fresh store over the same database.
        let restarted = SqliteAgentRunTranscriptStore::new(pool.clone());
        let read = page(restarted.read(&run_id, 0, 16).await.unwrap());
        assert_eq!(
            read.entries
                .iter()
                .map(|entry| (entry.ordinal, entry.role, entry.kind))
                .collect::<Vec<_>>(),
            vec![
                (0, TranscriptRole::Internal, TranscriptKind::ModelMessage),
                (1, TranscriptRole::Orchestrator, TranscriptKind::ToolResult),
            ]
        );
        assert_eq!(read.entries[1].redacted_body, "the tool result");
    }

    #[tokio::test]
    async fn the_migration_applies_to_a_database_that_already_holds_delegated_runs() {
        let pool = isolated_pool().await;
        let run_store = super::super::agent_run::SqliteAgentRunStore::new(pool.clone());
        let run_id = create_run(&pool).await;

        // Rewind the database to the state before this migration: the
        // transcript tables do not exist and the layout marker is v88.
        sqlx::query("DROP TABLE agent_run_transcript")
            .execute(&pool)
            .await
            .expect("drop the transcript table");
        sqlx::query("DROP TABLE agent_run_transcript_state")
            .execute(&pool)
            .await
            .expect("drop the transcript state table");
        sqlx::query("UPDATE migration_version SET version = 88 WHERE id = 1")
            .execute(&pool)
            .await
            .expect("rewind the layout marker");

        crate::session::schema::migrate(&pool)
            .await
            .expect("the migration applies over a database with delegated runs");

        let version: i64 = sqlx::query_scalar("SELECT version FROM migration_version WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(version, crate::storage::STORAGE_LAYOUT_VERSION as i64);
        assert!(
            run_store.get_run(&run_id).await.unwrap().is_some(),
            "the delegated run must survive the migration"
        );

        let store = SqliteAgentRunTranscriptStore::new(pool.clone());
        let outcome = store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                "after the migration",
            )
            .await
            .unwrap();
        assert_eq!(outcome.ordinal, 0);
    }
}
