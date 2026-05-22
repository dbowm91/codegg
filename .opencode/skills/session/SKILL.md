---
name: session
description: Session storage, database schema, CRUD operations
version: 1.2.0
tags:
  - session
  - storage
  - sqlite
  - database
  - crud
---

# Session System Guide

This skill covers the session storage and persistence system in opencode-rs.

## Architecture Overview

```
SessionStore (session/store.rs) - Main store implementation
├── SQLite database (sessions, messages, parts, todos, permissions)
├── Session CRUD operations
├── Import/export functionality
└── QueryBuilder for batch operations

Supporting modules:
├── row.rs - Database row mappings
├── models.rs - Session, CreateSession, UpdateSession structs
├── import.rs - Import validation, redaction, and processing
├── message.rs - Message, Part, PartData types with serialization tests
├── checkpoint.rs - Session checkpoints with CheckpointStore
├── status.rs - SessionStatus and SessionState for UI
└── schema.rs - Database migrations v1-v14
```

## Database Schema

### Core Tables

**session** - Session metadata:
```sql
CREATE TABLE IF NOT EXISTS session (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    workspace_id TEXT,
    parent_id TEXT,
    slug TEXT NOT NULL,
    directory TEXT NOT NULL,
    title TEXT NOT NULL,
    version TEXT NOT NULL,
    share_url TEXT,
    summary_additions INTEGER,
    summary_deletions INTEGER,
    summary_files INTEGER,
    summary_diffs TEXT,
    revert TEXT,
    permission TEXT,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_compacting INTEGER,
    time_archived INTEGER,
    time_deleted INTEGER,
    FOREIGN KEY (project_id) REFERENCES project(id) ON DELETE CASCADE
)
```

**message** - Individual messages:
```sql
CREATE TABLE IF NOT EXISTS message (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
)
```

**part** - Message content parts:
```sql
CREATE TABLE IF NOT EXISTS part (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data TEXT NOT NULL,
    FOREIGN KEY (message_id) REFERENCES message(id) ON DELETE CASCADE
)
```

### Supporting Tables

**project** - Project tracking:
```sql
CREATE TABLE IF NOT EXISTS project (
    id TEXT PRIMARY KEY,
    worktree TEXT NOT NULL,
    vcs TEXT,
    name TEXT,
    icon_url TEXT,
    icon_color TEXT,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_initialized INTEGER,
    sandboxes TEXT NOT NULL
)
```

**todo** - Todo items:
```sql
CREATE TABLE IF NOT EXISTS todo (
    session_id TEXT NOT NULL,
    content TEXT NOT NULL,
    status TEXT NOT NULL,
    priority TEXT NOT NULL,
    position INTEGER NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    PRIMARY KEY (session_id, position),
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
)
```

**session_share** - Session sharing with expiring tokens:
```sql
CREATE TABLE IF NOT EXISTS session_share (
    session_id TEXT PRIMARY KEY,
    id TEXT NOT NULL,
    secret TEXT NOT NULL,
    url TEXT NOT NULL,
    share_expires_at INTEGER,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
)
```

**checkpoints** - Session checkpoints for resume:
```sql
CREATE TABLE IF NOT EXISTS checkpoints (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    state TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
)
```

**task** - Task tracking:
```sql
CREATE TABLE IF NOT EXISTS task (
    id INTEGER PRIMARY KEY,
    parent_id TEXT,
    session_id TEXT NOT NULL,
    description TEXT NOT NULL,
    prompt TEXT NOT NULL,
    agent TEXT NOT NULL,
    status TEXT NOT NULL,
    result TEXT,
    denied_tools TEXT,
    allowed_paths TEXT,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL
)
```

**snapshot** - File snapshots:
```sql
CREATE TABLE IF NOT EXISTS snapshot (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    label TEXT,
    data TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
)
```

**migration_version** - Schema version tracking:
```sql
CREATE TABLE IF NOT EXISTS migration_version (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL DEFAULT 0
)
```

## Session Struct

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_id: String,
    pub workspace_id: Option<String>,
    pub parent_id: Option<String>,
    pub slug: String,
    pub directory: String,
    pub title: String,
    pub version: String,
    pub share_url: Option<String>,
    pub summary_additions: Option<i64>,
    pub summary_deletions: Option<i64>,
    pub summary_files: Option<i64>,
    pub summary_diffs: Option<serde_json::Value>,
    pub revert: Option<serde_json::Value>,
    pub permission: Option<serde_json::Value>,
    pub tags: Vec<String>,
    pub time_created: i64,
    pub time_updated: i64,
    pub time_compacting: Option<i64>,
    pub time_archived: Option<i64>,
    pub time_deleted: Option<i64>,
}
```

Note: `CreateSession` has `agent` and `model` fields that are accepted during creation but **not stored** in the database.

## Message Struct (internal JSON storage)

Messages store data as serialized JSON:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: MessageData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageData {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(rename = "messageID")]
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub parts: Vec<PartInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartData {
    Text { text: String },
    Reasoning { reasoning: String },
    ToolCall { id: String, name: String, input: Value, output: Option<String>, status: ToolStatus },
    Image { url: String },
    File { path: String, content: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Error,
}
```

## Stores

The module provides multiple stores, all re-exported from `session::mod.rs`:

```rust
pub use store::{
    escape_sql_like, generate_slug, MessageStore, PartStore, PermissionStore, SessionStore, TodoStore,
};
pub use checkpoint::CheckpointStore;
```

### SessionStore - Main session operations

```rust
impl SessionStore {
    // Session CRUD
    pub async fn create(&self, input: CreateSession) -> Result<Session, StorageError>
    pub async fn create_from_template(&self, template: &SessionTemplate, project_id: &str, directory: &str) -> Result<Session, StorageError>
    pub async fn get(&self, id: &str) -> Result<Option<Session>, StorageError>
    pub async fn update(&self, id: &str, input: UpdateSession) -> Result<Session, StorageError>
    pub async fn delete(&self, id: &str) -> Result<(), StorageError>  // soft delete

    // Listing and search
    pub async fn list(&self, project_id: &str, limit: usize) -> Result<Vec<Session>, StorageError>
    pub async fn list_with_offset(&self, project_id: &str, limit: usize, offset: usize) -> Result<Vec<Session>, StorageError>
    pub async fn list_all(&self, project_id: &str, limit: Option<usize>) -> Result<Vec<Session>, StorageError>
    pub async fn list_all_with_offset(&self, project_id: &str, limit: Option<usize>, offset: usize) -> Result<Vec<Session>, StorageError>
    pub async fn search(&self, project_id: &str, query: &str) -> Result<Vec<Session>, StorageError>
    pub async fn search_all(&self, project_id: &str, query: &str) -> Result<Vec<Session>, StorageError>
    pub async fn find_by_tag(&self, project_id: &str, tag: &str) -> Result<Vec<Session>, StorageError>
    pub async fn all_tags(&self, project_id: &str) -> Result<Vec<String>, StorageError>

    // Counts
    pub async fn session_count(&self, project_id: &str) -> Result<usize, StorageError>
    pub async fn message_count(&self, session_id: &str) -> Result<usize, StorageError>
    pub async fn message_counts(&self, session_ids: &[String]) -> Result<HashMap<String, usize>, StorageError>

    // Soft delete/restore
    pub async fn soft_delete(&self, id: &str) -> Result<Session, StorageError>
    pub async fn restore(&self, id: &str) -> Result<Session, StorageError>
    pub async fn list_deleted(&self, project_id: &str) -> Result<Vec<Session>, StorageError>

    // Archive
    pub async fn archive(&self, id: &str) -> Result<Session, StorageError>
    pub async fn unarchive(&self, id: &str) -> Result<Session, StorageError>

    // Fork
    pub async fn fork(&self, id: &str) -> Result<Session, StorageError>

    // Hierarchy
    pub async fn children(&self, id: &str) -> Result<Vec<Session>, StorageError>

    // Tags
    pub async fn set_tags(&self, id: &str, tags: Vec<String>) -> Result<Session, StorageError>

    // Revert
    pub async fn revert_to_message(&self, session_id: &str, message_id: &str) -> Result<Session, StorageError>
    pub async fn unrevert_session(&self, session_id: &str) -> Result<Session, StorageError>

    // Sharing
    pub async fn share_session(&self, session_id: &str) -> Result<Session, StorageError>
    pub async fn unshare_session(&self, session_id: &str) -> Result<Session, StorageError>
    pub async fn set_share_url(&self, id: &str, url: &str) -> Result<Session, StorageError>

    // Summary generation
    pub async fn generate_summary(&self, provider: &impl SessionSummaryProvider, session_id: &str) -> Result<Session, StorageError>
    pub async fn generate_title(&self, provider: &impl SessionSummaryProvider, session_id: &str) -> Result<Session, StorageError>

    // Import/export
    pub async fn export_session(&self, session_id: &str) -> Result<Value, StorageError>
    pub async fn import_session(&self, data: Value, new_project_id: Option<&str>) -> Result<Session, StorageError>

    // Analytics
    pub async fn get_analytics(&self, project_id: &str) -> Result<SessionAnalytics, StorageError>
}
```

### MessageStore - Message operations

```rust
impl MessageStore {
    pub async fn create(&self, session_id: &str, data: Value) -> Result<Message, StorageError>
    pub async fn get(&self, session_id: &str, id: &str) -> Result<Option<Message>, StorageError>
    pub async fn list(&self, session_id: &str) -> Result<Vec<Message>, StorageError>
    pub async fn count(&self, session_id: &str) -> Result<usize, StorageError>
    pub async fn update(&self, session_id: &str, id: &str, data: Value) -> Result<Message, StorageError>
    pub async fn delete(&self, session_id: &str, id: &str) -> Result<(), StorageError>
}
```

### PartStore - Part operations

```rust
impl PartStore {
    pub async fn create(&self, message_id: &str, session_id: &str, data: Value) -> Result<Part, StorageError>
    pub async fn get(&self, id: &str) -> Result<Option<Part>, StorageError>
    pub async fn list_by_message(&self, message_id: &str) -> Result<Vec<Part>, StorageError>
    pub async fn list_by_session(&self, session_id: &str) -> Result<Vec<Part>, StorageError>
    pub async fn update(&self, id: &str, data: Value) -> Result<Part, StorageError>
    pub async fn delete(&self, id: &str) -> Result<(), StorageError>
}
```

### TodoStore - Todo operations

```rust
impl TodoStore {
    pub async fn list(&self, session_id: &str) -> Result<Vec<TodoItem>, StorageError>
    pub async fn set(&self, session_id: &str, items: Vec<TodoItemInput>) -> Result<Vec<TodoItem>, StorageError>
    pub async fn add(&self, session_id: &str, item: TodoItemInput) -> Result<TodoItem, StorageError>
    pub async fn update(&self, session_id: &str, position: i64, item: TodoItemInput) -> Result<Vec<TodoItem>, StorageError>
    pub async fn remove(&self, session_id: &str, position: i64) -> Result<Vec<TodoItem>, StorageError>
    pub async fn clear(&self, session_id: &str) -> Result<(), StorageError>
}
```

### PermissionStore - Permission operations

```rust
impl PermissionStore {
    pub async fn get(&self, project_id: &str) -> Result<Option<PermissionEntry>, StorageError>
    pub async fn upsert(&self, project_id: &str, data: Value) -> Result<PermissionEntry, StorageError>
    pub async fn delete(&self, project_id: &str) -> Result<(), StorageError>
}
```

### CheckpointStore - Checkpoint operations

```rust
impl CheckpointStore {
    pub fn new(pool: SqlitePool) -> Self
    pub async fn save(&self, checkpoint: &Checkpoint) -> Result<(), StorageError>
    pub async fn load(&self, id: &str) -> Result<Option<Checkpoint>, StorageError>
    pub async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>, StorageError>
    pub async fn list(&self, session_id: &str) -> Result<Vec<Checkpoint>, StorageError>
    pub async fn delete(&self, id: &str) -> Result<(), StorageError>
    pub async fn delete_all(&self, session_id: &str) -> Result<(), StorageError>
    pub async fn has_checkpoint(&self, session_id: &str) -> Result<bool, StorageError>
}

// Helper functions
pub fn compute_checksum(content: &str) -> String;
pub fn create_working_file(path: &str, pre_state: Option<String>) -> Option<WorkingFile>;
pub fn verify_file(path: &str, expected_checksum: &str) -> bool;
```

## Query Constants

The module defines these constants to avoid duplication:

```rust
const SESSION_COLUMNS: &str = r#"id, project_id, workspace_id, parent_id, slug, directory,
    title, version, share_url, summary_additions, summary_deletions,
    summary_files, summary_diffs, revert, permission, tags,
    time_created, time_updated, time_compacting, time_archived, time_deleted"#;

const SESSION_COLUMNS_QUALIFIED: &str = r#"s.id, s.project_id, s.workspace_id, s.parent_id, s.slug, s.directory,
    s.title, s.version, s.share_url, s.summary_additions, s.summary_deletions,
    s.summary_files, s.summary_diffs, s.revert, s.permission, s.tags,
    s.time_created, s.time_updated, s.time_compacting, s.time_archived, s.time_deleted"#;

const MESSAGE_QUERY: &str = r#"SELECT id, session_id, time_created, time_updated, data
    FROM message WHERE session_id = ?
    ORDER BY time_created ASC, id ASC"#;

const PART_QUERY: &str = r#"SELECT id, message_id, session_id, time_created, time_updated, data
    FROM part WHERE session_id = ?
    ORDER BY time_created ASC, id ASC"#;
```

## Soft Delete Pattern

Sessions use soft delete with `time_deleted` column (added in migration v12):

```rust
pub async fn soft_delete(&self, id: &str) -> Result<Session, StorageError> {
    sqlx::query_as::<_, SessionRow>(
        "UPDATE session SET time_deleted = ?, time_updated = ? WHERE id = ? RETURNING *"
    )
    .bind(now)
    .bind(now)
    .bind(id)
    .fetch_one(&self.pool)
    .await
    .map_err(|e| StorageError::Database(e.to_string()))?
    .into()
}

pub async fn restore(&self, id: &str) -> Result<Session, StorageError> {
    sqlx::query_as::<_, SessionRow>(
        "UPDATE session SET time_deleted = NULL, time_updated = ? WHERE id = ? RETURNING *"
    )
    .bind(now)
    .bind(now)
    .bind(id)
    .fetch_one(&self.pool)
    .await?
    .into()
}
```

## Helper Functions

### generate_slug

Generates URL-friendly slugs from session titles:

```rust
pub fn generate_slug(title: &Option<String>) -> String {
    title
        .as_ref()
        .map(|t| {
            t.to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ')
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "untitled".to_string())
}
```

### escape_sql_like

Escapes special characters for SQL LIKE queries:

```rust
pub fn escape_sql_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
```

### redact_for_export

Redacts sensitive tool inputs/outputs during export:

```rust
pub fn redact_for_export(value: Value) -> Value {
    // Recursively processes JSON, redacting:
    // - tool_call inputs/outputs
    // - Specific keys: command, path, content, text, pattern, replacement,
    //   old_string, new_string, url, patch for sensitive tools (bash, write, read, edit, etc.)
}
```

### parse_json_field

Graceful JSON parsing helper that returns `Null` on failure (with warning logged):

```rust
pub(crate) fn parse_json_field(raw: &str) -> serde_json::Value {
    match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            let preview = if raw.len() > 100 {
                format!("{}...", &raw[..100])
            } else {
                raw.to_string()
            };
            warn!(
                "failed to parse JSON field (input preview: {}): {}",
                preview, e
            );
            serde_json::Value::Null
        }
    }
}
```

## RETURNING Clause Usage

Methods that update and need the updated session use `RETURNING *` to avoid extra SELECT:

```rust
// Soft delete with RETURNING
"UPDATE session SET time_deleted = ?, time_updated = ? WHERE id = ? RETURNING *"

// Set tags with RETURNING
"UPDATE session SET time_updated = ?, tags = ? WHERE id = ? RETURNING *"

// Archive/unarchive with RETURNING
"UPDATE session SET time_archived = ?, time_updated = ? WHERE id = ? RETURNING *"
"UPDATE session SET time_archived = NULL, time_updated = ? WHERE id = ? RETURNING *"

// Share URL with RETURNING
"UPDATE session SET share_url = ?, time_updated = ? WHERE id = ? RETURNING *"
```

## Batch Inserts with QueryBuilder

Bulk operations use `QueryBuilder::push_values()` for efficient batch inserts:

```rust
// In fork() or import_session()
let mut msg_query: QueryBuilder<_> = QueryBuilder::new(
    "INSERT INTO message (id, session_id, time_created, time_updated, data) ",
);
msg_query.push_values(&msg_values, |mut b, val| {
    b.push_bind(&val.0)
        .push_bind(&val.1)
        .push_bind(val.2)
        .push_bind(val.3)
        .push_bind(&val.4);
});
msg_query.build().execute(&mut *tx).await?;
```

## Transaction Safety

**Important**: When using transactions, ALL database operations must use `&mut *tx`, not `&self.pool`:

```rust
// Good - all operations use the transaction
let mut tx = self.pool.begin().await?;
sqlx::query("...").bind(id).execute(&mut *tx).await?;
tx.commit().await?;

// Bad - mixing transaction and pool usage
let mut tx = self.pool.begin().await?;
self.get(id).await?;  // Uses pool, not transaction!
sqlx::query("...").bind(id).execute(&mut *tx).await?;
tx.commit().await?;  // May timeout due to pool exhaustion
```

## Import Limits

Import validation enforces these limits:

```rust
const MAX_IMPORT_MESSAGES: usize = 100_000;
const MAX_IMPORT_PARTS: usize = 500_000;
const MAX_TOTAL_IMPORT_BYTES: usize = 500 * 1024 * 1024; // 500 MB
```

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("llm operation failed: {operation}: {message}")]
    LlmOperation { operation: String, message: String },

    #[error("import error: {0}")]
    Import(String),

    #[error("export error: {0}")]
    Export(String),
}
```

## SessionStatus and SessionState (status.rs)

For TUI state tracking:

```rust
#[derive(Debug, Clone, Default)]
pub enum SessionStatus {
    #[default]
    Idle,
    Busy,
    Error,
    Compacting,
    Exporting,
}

impl SessionStatus {
    pub fn is_busy(&self) -> bool;  // true for Busy, Compacting, Exporting
    pub fn is_terminal(&self) -> bool;  // true only for Error
    pub fn label(&self) -> &'static str;
    pub fn icon(&self) -> &'static str;
}

#[derive(Debug, Clone, Default)]
pub struct SessionState {
    pub status: SessionStatus,
    pub started_at: Option<SystemTime>,
    pub last_activity: Option<SystemTime>,
    pub turn_count: usize,
    pub token_in: usize,
    pub token_out: usize,
    pub error_message: Option<String>,
}
```