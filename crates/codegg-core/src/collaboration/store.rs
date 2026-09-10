//! Concrete collaboration schema, row decoding, and locator persistence helpers.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::{
    ChatChannel, ChatMessage, ChatMessageId, ChatReference, ChatReferenceKind, CollaborationError,
};
use crate::error::StorageError;
use crate::identity::{ChannelId, PrincipalId, ProjectId};

// ── Durable tables ───────────────────────────────────────────────────

/// Canonical `CREATE TABLE` statements for the chat domain.
///
/// The session-schema migration (v55) executes these same statements;
/// this helper keeps tests and pool-less guards on the identical shape.
pub const CHAT_SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS chat_channel (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL,
        name TEXT NOT NULL,
        created_by TEXT NOT NULL,
        created_at INTEGER NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS chat_message (
        id TEXT PRIMARY KEY,
        channel_id TEXT NOT NULL,
        project_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        author_principal TEXT NOT NULL,
        author_agent TEXT,
        body TEXT NOT NULL,
        reply_to TEXT,
        thread_root TEXT,
        mentions_json TEXT NOT NULL DEFAULT '[]',
        references_json TEXT NOT NULL DEFAULT '[]',
        revision INTEGER NOT NULL DEFAULT 1,
        redacted INTEGER NOT NULL DEFAULT 0,
        idempotency_key TEXT,
        created_at INTEGER NOT NULL,
        edited_at INTEGER,
        UNIQUE(channel_id, seq)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS chat_revision (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        message_id TEXT NOT NULL,
        revision INTEGER NOT NULL,
        body TEXT NOT NULL,
        edited_by TEXT NOT NULL,
        edited_at INTEGER NOT NULL,
        reason TEXT,
        UNIQUE(message_id, revision)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS chat_read_marker (
        channel_id TEXT NOT NULL,
        principal_id TEXT NOT NULL,
        last_read_seq INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(channel_id, principal_id)
    )
    "#,
    "CREATE UNIQUE INDEX IF NOT EXISTS idx_chat_message_idempotency ON chat_message(channel_id, idempotency_key) WHERE idempotency_key IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS idx_chat_channel_project ON chat_channel(project_id, created_at)",
    "CREATE INDEX IF NOT EXISTS idx_chat_message_channel_seq ON chat_message(channel_id, seq)",
    "CREATE INDEX IF NOT EXISTS idx_chat_message_project ON chat_message(project_id)",
    "CREATE INDEX IF NOT EXISTS idx_chat_revision_message ON chat_revision(message_id, revision)",
    "CREATE INDEX IF NOT EXISTS idx_chat_read_channel ON chat_read_marker(channel_id)",
];

/// Canonical `CREATE TABLE` statements for structured chat actions
/// (M003).
///
/// The session-schema migration (v56) executes these same statements;
/// this helper keeps tests and pool-less guards on the identical shape.
/// Chat stores only the reference/status projection; the job is
/// canonical durable state elsewhere. `(channel_id, idempotency_key)`
/// is unique so retries converge without a second job.
pub const CHAT_ACTION_SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS chat_action (
        action_id TEXT PRIMARY KEY,
        channel_id TEXT NOT NULL,
        message_id TEXT NOT NULL,
        project_id TEXT NOT NULL,
        actor TEXT NOT NULL,
        kind TEXT NOT NULL,
        title TEXT,
        job_id TEXT,
        status TEXT NOT NULL,
        idempotency_key TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        UNIQUE(channel_id, idempotency_key)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_chat_action_channel ON chat_action(channel_id, created_at)",
    "CREATE INDEX IF NOT EXISTS idx_chat_action_message ON chat_action(message_id, created_at)",
    "CREATE INDEX IF NOT EXISTS idx_chat_action_project ON chat_action(project_id)",
];

/// Ensure the chat tables exist. Idempotent.
pub async fn ensure_collaboration_tables(pool: &SqlitePool) -> Result<(), CollaborationError> {
    for statement in CHAT_SCHEMA_STATEMENTS
        .iter()
        .chain(CHAT_ACTION_SCHEMA_STATEMENTS.iter())
    {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|e| CollaborationError::Storage(StorageError::Migration(e.to_string())))?;
    }
    Ok(())
}

/// Owning project of one channel, if the channel exists.
///
/// Used by the daemon authorization resolver so channel-scoped requests
/// map to a `DirectProject` scope. Unknown or malformed ids return `None`
/// so team principals fail closed.
pub async fn channel_project(pool: &SqlitePool, channel_id: &str) -> Option<ProjectId> {
    let channel = ChannelId::parse(channel_id).ok()?;
    let row: Option<(String,)> = sqlx::query_as("SELECT project_id FROM chat_channel WHERE id = ?")
        .bind(channel.as_str())
        .fetch_optional(pool)
        .await
        .ok()?;
    let (project_raw,) = row?;
    ProjectId::parse(&project_raw).ok()
}

// ── Row mapping ──────────────────────────────────────────────────────

/// One decoded `chat_message` row: `(id, channel_id, project_id, seq,
/// author_principal, author_agent, body, reply_to, thread_root,
/// mentions_json, references_json, revision, redacted, idempotency_key,
/// created_at, edited_at)`.
pub(super) type MessageRow = (
    String,
    String,
    String,
    i64,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    i64,
    i64,
    Option<String>,
    i64,
    Option<i64>,
);

/// One decoded `chat_action` row: `(action_id, channel_id, message_id,
/// project_id, actor, kind, title, job_id, status, idempotency_key,
/// created_at, updated_at)`.
pub(super) type ActionRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    i64,
    i64,
);

#[allow(clippy::too_many_arguments)]
pub(super) fn row_to_channel(
    id: String,
    project_id: String,
    name: String,
    created_by: String,
    created_at: i64,
) -> Result<ChatChannel, CollaborationError> {
    Ok(ChatChannel {
        id: ChannelId::parse(&id)
            .map_err(|e| CollaborationError::invalid("channel_id", e.to_string()))?,
        project_id: ProjectId::parse(&project_id)
            .map_err(|e| CollaborationError::invalid("project_id", e.to_string()))?,
        name,
        created_by: PrincipalId::parse(&created_by)
            .map_err(|e| CollaborationError::invalid("created_by", e.to_string()))?,
        created_at_ms: created_at,
    })
}

pub(super) fn row_to_message(row: MessageRow) -> Result<ChatMessage, CollaborationError> {
    let (
        id,
        channel_id,
        project_id,
        seq,
        author_principal,
        author_agent,
        body,
        reply_to,
        thread_root,
        mentions_json,
        references_json,
        revision,
        redacted,
        idempotency_key,
        created_at,
        edited_at,
    ) = row;
    let bad =
        |field: &'static str| CollaborationError::invalid(field, "stored row failed to parse");
    let mentions_raw: Vec<String> =
        serde_json::from_str(&mentions_json).map_err(|_| bad("mentions"))?;
    let mut mentions = Vec::with_capacity(mentions_raw.len());
    for raw in mentions_raw {
        mentions.push(PrincipalId::parse(&raw).map_err(|_| bad("mentions"))?);
    }
    #[derive(Deserialize)]
    struct StoredReference {
        kind: String,
        target_id: String,
        #[serde(default)]
        display_hint: Option<String>,
    }
    let references_raw: Vec<StoredReference> =
        serde_json::from_str(&references_json).map_err(|_| bad("references"))?;
    let mut references = Vec::with_capacity(references_raw.len());
    for raw in references_raw {
        let kind = ChatReferenceKind::parse(&raw.kind).ok_or(bad("references"))?;
        references.push(ChatReference {
            kind,
            target_id: raw.target_id,
            display_hint: raw.display_hint,
        });
    }
    Ok(ChatMessage {
        id: ChatMessageId::parse(&id).map_err(|_| bad("message_id"))?,
        channel_id: ChannelId::parse(&channel_id).map_err(|_| bad("channel_id"))?,
        project_id: ProjectId::parse(&project_id).map_err(|_| bad("project_id"))?,
        seq: u64::try_from(seq).map_err(|_| bad("seq"))?,
        author_principal: PrincipalId::parse(&author_principal)
            .map_err(|_| bad("author_principal"))?,
        author_agent,
        body,
        reply_to: reply_to
            .map(|raw| ChatMessageId::parse(&raw))
            .transpose()
            .map_err(|_| bad("reply_to"))?,
        thread_root: thread_root
            .map(|raw| ChatMessageId::parse(&raw))
            .transpose()
            .map_err(|_| bad("thread_root"))?,
        mentions,
        references,
        revision: u64::try_from(revision).map_err(|_| bad("revision"))?,
        redacted: redacted != 0,
        idempotency_key,
        created_at_ms: created_at,
        edited_at_ms: edited_at,
    })
}

#[derive(Serialize)]
struct StoredReference<'a> {
    kind: &'a str,
    target_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_hint: &'a Option<String>,
}

pub(super) fn encode_references(references: &[ChatReference]) -> String {
    let stored: Vec<StoredReference<'_>> = references
        .iter()
        .map(|r| StoredReference {
            kind: r.kind.as_str(),
            target_id: r.target_id.as_str(),
            display_hint: &r.display_hint,
        })
        .collect();
    serde_json::to_string(&stored).unwrap_or_else(|_| "[]".to_owned())
}

pub(super) fn encode_mentions(mentions: &[PrincipalId]) -> String {
    let raw: Vec<&str> = mentions.iter().map(PrincipalId::as_str).collect();
    serde_json::to_string(&raw).unwrap_or_else(|_| "[]".to_owned())
}
