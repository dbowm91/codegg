//! Team collaboration corrective M004: turn-scoped shared-session controller lease (ADR-0007).
//!
//! An idle session has no controller. Successful `TurnSubmit` atomically
//! establishes a lease attributed to the submitting principal keyed by
//! canonical `(session_id, turn_id)`. While the turn is active, the
//! controller principal is required in addition to existing capabilities
//! for steer/cancel/permission/question responses. Transfer is explicit
//! (current controller plus eligible recipient, CAS revision); forced
//! takeover requires Maintainer/Owner-equivalent policy plus a bounded
//! reason and is audited. Control requests are inert notification/state
//! only and never mutate the lease.
//!
//! This module owns the durable state machine and converts to the
//! `session_control.v1` wire DTOs in [`codegg_protocol::core`]. It
//! performs no transport authentication itself: the daemon supplies typed
//! principal ids plus the caller's project role, and maps storage
//! failures to fail-closed denials. Projection carries only principal
//! ids plus coarse revision/action metadata, never credentials or device
//! secrets.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;

use crate::error::StorageError;
use crate::identity::PrincipalId;

/// Version of the session-control protocol surface.
pub const SESSION_CONTROL_PROTOCOL_VERSION: u32 = 1;
/// Capability string advertised for the session-control surface.
pub const SESSION_CONTROL_CAPABILITY: &str = "session_control.v1";
/// Maximum UTF-8 bytes accepted for a takeover/transfer reason or request message.
pub const SESSION_CONTROL_MAX_REASON_LEN: usize = 280;
/// Maximum inert requests retained per session.
pub const SESSION_CONTROL_MAX_REQUESTS_PER_SESSION: usize = 20;
/// Maximum rows returned by a control-request listing.
pub const SESSION_CONTROL_MAX_LIST_LIMIT: usize = 20;

/// Last lease action, stored as an opaque string and degraded safely by readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionControlAction {
    Acquired,
    Transferred,
    Takeover,
    Released,
}

impl SessionControlAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Acquired => "acquired",
            Self::Transferred => "transferred",
            Self::Takeover => "takeover",
            Self::Released => "released",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "acquired" => Some(Self::Acquired),
            "transferred" => Some(Self::Transferred),
            "takeover" => Some(Self::Takeover),
            "released" => Some(Self::Released),
            _ => None,
        }
    }
}

/// Durable controller lease: at most one active row per session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionControllerRecord {
    pub session_id: String,
    pub turn_id: String,
    pub controller_principal: PrincipalId,
    pub origin_client: Option<String>,
    pub revision: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_action: String,
    pub last_actor: Option<String>,
    pub last_reason: Option<String>,
}

impl SessionControllerRecord {
    pub fn to_dto(&self) -> codegg_protocol::core::SessionControllerDto {
        codegg_protocol::core::SessionControllerDto {
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
            controller_principal: self.controller_principal.as_str().to_owned(),
            origin_client: self.origin_client.clone(),
            revision: self.revision,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            last_action: self.last_action.clone(),
            last_actor: self.last_actor.clone(),
            last_reason: self.last_reason.clone(),
        }
    }
}

/// One inert control request: notification/state only, never a lease mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionControlRequestRecord {
    pub request_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub requester_principal: PrincipalId,
    pub message: Option<String>,
    pub created_at_ms: i64,
}

impl SessionControlRequestRecord {
    pub fn to_dto(&self) -> codegg_protocol::core::SessionControlRequestDto {
        codegg_protocol::core::SessionControlRequestDto {
            request_id: self.request_id.clone(),
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
            requester_principal: self.requester_principal.as_str().to_owned(),
            message: self.message.clone(),
            created_at_ms: self.created_at_ms,
        }
    }
}

/// Typed failure for the session-control domain.
#[derive(Debug, Error)]
pub enum SessionControlError {
    #[error("session control conflict: {0}")]
    Conflict(String),
    #[error("session control not found: {0}")]
    NotFound(String),
    #[error("stale session control revision: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error("not the session controller")]
    NotController,
    #[error("invalid session control {field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
    #[error("session control store unavailable: {0}")]
    Unavailable(String),
    #[error("session control storage error: {0}")]
    Storage(#[from] StorageError),
}

impl SessionControlError {
    /// Stable wire code for `CoreResponse::Error`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Conflict(_) => "session_control_conflict",
            Self::NotFound(_) => "session_control_not_found",
            Self::RevisionConflict { .. } => "session_control_conflict",
            Self::NotController => "session_control_not_controller",
            Self::Invalid { .. } => "session_control_invalid_input",
            Self::Unavailable(_) => "session_control_unavailable",
            Self::Storage(_) => "session_control_storage_error",
        }
    }

    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            message: message.into(),
        }
    }
}

impl From<sqlx::Error> for SessionControlError {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(StorageError::Database(error.to_string()))
    }
}

/// Canonical `CREATE TABLE` statements for the M004 controller domain.
///
/// Executed by the session-schema migration (v65); this helper keeps
/// tests and pool-less guards on the identical shape. All additive
/// `IF NOT EXISTS`, safe on existing databases: pre-lease databases
/// gain empty tables and readers treat absence as "no controller".
pub const SESSION_CONTROL_SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS session_turn_controller (
        session_id TEXT PRIMARY KEY,
        turn_id TEXT NOT NULL,
        controller_principal TEXT NOT NULL,
        origin_client TEXT,
        revision INTEGER NOT NULL DEFAULT 1,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        last_action TEXT NOT NULL DEFAULT 'acquired',
        last_actor TEXT,
        last_reason TEXT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS session_control_request (
        request_id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        turn_id TEXT,
        requester_principal TEXT NOT NULL,
        message TEXT,
        created_at INTEGER NOT NULL
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_session_control_request_session ON session_control_request(session_id, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_session_turn_controller_turn ON session_turn_controller(turn_id)",
];

/// Ensure the session-control tables exist. Idempotent.
pub async fn ensure_session_control_tables(pool: &SqlitePool) -> Result<(), SessionControlError> {
    for statement in SESSION_CONTROL_SCHEMA_STATEMENTS {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| SessionControlError::Storage(StorageError::Migration(e.to_string())))?;
    }
    Ok(())
}

fn validate_session_id(value: &str) -> Result<(), SessionControlError> {
    if value.is_empty() || value.len() > 128 {
        return Err(SessionControlError::invalid(
            "session_id",
            "session locator must be 1..=128 bytes",
        ));
    }
    if value.bytes().any(|b| b == 0) || value.chars().any(char::is_control) {
        return Err(SessionControlError::invalid(
            "session_id",
            "session locator contains an unsupported character",
        ));
    }
    Ok(())
}

fn validate_turn_id(value: &str) -> Result<(), SessionControlError> {
    if value.is_empty() || value.len() > 128 {
        return Err(SessionControlError::invalid(
            "turn_id",
            "turn locator must be 1..=128 bytes",
        ));
    }
    if value.bytes().any(|b| b == 0) || value.chars().any(char::is_control) {
        return Err(SessionControlError::invalid(
            "turn_id",
            "turn locator contains an unsupported character",
        ));
    }
    Ok(())
}

fn validate_client_id(value: &str) -> Result<(), SessionControlError> {
    if value.is_empty() || value.len() > 128 {
        return Err(SessionControlError::invalid(
            "client_id",
            "client locator must be 1..=128 bytes",
        ));
    }
    if value.bytes().any(|b| b == 0) || value.chars().any(char::is_control) {
        return Err(SessionControlError::invalid(
            "client_id",
            "client locator contains an unsupported character",
        ));
    }
    Ok(())
}

/// Validate a bounded reason/message: non-empty trimmed text without
/// control characters, at most [`SESSION_CONTROL_MAX_REASON_LEN`] bytes.
pub fn validate_reason(value: &str) -> Result<String, SessionControlError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(SessionControlError::invalid(
            "reason",
            "reason must not be empty",
        ));
    }
    if trimmed.len() > SESSION_CONTROL_MAX_REASON_LEN {
        return Err(SessionControlError::invalid(
            "reason",
            "reason is too large (max 280 bytes)",
        ));
    }
    if trimmed.bytes().any(|b| b == 0) || trimmed.chars().any(char::is_control) {
        return Err(SessionControlError::invalid(
            "reason",
            "reason contains an unsupported character",
        ));
    }
    Ok(trimmed.to_owned())
}

/// Validate an optional request message: empty/`None` stays `None`.
pub fn validate_optional_message(
    value: Option<&str>,
) -> Result<Option<String>, SessionControlError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > SESSION_CONTROL_MAX_REASON_LEN {
        return Err(SessionControlError::invalid(
            "message",
            "request message is too large (max 280 bytes)",
        ));
    }
    if trimmed.bytes().any(|b| b == 0) || trimmed.chars().any(char::is_control) {
        return Err(SessionControlError::invalid(
            "message",
            "request message contains an unsupported character",
        ));
    }
    Ok(Some(trimmed.to_owned()))
}

/// One `session_turn_controller` row as decoded from storage.
type ControllerRow = (
    String,
    String,
    String,
    Option<String>,
    i64,
    i64,
    i64,
    String,
    Option<String>,
    Option<String>,
);

/// One `session_control_request` row as decoded from storage.
type ControlRequestRow = (String, String, Option<String>, String, Option<String>, i64);

fn row_to_record(row: ControllerRow) -> Result<SessionControllerRecord, SessionControlError> {
    let (
        session_id,
        turn_id,
        controller_principal,
        origin_client,
        revision,
        created_at,
        updated_at,
        last_action,
        last_actor,
        last_reason,
    ) = row;
    let controller_principal = PrincipalId::parse(&controller_principal)
        .map_err(|e| SessionControlError::invalid("controller_principal", e.to_string()))?;
    Ok(SessionControllerRecord {
        session_id,
        turn_id,
        controller_principal,
        origin_client,
        revision: u64::try_from(revision).unwrap_or(0),
        created_at_ms: created_at,
        updated_at_ms: updated_at,
        last_action,
        last_actor,
        last_reason,
    })
}

/// Daemon-owned session-control store.
#[derive(Debug, Clone)]
pub struct SessionControllerStore {
    pool: SqlitePool,
}

impl SessionControllerStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Fetch the active lease for one session, if any.
    pub async fn get(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionControllerRecord>, SessionControlError> {
        validate_session_id(session_id)?;
        let row: Option<ControllerRow> = sqlx::query_as(
            "SELECT session_id, turn_id, controller_principal, origin_client, revision, \
              created_at, updated_at, last_action, last_actor, last_reason \
              FROM session_turn_controller WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_record).transpose()
    }

    /// Atomically acquire the lease for `(session_id, turn_id)`.
    ///
    /// At most one active row exists per session. A retry carrying the
    /// same turn and principal converges on the stored row (idempotent);
    /// any other contender observes [`SessionControlError::Conflict`]
    /// and changes nothing, so a failed turn submission never leaves a
    /// lease behind (callers roll back the in-memory turn on error).
    pub async fn acquire(
        &self,
        session_id: &str,
        turn_id: &str,
        controller: &PrincipalId,
        origin_client: Option<&str>,
        now_ms: i64,
    ) -> Result<SessionControllerRecord, SessionControlError> {
        validate_session_id(session_id)?;
        validate_turn_id(turn_id)?;
        if let Some(client) = origin_client {
            validate_client_id(client)?;
        }
        sqlx::query(
            "INSERT INTO session_turn_controller \
              (session_id, turn_id, controller_principal, origin_client, revision, \
               created_at, updated_at, last_action, last_actor, last_reason) \
              VALUES (?, ?, ?, ?, 1, ?, ?, 'acquired', ?, NULL) \
              ON CONFLICT(session_id) DO NOTHING",
        )
        .bind(session_id)
        .bind(turn_id)
        .bind(controller.as_str())
        .bind(origin_client)
        .bind(now_ms)
        .bind(now_ms)
        .bind(controller.as_str())
        .execute(&self.pool)
        .await?;
        let stored = self.get(session_id).await?.ok_or_else(|| {
            SessionControlError::Storage(StorageError::Database(
                "session control acquire lost".to_owned(),
            ))
        })?;
        if stored.turn_id == turn_id && stored.controller_principal == *controller {
            return Ok(stored);
        }
        Err(SessionControlError::Conflict(format!(
            "session {session_id} already has an active controller turn"
        )))
    }

    /// Release the lease when the turn reaches a terminal state.
    ///
    /// Deletes only when `turn_id` matches the stored row, so a stale
    /// transfer racing completion changes nothing: the terminal
    /// transition wins and later writers observe `NotFound`.
    pub async fn release(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<bool, SessionControlError> {
        validate_session_id(session_id)?;
        validate_turn_id(turn_id)?;
        let result =
            sqlx::query("DELETE FROM session_turn_controller WHERE session_id = ? AND turn_id = ?")
                .bind(session_id)
                .bind(turn_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Explicit transfer from the current controller to an eligible
    /// recipient under CAS revision protection.
    ///
    /// The caller must have verified `caller == stored.controller` and
    /// recipient eligibility before calling; the store enforces the
    /// revision/turn/controller triple atomically so simultaneous
    /// transfer/takeover attempts converge on exactly one winner.
    #[allow(clippy::too_many_arguments)]
    pub async fn transfer(
        &self,
        session_id: &str,
        turn_id: &str,
        expected_revision: u64,
        from: &PrincipalId,
        to: &PrincipalId,
        actor: &PrincipalId,
        reason: Option<&str>,
        now_ms: i64,
    ) -> Result<SessionControllerRecord, SessionControlError> {
        validate_session_id(session_id)?;
        validate_turn_id(turn_id)?;
        let reason = validate_optional_message(reason)?;
        let expected = i64::try_from(expected_revision).unwrap_or(i64::MAX);
        let result = sqlx::query(
            "UPDATE session_turn_controller SET turn_id = ?, controller_principal = ?, \
              revision = revision + 1, updated_at = ?, last_action = 'transferred', \
              last_actor = ?, last_reason = ? \
              WHERE session_id = ? AND turn_id = ? AND revision = ? AND controller_principal = ?",
        )
        .bind(turn_id)
        .bind(to.as_str())
        .bind(now_ms)
        .bind(actor.as_str())
        .bind(reason.clone())
        .bind(session_id)
        .bind(turn_id)
        .bind(expected)
        .bind(from.as_str())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() > 0 {
            let stored = self.get(session_id).await?.ok_or_else(|| {
                SessionControlError::Storage(StorageError::Database(
                    "session control transfer lost".to_owned(),
                ))
            })?;
            return Ok(stored);
        }
        // Diagnose the stale write without leaking beyond the caller's
        // own session scope (the daemon maps these to typed codes).
        let Some(current) = self.get(session_id).await? else {
            return Err(SessionControlError::NotFound(format!(
                "no active controller for session {session_id}"
            )));
        };
        if current.turn_id != turn_id {
            return Err(SessionControlError::Conflict(format!(
                "turn {turn_id} is no longer active for session {session_id}"
            )));
        }
        if current.controller_principal != *from {
            return Err(SessionControlError::NotController);
        }
        Err(SessionControlError::RevisionConflict {
            expected: expected_revision,
            current: current.revision,
        })
    }

    /// Explicit forced takeover (Maintainer/Owner recovery).
    ///
    /// CAS on `(session_id, turn_id, revision)`; the previous
    /// controller is recorded as the actor's provenance, never as new
    /// authority beyond what the recipient already holds (enforced by
    /// the daemon, not the store).
    pub async fn takeover(
        &self,
        session_id: &str,
        turn_id: &str,
        expected_revision: u64,
        new_controller: &PrincipalId,
        actor: &PrincipalId,
        reason: &str,
        now_ms: i64,
    ) -> Result<SessionControllerRecord, SessionControlError> {
        validate_session_id(session_id)?;
        validate_turn_id(turn_id)?;
        let reason = validate_reason(reason)?;
        let expected = i64::try_from(expected_revision).unwrap_or(i64::MAX);
        let result = sqlx::query(
            "UPDATE session_turn_controller SET turn_id = ?, controller_principal = ?, \
              revision = revision + 1, updated_at = ?, last_action = 'takeover', \
              last_actor = ?, last_reason = ? \
              WHERE session_id = ? AND turn_id = ? AND revision = ?",
        )
        .bind(turn_id)
        .bind(new_controller.as_str())
        .bind(now_ms)
        .bind(actor.as_str())
        .bind(reason)
        .bind(session_id)
        .bind(turn_id)
        .bind(expected)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() > 0 {
            let stored = self.get(session_id).await?.ok_or_else(|| {
                SessionControlError::Storage(StorageError::Database(
                    "session control takeover lost".to_owned(),
                ))
            })?;
            return Ok(stored);
        }
        let Some(current) = self.get(session_id).await? else {
            return Err(SessionControlError::NotFound(format!(
                "no active controller for session {session_id}"
            )));
        };
        if current.turn_id != turn_id {
            return Err(SessionControlError::Conflict(format!(
                "turn {turn_id} is no longer active for session {session_id}"
            )));
        }
        Err(SessionControlError::RevisionConflict {
            expected: expected_revision,
            current: current.revision,
        })
    }

    /// Record one inert control request. Bounded: keeps the newest
    /// [`SESSION_CONTROL_MAX_REQUESTS_PER_SESSION`] rows per session.
    pub async fn request_control(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        requester: &PrincipalId,
        message: Option<&str>,
        now_ms: i64,
    ) -> Result<SessionControlRequestRecord, SessionControlError> {
        validate_session_id(session_id)?;
        if let Some(turn) = turn_id {
            validate_turn_id(turn)?;
        }
        let message = validate_optional_message(message)?;
        let request_id = format!("ctlreq-{}", uuid::Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO session_control_request \
              (request_id, session_id, turn_id, requester_principal, message, created_at) \
              VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&request_id)
        .bind(session_id)
        .bind(turn_id)
        .bind(requester.as_str())
        .bind(message.clone())
        .bind(now_ms)
        .execute(&self.pool)
        .await?;
        // Best-effort bound: prune oldest rows beyond the per-session cap.
        let cap = SESSION_CONTROL_MAX_REQUESTS_PER_SESSION as i64;
        let _ = sqlx::query(
            "DELETE FROM session_control_request WHERE session_id = ? AND request_id NOT IN ( \
              SELECT request_id FROM session_control_request \
              WHERE session_id = ? ORDER BY created_at DESC, request_id DESC LIMIT ?)",
        )
        .bind(session_id)
        .bind(session_id)
        .bind(cap)
        .execute(&self.pool)
        .await;
        Ok(SessionControlRequestRecord {
            request_id,
            session_id: session_id.to_owned(),
            turn_id: turn_id.map(str::to_owned),
            requester_principal: requester.clone(),
            message,
            created_at_ms: now_ms,
        })
    }

    /// Bounded listing of inert requests, newest first.
    pub async fn list_requests(
        &self,
        session_id: &str,
        limit: Option<usize>,
    ) -> Result<(Vec<SessionControlRequestRecord>, bool), SessionControlError> {
        validate_session_id(session_id)?;
        let bound = limit
            .unwrap_or(SESSION_CONTROL_MAX_LIST_LIMIT)
            .clamp(1, SESSION_CONTROL_MAX_LIST_LIMIT.max(1));
        let rows: Vec<ControlRequestRow> = sqlx::query_as(
            "SELECT request_id, session_id, turn_id, requester_principal, message, created_at \
                  FROM session_control_request WHERE session_id = ? \
                  ORDER BY created_at DESC, request_id DESC LIMIT ?",
        )
        .bind(session_id)
        .bind((bound as i64) + 1)
        .fetch_all(&self.pool)
        .await?;
        let truncated = rows.len() > bound;
        let mut out = Vec::with_capacity(rows.len().min(bound));
        for (request_id, session_id, turn_id, requester_raw, message, created_at) in
            rows.into_iter().take(bound)
        {
            let Ok(requester) = PrincipalId::parse(&requester_raw) else {
                continue;
            };
            out.push(SessionControlRequestRecord {
                request_id,
                session_id,
                turn_id,
                requester_principal: requester,
                message,
                created_at_ms: created_at,
            });
        }
        Ok((out, truncated))
    }

    /// List every active lease (restart reconciliation only, bounded).
    pub async fn list_active(
        &self,
        limit: usize,
    ) -> Result<Vec<SessionControllerRecord>, SessionControlError> {
        let bound = limit.clamp(1, 1000) as i64;
        let rows: Vec<ControllerRow> = sqlx::query_as(
            "SELECT session_id, turn_id, controller_principal, origin_client, revision, \
              created_at, updated_at, last_action, last_actor, last_reason \
              FROM session_turn_controller ORDER BY updated_at DESC LIMIT ?",
        )
        .bind(bound)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            if let Ok(record) = row_to_record(row) {
                out.push(record);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::PrincipalId;

    async fn test_pool() -> SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        let url = format!(
            "file:session_control_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid options")
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect");
        crate::session::schema::migrate(&pool)
            .await
            .expect("migrate");
        pool
    }

    fn principal(id: &str) -> PrincipalId {
        PrincipalId::parse(id).expect("valid principal")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn acquire_is_idempotent_for_same_turn_principal() {
        let pool = test_pool().await;
        let store = SessionControllerStore::new(pool);
        let alice = principal("alice");
        let first = store
            .acquire("s1", "t1", &alice, Some("c1"), 1)
            .await
            .expect("acquire");
        assert_eq!(first.revision, 1);
        let second = store
            .acquire("s1", "t1", &alice, Some("c1"), 2)
            .await
            .expect("retry converges");
        assert_eq!(second.revision, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn second_turn_conflicts_without_overwrite() {
        let pool = test_pool().await;
        let store = SessionControllerStore::new(pool);
        let alice = principal("alice");
        let bob = principal("bob");
        store
            .acquire("s1", "t1", &alice, None, 1)
            .await
            .expect("acquire");
        let err = store
            .acquire("s1", "t2", &bob, None, 2)
            .await
            .expect_err("second turn conflicts");
        assert!(matches!(err, SessionControlError::Conflict(_)));
        let stored = store.get("s1").await.expect("get").expect("row");
        assert_eq!(stored.turn_id, "t1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn transfer_and_takeover_are_revision_safe() {
        let pool = test_pool().await;
        let store = SessionControllerStore::new(pool);
        let alice = principal("alice");
        let bob = principal("bob");
        let carol = principal("carol");
        let acquired = store
            .acquire("s1", "t1", &alice, None, 1)
            .await
            .expect("acquire");
        let moved = store
            .transfer("s1", "t1", acquired.revision, &alice, &bob, &alice, None, 2)
            .await
            .expect("transfer");
        assert_eq!(moved.controller_principal, bob);
        assert_eq!(moved.revision, 2);
        let stale = store
            .transfer("s1", "t1", acquired.revision, &bob, &carol, &bob, None, 3)
            .await
            .expect_err("stale revision conflicts");
        assert!(matches!(
            stale,
            SessionControlError::RevisionConflict { .. }
        ));
        let taken = store
            .takeover("s1", "t1", moved.revision, &carol, &carol, "recovery", 4)
            .await
            .expect("takeover");
        assert_eq!(taken.controller_principal, carol);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn release_wins_over_stale_transfer() {
        let pool = test_pool().await;
        let store = SessionControllerStore::new(pool);
        let alice = principal("alice");
        let bob = principal("bob");
        let acquired = store
            .acquire("s1", "t1", &alice, None, 1)
            .await
            .expect("acquire");
        assert!(store.release("s1", "t1").await.expect("release"));
        let err = store
            .transfer("s1", "t1", acquired.revision, &alice, &bob, &alice, None, 2)
            .await
            .expect_err("transfer after terminal fails");
        assert!(matches!(err, SessionControlError::NotFound(_)));
    }
}
