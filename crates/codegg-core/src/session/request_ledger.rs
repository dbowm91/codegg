//! M002: the durable request ledger — one row per model request attempt.
//!
//! `record_attempt` is the only writer: it opens `BEGIN IMMEDIATE` because it
//! counts and prunes rows before it inserts, and `settle_attempt` only updates
//! the row that writer created. Both retention bounds —
//! `MAX_REQUEST_LEDGER_ROWS_PER_SESSION` and `MAX_REQUEST_LEDGER_ROWS_TOTAL` —
//! are enforced in that same transaction, so no second writer can bypass them.
//! The table-wide bound exists because a one-shot invocation's session id is
//! new on every run: it never reaches its own per-session bound, so only a
//! table-wide one can bound it.
//!
//! A body is stored only for an attempt whose caller supplies one (the
//! failure path), redacted here at write. Digests are over the dispatched
//! request, never over the body, so a body cannot recompute them.
//! `streaming` is the row a process that died mid-attempt leaves behind.

use sqlx::{FromRow, SqlitePool};
use std::collections::BTreeMap;
use thiserror::Error;
use uuid::Uuid;

use crate::storage::immediate::begin_immediate;

/// The per-session retained-row bound enforced by the writer.
pub const MAX_REQUEST_LEDGER_ROWS_PER_SESSION: i64 = 256;

/// The table-wide retained-row bound enforced by the writer.
///
/// A per-session bound cannot bound a **one-shot** invocation, because its
/// session id is minted for that run and never recurs, so no session ever
/// reaches 256 rows. This bound is the table's, so every writer statement is
/// measured against it whatever the session id was. It is deliberately a
/// multiple of the per-session bound: it never binds before 32 sessions have
/// each filled their own.
pub const MAX_REQUEST_LEDGER_ROWS_TOTAL: i64 = MAX_REQUEST_LEDGER_ROWS_PER_SESSION * 32;

/// The largest stored body, in bytes.
pub const REQUEST_LEDGER_MAX_BODY_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnRequestOutcome {
    Streaming,
    Ok,
    Failed,
    Cancelled,
}

impl TurnRequestOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, RequestLedgerError> {
        match value {
            "streaming" => Ok(Self::Streaming),
            "ok" => Ok(Self::Ok),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(RequestLedgerError::Invalid {
                field: "outcome",
                message: format!("unknown stored outcome {other:?}"),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRequestRecord {
    pub id: String,
    pub session_id: String,
    pub turn_index: i64,
    pub attempt_index: i64,
    pub attempt_id: String,
    pub model: String,
    pub effort: Option<String>,
    pub system_digest: String,
    pub tools_digest: String,
    pub message_digest: String,
    pub message_count: i64,
    pub section_digests: Option<BTreeMap<String, String>>,
    pub outcome: TurnRequestOutcome,
    pub body: Option<String>,
    pub created_at_ms: i64,
}

/// One attempt to record; `(session_id, turn_index, attempt_index)` is unique.
#[derive(Debug, Clone)]
pub struct TurnRequestAttempt<'a> {
    pub session_id: &'a str,
    pub turn_index: i64,
    pub attempt_index: i64,
    pub attempt_id: &'a str,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub system_digest: &'a str,
    pub tools_digest: &'a str,
    pub message_digest: &'a str,
    pub message_count: i64,
    pub section_digests: Option<&'a BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLedgerHealth {
    pub total_rows: i64,
    pub last: Option<TurnRequestRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RequestLedgerError {
    #[error("invalid request ledger field: {field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
    #[error("this attempt is already recorded for the turn")]
    DuplicateAttempt,
    #[error("request ledger storage is unavailable: {0}")]
    Storage(String),
}

fn storage_error(error: sqlx::Error) -> RequestLedgerError {
    if let sqlx::Error::Database(database) = &error {
        if database.is_unique_violation() {
            return RequestLedgerError::DuplicateAttempt;
        }
    }
    RequestLedgerError::Storage(error.to_string())
}

/// True when the ledger table is absent: an older store reports no rows.
pub fn is_missing_table(error: &RequestLedgerError) -> bool {
    matches!(error, RequestLedgerError::Storage(message) if message.contains("no such table"))
}

const SELECT_COLUMNS: &str = "id, session_id, turn_index, attempt_index, attempt_id, model, \
     effort, system_digest, tools_digest, message_digest, message_count, section_digests, \
     outcome, body, created_at_ms";

#[derive(Debug, Clone, FromRow)]
struct LedgerRow {
    id: String,
    session_id: String,
    turn_index: i64,
    attempt_index: i64,
    attempt_id: String,
    model: String,
    effort: Option<String>,
    system_digest: String,
    tools_digest: String,
    message_digest: String,
    message_count: i64,
    section_digests: Option<String>,
    outcome: String,
    body: Option<String>,
    created_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct TurnRequestLedgerStore {
    pool: SqlitePool,
}

impl TurnRequestLedgerStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// The next per-session turn ordinal; retention keeps the maximum.
    pub async fn next_turn_index(&self, session_id: &str) -> Result<i64, RequestLedgerError> {
        let highest: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(turn_index), 0) FROM turn_request_ledger WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(highest + 1)
    }

    /// Write one attempt, pruned to both retention bounds, before dispatch.
    ///
    /// The oldest rows are removed first, and "oldest" is insertion order:
    /// `rowid` on this table is assigned in the same order `created_at_ms`
    /// records, and it is the table's own ordering, so the prune needs no
    /// sort.
    pub async fn record_attempt(
        &self,
        attempt: TurnRequestAttempt<'_>,
    ) -> Result<String, RequestLedgerError> {
        validate_attempt(&attempt)?;
        let section_digests = attempt
            .section_digests
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| RequestLedgerError::Invalid {
                field: "section_digests",
                message: error.to_string(),
            })?;
        let id = Uuid::new_v4().to_string();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut tx = begin_immediate(&self.pool).await.map_err(storage_error)?;
        let retained: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM turn_request_ledger WHERE session_id = ?")
                .bind(attempt.session_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(storage_error)?;
        if retained >= MAX_REQUEST_LEDGER_ROWS_PER_SESSION {
            let excess = retained - MAX_REQUEST_LEDGER_ROWS_PER_SESSION + 1;
            sqlx::query(
                "DELETE FROM turn_request_ledger WHERE id IN (SELECT id FROM turn_request_ledger \
                 WHERE session_id = ? ORDER BY turn_index ASC, attempt_index ASC LIMIT ?)",
            )
            .bind(attempt.session_id)
            .bind(excess)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
        }
        let retained_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM turn_request_ledger")
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
        if retained_total >= MAX_REQUEST_LEDGER_ROWS_TOTAL {
            let excess = retained_total - MAX_REQUEST_LEDGER_ROWS_TOTAL + 1;
            sqlx::query(
                "DELETE FROM turn_request_ledger WHERE id IN (SELECT id FROM turn_request_ledger \
                 ORDER BY rowid ASC LIMIT ?)",
            )
            .bind(excess)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
        }
        sqlx::query(
            "INSERT INTO turn_request_ledger (id, session_id, turn_index, attempt_index, \
             attempt_id, model, effort, system_digest, tools_digest, message_digest, \
             message_count, section_digests, outcome, body, created_at_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'streaming', NULL, ?)",
        )
        .bind(&id)
        .bind(attempt.session_id)
        .bind(attempt.turn_index)
        .bind(attempt.attempt_index)
        .bind(attempt.attempt_id)
        .bind(attempt.model)
        .bind(attempt.effort)
        .bind(attempt.system_digest)
        .bind(attempt.tools_digest)
        .bind(attempt.message_digest)
        .bind(attempt.message_count)
        .bind(section_digests)
        .bind(now_ms)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(id)
    }

    /// Update the writer's row in place; this never inserts a second row.
    pub async fn settle_attempt(
        &self,
        id: &str,
        outcome: TurnRequestOutcome,
        body: Option<&str>,
    ) -> Result<bool, RequestLedgerError> {
        if matches!(outcome, TurnRequestOutcome::Streaming) {
            return Err(RequestLedgerError::Invalid {
                field: "outcome",
                message: "an attempt is recorded as streaming only by the writer".to_owned(),
            });
        }
        let body = body.map(prepare_body);
        let updated =
            sqlx::query("UPDATE turn_request_ledger SET outcome = ?, body = ? WHERE id = ?")
                .bind(outcome.as_str())
                .bind(body)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(storage_error)?
                .rows_affected();
        Ok(updated > 0)
    }

    pub async fn turn_attempts(
        &self,
        session_id: &str,
        turn_index: i64,
    ) -> Result<Vec<TurnRequestRecord>, RequestLedgerError> {
        let rows = sqlx::query_as::<_, LedgerRow>(&format!(
            "SELECT {SELECT_COLUMNS} FROM turn_request_ledger \
             WHERE session_id = ? AND turn_index = ? ORDER BY attempt_index ASC"
        ))
        .bind(session_id)
        .bind(turn_index)
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        rows.into_iter().map(record_from_row).collect()
    }

    pub async fn latest_attempt(&self) -> Result<Option<TurnRequestRecord>, RequestLedgerError> {
        let row = sqlx::query_as::<_, LedgerRow>(&format!(
            "SELECT {SELECT_COLUMNS} FROM turn_request_ledger \
             ORDER BY turn_index DESC, attempt_index DESC, created_at_ms DESC LIMIT 1"
        ))
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        row.map(record_from_row).transpose()
    }

    pub async fn ledger_health(&self) -> Result<RequestLedgerHealth, RequestLedgerError> {
        let total_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM turn_request_ledger")
            .fetch_one(&self.pool)
            .await
            .map_err(storage_error)?;
        Ok(RequestLedgerHealth {
            total_rows,
            last: self.latest_attempt().await?,
        })
    }
}

fn validate_attempt(attempt: &TurnRequestAttempt<'_>) -> Result<(), RequestLedgerError> {
    let required = [
        ("session_id", attempt.session_id),
        ("attempt_id", attempt.attempt_id),
        ("model", attempt.model),
        ("system_digest", attempt.system_digest),
        ("tools_digest", attempt.tools_digest),
        ("message_digest", attempt.message_digest),
    ];
    for (field, value) in required {
        if value.is_empty() {
            return Err(RequestLedgerError::Invalid {
                field,
                message: "the field is empty".to_owned(),
            });
        }
    }
    if attempt.turn_index < 0 || attempt.attempt_index < 0 || attempt.message_count < 0 {
        return Err(RequestLedgerError::Invalid {
            field: "indices",
            message: "turn, attempt and message count must not be negative".to_owned(),
        });
    }
    Ok(())
}

fn prepare_body(raw: &str) -> String {
    let redacted = crate::agent_run_transcript::redact_transcript_body(
        crate::agent_run_transcript::TranscriptKind::ToolCall,
        raw,
    );
    if redacted.len() <= REQUEST_LEDGER_MAX_BODY_BYTES {
        return redacted;
    }
    let mut end = REQUEST_LEDGER_MAX_BODY_BYTES;
    while end > 0 && !redacted.is_char_boundary(end) {
        end -= 1;
    }
    redacted[..end].to_string()
}

fn record_from_row(row: LedgerRow) -> Result<TurnRequestRecord, RequestLedgerError> {
    let section_digests = row
        .section_digests
        .map(|json| {
            serde_json::from_str::<BTreeMap<String, String>>(&json).map_err(|error| {
                RequestLedgerError::Invalid {
                    field: "section_digests",
                    message: error.to_string(),
                }
            })
        })
        .transpose()?;
    Ok(TurnRequestRecord {
        id: row.id,
        session_id: row.session_id,
        turn_index: row.turn_index,
        attempt_index: row.attempt_index,
        attempt_id: row.attempt_id,
        model: row.model,
        effort: row.effort,
        system_digest: row.system_digest,
        tools_digest: row.tools_digest,
        message_digest: row.message_digest,
        message_count: row.message_count,
        section_digests,
        outcome: TurnRequestOutcome::parse(&row.outcome)?,
        body: row.body,
        created_at_ms: row.created_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use std::time::Duration;

    const SECRET: &str = "sk-live-ABCDEF0123456789abcdef";

    fn options(url: &str) -> SqliteConnectOptions {
        SqliteConnectOptions::from_str(url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(10))
            .foreign_keys(true)
    }

    async fn isolated_pool() -> SqlitePool {
        let name = format!("ledger_{}", uuid::Uuid::new_v4().simple());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options(&format!("file:{name}?mode=memory&cache=shared")))
            .await
            .expect("connect in-memory sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrations run inside the helper");
        pool
    }

    async fn file_pool(directory: &std::path::Path, connections: u32) -> SqlitePool {
        let url = format!("sqlite://{}/ledger.db", directory.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(connections)
            .connect_with(options(&url))
            .await
            .expect("connect file sqlite");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrations run inside the helper");
        pool
    }

    fn attempt<'a>(session: &'a str, turn: i64, index: i64) -> TurnRequestAttempt<'a> {
        TurnRequestAttempt {
            session_id: session,
            turn_index: turn,
            attempt_index: index,
            attempt_id: "attempt-0000",
            model: "model-a",
            effort: Some("high"),
            system_digest: "system-digest",
            tools_digest: "tools-digest",
            message_digest: "message-digest",
            message_count: 3,
            section_digests: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn one_row_per_attempt_and_the_reader_orders_by_attempt() {
        let store = TurnRequestLedgerStore::new(isolated_pool().await);
        let first = store
            .record_attempt(attempt("session-1", 1, 0))
            .await
            .expect("first attempt recorded");
        store
            .record_attempt(attempt("session-1", 1, 1))
            .await
            .expect("retried attempt recorded");

        let rows = store
            .turn_attempts("session-1", 1)
            .await
            .expect("read the turn");
        assert_eq!(rows.len(), 2, "one row per attempt");
        assert_eq!(
            rows.iter().map(|row| row.attempt_index).collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(rows[0].id, first);
        assert_eq!(rows[0].outcome, TurnRequestOutcome::Streaming);
        assert_eq!(rows[1].model, "model-a");
        assert_eq!(rows[1].effort.as_deref(), Some("high"));
        assert_eq!(rows[1].tools_digest, "tools-digest");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_second_row_for_one_attempt_is_refused() {
        let store = TurnRequestLedgerStore::new(isolated_pool().await);
        store
            .record_attempt(attempt("session-1", 1, 0))
            .await
            .expect("first write");

        let duplicate = store.record_attempt(attempt("session-1", 1, 0)).await;
        assert_eq!(duplicate, Err(RequestLedgerError::DuplicateAttempt));
        assert_eq!(
            store
                .turn_attempts("session-1", 1)
                .await
                .expect("read the turn")
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn settling_stores_a_redacted_failure_body_and_none_for_success() {
        let store = TurnRequestLedgerStore::new(isolated_pool().await);
        let failed = store
            .record_attempt(attempt("session-1", 1, 0))
            .await
            .expect("failed attempt recorded");
        let completed = store
            .record_attempt(attempt("session-1", 1, 1))
            .await
            .expect("successful attempt recorded");

        let raw = format!("tools: bash {{\"command\": \"export API_KEY={SECRET}\"}} messages: 3");
        assert!(
            store
                .settle_attempt(&failed, TurnRequestOutcome::Failed, Some(&raw))
                .await
                .expect("settle the failed attempt"),
            "the failed attempt's row is updated"
        );
        store
            .settle_attempt(&completed, TurnRequestOutcome::Ok, None)
            .await
            .expect("settle the successful attempt");

        let rows = store
            .turn_attempts("session-1", 1)
            .await
            .expect("read the turn");
        assert_eq!(rows[0].outcome, TurnRequestOutcome::Failed);
        let body = rows[0].body.as_deref().expect("the failure retains a body");
        assert!(
            !body.contains(SECRET),
            "the stored body must not carry the secret"
        );
        assert!(
            body.contains("messages: 3"),
            "redaction replaces the secret, not the whole body"
        );
        assert_eq!(rows[1].outcome, TurnRequestOutcome::Ok);
        assert_eq!(rows[1].body, None, "a success stores no body by default");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_attempt_identity_is_required_before_any_write() {
        let store = TurnRequestLedgerStore::new(isolated_pool().await);
        let refused = store.record_attempt(attempt("", 1, 0)).await;
        assert!(
            matches!(
                refused,
                Err(RequestLedgerError::Invalid {
                    field: "session_id",
                    ..
                })
            ),
            "an empty session identity is refused: {refused:?}"
        );
        assert_eq!(
            store.ledger_health().await.expect("health").total_rows,
            0,
            "a refused write leaves no row"
        );
    }

    /// Seed `count` attempts, one per synthetic one-shot session.
    ///
    /// A one-shot invocation mints a session id per run and never returns to
    /// it, so only the table-wide bound can see those rows. They are written
    /// in one statement because the writer is not what this fixture measures.
    async fn seed_one_shot_sessions(pool: &SqlitePool, count: i64) {
        sqlx::query(
            "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < ?) \
             INSERT INTO turn_request_ledger (id, session_id, turn_index, attempt_index, \
             attempt_id, model, effort, system_digest, tools_digest, message_digest, \
             message_count, section_digests, outcome, body, created_at_ms) \
             SELECT 'seed-id-' || n, 'oneshot-' || n, 1, 0, 'seed-attempt', 'model-a', NULL, \
             'system-digest', 'tools-digest', 'message-digest', 1, NULL, 'ok', NULL, n \
             FROM seq",
        )
        .bind(count)
        .execute(pool)
        .await
        .expect("seed one-shot sessions");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retention_holds_the_bound_and_prunes_the_oldest_attempts() {
        let store = TurnRequestLedgerStore::new(isolated_pool().await);
        for turn in 1..=MAX_REQUEST_LEDGER_ROWS_PER_SESSION + 2 {
            store
                .record_attempt(attempt("session-1", turn, 0))
                .await
                .expect("record within the bound");
        }

        let health = store.ledger_health().await.expect("health");
        assert_eq!(health.total_rows, MAX_REQUEST_LEDGER_ROWS_PER_SESSION);
        let last = health.last.expect("the newest row is retained");
        assert_eq!(
            last.turn_index,
            MAX_REQUEST_LEDGER_ROWS_PER_SESSION + 2,
            "pruning removes the oldest rows, not the newest"
        );
        assert_eq!(
            store
                .turn_attempts("session-1", 1)
                .await
                .expect("read the pruned turn")
                .len(),
            0,
            "the oldest turn is gone"
        );
        assert_eq!(
            store
                .next_turn_index("session-1")
                .await
                .expect("allocate the next turn"),
            MAX_REQUEST_LEDGER_ROWS_PER_SESSION + 3,
            "the ordinal stays monotonic after pruning"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_table_bound_stops_one_shot_sessions_from_growing_the_ledger() {
        let pool = isolated_pool().await;
        let store = TurnRequestLedgerStore::new(pool.clone());
        seed_one_shot_sessions(&pool, MAX_REQUEST_LEDGER_ROWS_TOTAL - 1).await;

        store
            .record_attempt(attempt("oneshot-fresh-a", 1, 0))
            .await
            .expect("a one-shot session records its attempt");
        assert_eq!(
            store.ledger_health().await.expect("health").total_rows,
            MAX_REQUEST_LEDGER_ROWS_TOTAL,
            "the table reaches its bound and does not pass it"
        );
        assert!(
            !store
                .turn_attempts("oneshot-1", 1)
                .await
                .expect("read the oldest seeded session")
                .is_empty(),
            "the oldest row is retained while the table is under its bound"
        );

        store
            .record_attempt(attempt("oneshot-fresh-b", 1, 0))
            .await
            .expect("the next one-shot session records its attempt");
        assert_eq!(
            store.ledger_health().await.expect("health").total_rows,
            MAX_REQUEST_LEDGER_ROWS_TOTAL,
            "a write at the bound evicts before it inserts, so one-shot rows cannot accumulate"
        );
        assert!(
            store
                .turn_attempts("oneshot-1", 1)
                .await
                .expect("read the evicted session")
                .is_empty(),
            "the table-wide prune removes the oldest row whatever session it belongs to"
        );
        assert_eq!(
            store
                .latest_attempt()
                .await
                .expect("read the newest attempt")
                .expect("the newest attempt is retained")
                .session_id,
            "oneshot-fresh-b",
            "the row just written is the one retained, not the one evicted"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_reopened_store_reads_the_rows_and_reports_streaming_as_interrupted() {
        let directory = tempfile::tempdir().expect("ledger fixture directory");
        let path = directory.path();
        let store = TurnRequestLedgerStore::new(file_pool(path, 1).await);
        store
            .record_attempt(attempt("session-1", 1, 0))
            .await
            .expect("record before the restart");
        store.pool.close().await;

        let reopened = TurnRequestLedgerStore::new(file_pool(path, 1).await);
        let rows = reopened
            .turn_attempts("session-1", 1)
            .await
            .expect("read after the restart");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].outcome,
            TurnRequestOutcome::Streaming,
            "a streaming row is the honest record of a process that died"
        );
        assert_eq!(rows[0].attempt_id, "attempt-0000");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_sessions_write_concurrently_without_a_busy_error() {
        let directory = tempfile::tempdir().expect("ledger fixture directory");
        let pool = file_pool(directory.path(), 2).await;
        let store = TurnRequestLedgerStore::new(pool);

        let (left, right) = tokio::join!(
            store.record_attempt(attempt("session-left", 1, 0)),
            store.record_attempt(attempt("session-right", 1, 0))
        );
        assert!(left.is_ok(), "first session wrote: {left:?}");
        assert!(right.is_ok(), "second session wrote: {right:?}");
        assert_eq!(
            store.ledger_health().await.expect("health").total_rows,
            2,
            "both concurrent attempts are recorded"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn health_reports_no_rows_before_the_first_attempt() {
        let store = TurnRequestLedgerStore::new(isolated_pool().await);
        let health = store.ledger_health().await.expect("health");
        assert_eq!(health.total_rows, 0);
        assert_eq!(health.last, None);
        assert_eq!(
            store
                .next_turn_index("session-1")
                .await
                .expect("first index"),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn section_digests_round_trip_as_a_json_map() {
        let store = TurnRequestLedgerStore::new(isolated_pool().await);
        let sections = BTreeMap::from([
            ("harness_contract".to_owned(), "aa".to_owned()),
            ("role_contract".to_owned(), "bb".to_owned()),
        ]);
        let mut write = attempt("session-1", 1, 0);
        write.section_digests = Some(&sections);
        store.record_attempt(write).await.expect("record");

        let rows = store
            .turn_attempts("session-1", 1)
            .await
            .expect("read the turn");
        assert_eq!(rows[0].section_digests.as_ref(), Some(&sections));
    }
}
