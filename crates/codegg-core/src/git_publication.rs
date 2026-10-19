//! C007: the durable journal for private Git index publication.
//!
//! The C004 production vertical stages an authorized `git add` into a
//! **private** index in daemon-owned scratch and only then publishes that
//! index into the real repository index. Publication is a compare-and-swap
//! against the index generation the staging observed, so the window between
//! "the intent exists" and "the real index holds the published content" is a
//! window in which the daemon can die.
//!
//! This module is the recovery substrate for that window. It stores one row
//! per publication intent, with:
//!
//! * the **gitdir identity** the publication is bound to (`dev`/`ino`), so a
//!   path that was swapped underneath us is detected instead of written into;
//! * the **fingerprint of the real index** the staging observed
//!   ([`IndexFingerprint`]), which is the compare-and-swap operand;
//! * the **fingerprint of the published index**, which is what makes a
//!   committed publication distinguishable from an aborted one during
//!   recovery;
//! * the **private index path**, so recovery can either discard the staged
//!   artifact or retain it for inspection when the outcome is ambiguous.
//!
//! # Why the journal is durable rather than in-memory
//!
//! The failure this exists to survive is a *process* death, so the record of
//! the intent has to outlive the process. An in-memory map would be lost
//! exactly when it is needed, and the publication would then be
//! indistinguishable from "never started" — which is the one answer the
//! recovery path must never give by accident.
//!
//! # Why every state has a producer
//!
//! [`GitIndexPublicationState`] has five values and all five are reachable:
//! `prepared` and `publishing` are written by the publisher, `committed` by
//! the publisher after the atomic rename, and `aborted`/`ambiguous` by the
//! reconciler. There is no value that exists only as a claim.
//!
//! # Terminality
//!
//! `committed`, `aborted`, and `ambiguous` are terminal. The reconciler never
//! replays a terminal row: a committed publication is never re-applied, and an
//! ambiguous one is handed to a human rather than guessed at. The store
//! enforces this — a transition out of a terminal state is refused, not merely
//! unused.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool, TransactionManager};
use thiserror::Error;

use crate::error::StorageError;

/// The lifecycle of one publication intent.
///
/// `prepared` and `publishing` are the only non-terminal states; a row in
/// either of them is what the reconciler looks for at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitIndexPublicationState {
    /// The intent is durable. The publication has not started: no lock has
    /// been taken and the real index has not been touched.
    Prepared,
    /// The Git index lock is held and the publication is in flight. The real
    /// index may or may not have been replaced already.
    Publishing,
    /// The real index now holds the published content.
    Committed,
    /// The publication did not happen and will not be retried.
    Aborted,
    /// The outcome could not be determined from the recorded evidence.
    ///
    /// This is not a failure of the store; it is the honest answer when the
    /// real index matches neither the expected nor the published
    /// fingerprint. It is terminal because guessing is not recovery.
    Ambiguous,
}

impl GitIndexPublicationState {
    /// The stable on-disk spelling. These are the values the v84 `CHECK`
    /// constraint admits, so this list and the migration must agree.
    pub const ALL: [GitIndexPublicationState; 5] = [
        GitIndexPublicationState::Prepared,
        GitIndexPublicationState::Publishing,
        GitIndexPublicationState::Committed,
        GitIndexPublicationState::Aborted,
        GitIndexPublicationState::Ambiguous,
    ];

    /// The on-disk spelling of this state.
    pub fn as_str(&self) -> &'static str {
        match self {
            GitIndexPublicationState::Prepared => "prepared",
            GitIndexPublicationState::Publishing => "publishing",
            GitIndexPublicationState::Committed => "committed",
            GitIndexPublicationState::Aborted => "aborted",
            GitIndexPublicationState::Ambiguous => "ambiguous",
        }
    }

    /// Parse an on-disk spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == raw)
    }

    /// Whether the state is terminal.
    ///
    /// Terminal means the reconciler will not touch the row again. That is
    /// true of `ambiguous` even though it is unresolved: an ambiguous
    /// publication is escalated, not replayed.
    pub fn is_terminal(&self) -> bool {
        !matches!(
            self,
            GitIndexPublicationState::Prepared | GitIndexPublicationState::Publishing
        )
    }

    /// Whether a transition from this state to `next` is permitted.
    ///
    /// The rules are deliberately narrow:
    ///
    /// * a terminal state never transitions — a committed publication is not
    ///   re-published and an ambiguous one is not silently resolved;
    /// * `publishing` never returns to `prepared` — the lock was taken, and
    ///   "un-take" is not a thing.
    pub fn permits_transition_to(&self, next: GitIndexPublicationState) -> bool {
        match self {
            GitIndexPublicationState::Prepared => true,
            GitIndexPublicationState::Publishing => {
                !matches!(next, GitIndexPublicationState::Prepared)
            }
            GitIndexPublicationState::Committed
            | GitIndexPublicationState::Aborted
            | GitIndexPublicationState::Ambiguous => false,
        }
    }
}

impl std::fmt::Display for GitIndexPublicationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The fingerprint of a real Git index file at one generation.
///
/// The comparison operand of the publication's compare-and-swap. `Absent` is
/// a first-class value rather than a sentinel string, because "the repository
/// had no index at all" is a real state (a repository with no commit and
/// nothing staged) and it has to be distinguishable from "the index exists
/// and is empty".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexFingerprint {
    /// The index file did not exist.
    Absent,
    /// The index file existed and its bytes hashed to this SHA-256.
    Digest(String),
}

impl IndexFingerprint {
    /// The on-disk spelling. `absent` is a reserved word that no SHA-256 hex
    /// digest can collide with, because a digest is 64 lowercase hex
    /// characters and `absent` is not hex at all.
    pub fn as_str(&self) -> &str {
        match self {
            IndexFingerprint::Absent => "absent",
            IndexFingerprint::Digest(digest) => digest.as_str(),
        }
    }

    /// Parse an on-disk spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        if raw == "absent" {
            return Some(IndexFingerprint::Absent);
        }
        if raw.len() == 64 && raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Some(IndexFingerprint::Digest(raw.to_ascii_lowercase()));
        }
        None
    }
}

impl std::fmt::Display for IndexFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One durable publication intent.
///
/// This is the evidence the reconciler reasons over. It records what the
/// publication was *based on* (`expected_index_fingerprint`), what it would
/// have *produced* (`published_index_fingerprint`), and *where* the staged
/// artifact lives, so that a recovery decision is derived from recorded facts
/// rather than from a re-run of the operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexPublicationIntent {
    /// Stable identity of this intent. Also the receipt key.
    pub id: String,
    /// The workspace the mutation was authorized against.
    pub workspace_root: String,
    /// Absolute path of the real repository gitdir.
    pub gitdir_path: String,
    /// Device identity of the gitdir directory, captured at staging time.
    pub gitdir_dev: u64,
    /// Inode identity of the gitdir directory, captured at staging time.
    pub gitdir_ino: u64,
    /// Absolute path of the private staged index in daemon-owned scratch.
    pub private_index_path: String,
    /// The real index's fingerprint at the generation the staging observed.
    pub expected_index_fingerprint: IndexFingerprint,
    /// The fingerprint of the private index that was staged.
    pub published_index_fingerprint: IndexFingerprint,
    /// The repository-relative paths this intent stages.
    pub staged_paths: Vec<String>,
    /// When the intent was recorded.
    pub created_at_ms: i64,
}

/// A stored publication intent together with its current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexPublicationRecord {
    /// The intent.
    pub intent: GitIndexPublicationIntent,
    /// The current lifecycle state.
    pub state: GitIndexPublicationState,
    /// Free-form detail about the transition (never the staged content).
    pub detail: Option<String>,
    /// When the row last changed state.
    pub updated_at_ms: i64,
}

/// Failures of the publication journal.
#[derive(Debug, Error)]
pub enum GitIndexPublicationStoreError {
    /// The underlying database rejected the operation.
    #[error("git index publication journal storage error: {0}")]
    Storage(#[from] StorageError),
    /// A row with this identity already exists. Publication identities are
    /// minted per attempt and never reused, so a duplicate is a bug or a
    /// replay, and both are refused rather than merged.
    #[error("git index publication journal already holds this intent: {0}")]
    Duplicate(String),
    /// The row does not exist.
    #[error("git index publication journal has no such intent: {0}")]
    NotFound(String),
    /// The requested transition is not permitted from the row's current
    /// state.
    #[error("git index publication journal refuses transition {from} -> {to} for {id}: {reason}")]
    IllegalTransition {
        /// The intent identity.
        id: String,
        /// The state the row is in.
        from: GitIndexPublicationState,
        /// The requested state.
        to: GitIndexPublicationState,
        /// Why the transition was refused.
        reason: String,
    },
    /// A stored value could not be parsed back into its domain type.
    #[error("git index publication journal row is malformed: {0}")]
    Malformed(String),
}

impl From<sqlx::Error> for GitIndexPublicationStoreError {
    fn from(error: sqlx::Error) -> Self {
        GitIndexPublicationStoreError::Storage(StorageError::from(error))
    }
}

/// The durable publication journal.
///
/// Every mutation is a single statement or a guarded read-then-write inside
/// an **immediate** transaction, so two daemons racing to publish the same
/// repository cannot both believe they won.
#[derive(Debug, Clone)]
pub struct GitIndexPublicationStore {
    pool: SqlitePool,
}

impl GitIndexPublicationStore {
    /// Bind the store to a storage pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// The backing pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Record a new publication intent in the `prepared` state.
    ///
    /// This is the first durable act of a publication. It happens **before**
    /// the Git index lock is taken, so that a crash between the intent and
    /// the lock is recoverable: the reconciler finds a `prepared` row, sees
    /// that the real index still carries the expected fingerprint, and aborts
    /// the operation without touching the repository.
    pub async fn record_intent(
        &self,
        intent: &GitIndexPublicationIntent,
    ) -> Result<(), GitIndexPublicationStoreError> {
        let staged_paths = serde_json::to_string(&intent.staged_paths)
            .map_err(|error| GitIndexPublicationStoreError::Malformed(error.to_string()))?;
        let result = sqlx::query(
            r#"
            INSERT INTO git_index_publication (
                id, workspace_root, gitdir_path, gitdir_dev, gitdir_ino,
                private_index_path, expected_index_fingerprint,
                published_index_fingerprint, staged_paths, state, detail,
                created_at_ms, updated_at_ms
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'prepared', NULL, ?, ?)
            "#,
        )
        .bind(&intent.id)
        .bind(&intent.workspace_root)
        .bind(&intent.gitdir_path)
        .bind(intent.gitdir_dev.to_string())
        .bind(intent.gitdir_ino.to_string())
        .bind(&intent.private_index_path)
        .bind(intent.expected_index_fingerprint.as_str())
        .bind(intent.published_index_fingerprint.as_str())
        .bind(staged_paths)
        .bind(intent.created_at_ms)
        .bind(intent.created_at_ms)
        .execute(&self.pool)
        .await;
        match result {
            Ok(result) => {
                if result.rows_affected() == 0 {
                    return Err(GitIndexPublicationStoreError::Duplicate(intent.id.clone()));
                }
                Ok(())
            }
            // A unique-constraint violation is the duplicate case, not a
            // generic storage failure: publication identities are minted per
            // attempt and never reused, so a collision is a bug or a replay
            // and both are refused rather than merged.
            Err(sqlx::Error::Database(ref database_error))
                if database_error.is_unique_violation() =>
            {
                Err(GitIndexPublicationStoreError::Duplicate(intent.id.clone()))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Move an intent to a new state.
    ///
    /// The transition is validated against the row's **current** state inside
    /// an immediate transaction, so a concurrent reconciler and publisher
    /// cannot both advance the same intent. Returns `true` when the row
    /// moved, `false` when the intent does not exist.
    pub async fn mark(
        &self,
        id: &str,
        next: GitIndexPublicationState,
        detail: Option<&str>,
        now_ms: i64,
    ) -> Result<bool, GitIndexPublicationStoreError> {
        let mut conn = self.pool.acquire().await?;
        // `BEGIN IMMEDIATE` takes the write lock at `BEGIN` instead of
        // upgrading a shared read lock at the first `UPDATE`. A deferred
        // transaction would let two callers read the same current state and
        // both decide the transition is legal.
        sqlx::sqlite::SqliteTransactionManager::begin(
            &mut conn,
            Some(std::borrow::Cow::Borrowed("BEGIN IMMEDIATE")),
        )
        .await?;

        let current: Option<String> =
            sqlx::query_scalar("SELECT state FROM git_index_publication WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *conn)
                .await?;

        let Some(current) = current else {
            let _ = sqlx::sqlite::SqliteTransactionManager::rollback(&mut conn).await;
            return Ok(false);
        };
        let current = GitIndexPublicationState::parse(&current).ok_or_else(|| {
            GitIndexPublicationStoreError::Malformed(format!("unknown state {current}"))
        })?;
        if !current.permits_transition_to(next) {
            let _ = sqlx::sqlite::SqliteTransactionManager::rollback(&mut conn).await;
            return Err(GitIndexPublicationStoreError::IllegalTransition {
                id: id.to_owned(),
                from: current,
                to: next,
                reason: "the current state does not admit this transition".to_owned(),
            });
        }

        sqlx::query(
            "UPDATE git_index_publication SET state = ?, detail = ?, updated_at_ms = ? \
             WHERE id = ?",
        )
        .bind(next.as_str())
        .bind(detail)
        .bind(now_ms)
        .bind(id)
        .execute(&mut *conn)
        .await?;
        sqlx::sqlite::SqliteTransactionManager::commit(&mut conn).await?;
        Ok(true)
    }

    /// Read one intent, whatever its state.
    pub async fn get(
        &self,
        id: &str,
    ) -> Result<Option<GitIndexPublicationRecord>, GitIndexPublicationStoreError> {
        let row = sqlx::query(
            r#"
            SELECT id, workspace_root, gitdir_path, gitdir_dev, gitdir_ino,
                   private_index_path, expected_index_fingerprint,
                   published_index_fingerprint, staged_paths, state, detail,
                   created_at_ms, updated_at_ms
            FROM git_index_publication
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(record_from_row).transpose()
    }

    /// List the intents that are not yet resolved, oldest first.
    ///
    /// This is the reconciler's work list. It is bounded by `limit` so a
    /// startup pass can never become an unbounded sweep.
    pub async fn list_unresolved(
        &self,
        limit: i64,
    ) -> Result<Vec<GitIndexPublicationRecord>, GitIndexPublicationStoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, workspace_root, gitdir_path, gitdir_dev, gitdir_ino,
                   private_index_path, expected_index_fingerprint,
                   published_index_fingerprint, staged_paths, state, detail,
                   created_at_ms, updated_at_ms
            FROM git_index_publication
            WHERE state IN ('prepared', 'publishing')
            ORDER BY created_at_ms ASC, id ASC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(record_from_row).collect()
    }
}

fn record_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<GitIndexPublicationRecord, GitIndexPublicationStoreError> {
    let state_raw: String = row.try_get("state")?;
    let state = GitIndexPublicationState::parse(&state_raw).ok_or_else(|| {
        GitIndexPublicationStoreError::Malformed(format!("unknown state {state_raw}"))
    })?;
    let expected_raw: String = row.try_get("expected_index_fingerprint")?;
    let expected = IndexFingerprint::parse(&expected_raw).ok_or_else(|| {
        GitIndexPublicationStoreError::Malformed(format!(
            "unknown expected index fingerprint {expected_raw}"
        ))
    })?;
    let published_raw: String = row.try_get("published_index_fingerprint")?;
    let published = IndexFingerprint::parse(&published_raw).ok_or_else(|| {
        GitIndexPublicationStoreError::Malformed(format!(
            "unknown published index fingerprint {published_raw}"
        ))
    })?;
    let staged_raw: String = row.try_get("staged_paths")?;
    let staged_paths = serde_json::from_str::<Vec<String>>(&staged_raw).map_err(|error| {
        GitIndexPublicationStoreError::Malformed(format!("staged paths: {error}"))
    })?;
    let gitdir_dev: String = row.try_get("gitdir_dev")?;
    let gitdir_ino: String = row.try_get("gitdir_ino")?;
    Ok(GitIndexPublicationRecord {
        intent: GitIndexPublicationIntent {
            id: row.try_get("id")?,
            workspace_root: row.try_get("workspace_root")?,
            gitdir_path: row.try_get("gitdir_path")?,
            gitdir_dev: gitdir_dev.parse().map_err(|_| {
                GitIndexPublicationStoreError::Malformed(format!(
                    "unparseable gitdir device identity {gitdir_dev}"
                ))
            })?,
            gitdir_ino: gitdir_ino.parse().map_err(|_| {
                GitIndexPublicationStoreError::Malformed(format!(
                    "unparseable gitdir inode identity {gitdir_ino}"
                ))
            })?,
            private_index_path: row.try_get("private_index_path")?,
            expected_index_fingerprint: expected,
            published_index_fingerprint: published,
            staged_paths,
            created_at_ms: row.try_get("created_at_ms")?,
        },
        state,
        detail: row.try_get("detail")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_spelling_round_trips() {
        for state in GitIndexPublicationState::ALL {
            assert_eq!(GitIndexPublicationState::parse(state.as_str()), Some(state));
        }
        assert_eq!(GitIndexPublicationState::parse("published"), None);
    }

    #[test]
    fn terminal_states_do_not_transition() {
        for state in [
            GitIndexPublicationState::Committed,
            GitIndexPublicationState::Aborted,
            GitIndexPublicationState::Ambiguous,
        ] {
            assert!(state.is_terminal());
            for next in GitIndexPublicationState::ALL {
                assert!(!state.permits_transition_to(next));
            }
        }
        assert!(!GitIndexPublicationState::Prepared.is_terminal());
        assert!(!GitIndexPublicationState::Publishing.is_terminal());
    }

    #[test]
    fn publishing_never_returns_to_prepared() {
        assert!(!GitIndexPublicationState::Publishing
            .permits_transition_to(GitIndexPublicationState::Prepared));
        assert!(GitIndexPublicationState::Publishing
            .permits_transition_to(GitIndexPublicationState::Committed));
        assert!(GitIndexPublicationState::Prepared
            .permits_transition_to(GitIndexPublicationState::Aborted));
    }

    #[test]
    fn fingerprint_absent_is_not_a_digest() {
        assert_eq!(
            IndexFingerprint::parse("absent"),
            Some(IndexFingerprint::Absent)
        );
        assert_eq!(IndexFingerprint::Absent.as_str(), "absent");
        let digest = "a".repeat(64);
        assert_eq!(
            IndexFingerprint::parse(&digest),
            Some(IndexFingerprint::Digest(digest.clone()))
        );
        assert_eq!(IndexFingerprint::parse("not-a-digest"), None);
        // A 64-character digest is the only other accepted shape, so the
        // absent sentinel cannot be confused with a real fingerprint.
        assert!(IndexFingerprint::parse("absent").is_some());
        assert!(matches!(
            IndexFingerprint::parse("absent"),
            Some(IndexFingerprint::Absent)
        ));
    }
}
