---
name: session
description: Session storage, database schema, CRUD operations
version: 1.0.0
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
├── SQLite database (sessions, messages, parts, todos)
├── Session CRUD operations
├── Import/export functionality
└── QueryBuilder for batch operations

Supporting modules:
├── row.rs - Database row mappings
├── models.rs - Session, CreateSession, UpdateSession structs
├── import.rs - Import validation and processing
├── message.rs - Message storage utilities
├── checkpoint.rs - Session checkpoints
└── schema.rs - Database migrations
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

**migration_version** - Schema version tracking:
```sql
CREATE TABLE IF NOT EXISTS migration_version (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL DEFAULT 0
)
```

## Session Struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
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

## Key Operations

### Create Session

```rust
pub async fn create(&self, input: CreateSession) -> Result<Session, StorageError>
```

### Get Session

```rust
pub async fn get(&self, id: &str) -> Result<Session, StorageError>
pub async fn get_with_messages(&self, id: &str) -> Result<(Session, Vec<Message>), StorageError>
```

### Update Session

```rust
pub async fn update(&self, id: &str, input: UpdateSession) -> Result<Session, StorageError>
pub async fn set_tags(&self, id: &str, tags: Vec<String>) -> Result<Session, StorageError>
pub async fn archive(&self, id: &str) -> Result<Session, StorageError>
pub async fn unarchive(&self, id: &str) -> Result<Session, StorageError>
```

### Soft Delete/Restore

Sessions use soft delete with `time_deleted` column (added in migration v12):

```rust
pub async fn soft_delete(&self, id: &str) -> Result<Session, StorageError> {
    sqlx::query("UPDATE session SET time_deleted = ?, time_updated = ? WHERE id = ? RETURNING *")
    // ...
}

pub async fn restore(&self, id: &str) -> Result<Session, StorageError> {
    sqlx::query("UPDATE session SET time_deleted = NULL, time_updated = ? WHERE id = ? RETURNING *")
    // ...
}

pub async fn list_deleted(&self, project_id: &str) -> Result<Vec<Session>, StorageError> {
    sqlx::query(&format!("SELECT {} FROM session WHERE project_id = ? AND time_deleted IS NOT NULL ORDER BY time_deleted DESC", SESSION_COLUMNS))
    // ...
}
```

### Hard Delete

```rust
pub async fn delete(&self, id: &str) -> Result<(), StorageError>
```

### Fork Session

```rust
pub async fn fork(&self, id: &str) -> Result<Session, StorageError>
```

### Search Sessions

All search methods use `escape_sql_like()` and filter out deleted sessions:

```rust
pub async fn search(&self, query: &str) -> Result<Vec<Session>, StorageError>
pub fn search_all(&self, query: &str) -> impl Stream<Item = Result<Session, StorageError>>
pub async fn find_by_tag(&self, tag: &str) -> Result<Vec<Session>, StorageError>
```

## Query Constants

The codebase uses constants to avoid duplication:

```rust
const SESSION_COLUMNS: &str = r#"id, project_id, workspace_id, parent_id, slug, directory,
    title, version, share_url, summary_additions, summary_deletions,
    summary_files, summary_diffs, revert, permission, tags,
    time_created, time_updated, time_compacting, time_archived"#;

const SESSION_COLUMNS_QUALIFIED: &str = r#"s.id, s.project_id, s.workspace_id, s.parent_id, s.slug, s.directory,
    s.title, s.version, s.share_url, s.summary_additions, s.summary_deletions,
    s.summary_files, s.summary_diffs, s.revert, s.permission, s.tags,
    s.time_created, s.time_updated, s.time_compacting, s.time_archived"#;

const MESSAGE_QUERY: &str = r#"SELECT id, session_id, time_created, time_updated, data
    FROM message WHERE session_id = ?
    ORDER BY time_created ASC, id ASC"#;
```

## Message Storage

### Retrieve Messages

```rust
pub async fn messages(&self, session_id: &str) -> Result<Vec<Message>, StorageError>
pub async fn parts(&self, session_id: &str) -> Result<Vec<Part>, StorageError>
```

### Store Messages

```rust
pub async fn store_message(&mut self, session_id: &str, message: Message) -> Result<Message, StorageError>
pub async fn store_part(&mut self, session_id: &str, part: Part) -> Result<Part, StorageError>
```

## Import/Export

### Export Session

```rust
pub async fn export(&self, id: &str) -> Result<Value, StorageError>
```

Options:
- `include_messages`: Include message history
- `include_parts`: Include message parts
- `redact`: Redact sensitive tool inputs/outputs

### Import Session

```rust
pub async fn import(&mut self, data: Value) -> Result<Session, StorageError>
```

**Limits:**
- MAX_IMPORT_MESSAGES: 100,000
- MAX_IMPORT_PARTS: 500,000
- MAX_TOTAL_IMPORT_BYTES: 500 MB

## Session Templates

Sessions can be created from templates:

```rust
pub struct SessionTemplate {
    pub id: String,
    pub project_id: String,
    pub template: Option<String>,
    pub workspace_id: Option<String>,
    pub directory: String,
    pub title: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub permission: Option<PermissionConfig>,
    pub system_prompt: Option<String>,
}
```

## Tagging System

Sessions support tags for organization:

```rust
pub async fn set_tags(&self, id: &str, tags: Vec<String>) -> Result<Session, StorageError>
pub async fn add_tag(&self, id: &str, tag: &str) -> Result<Session, StorageError>
pub async fn remove_tag(&self, id: &str, tag: &str) -> Result<Session, StorageError>
pub async fn find_by_tag(&self, tag: &str) -> Result<Vec<Session>, StorageError>
```

## RETURNING Clause Usage

Methods that update and need the updated session use `RETURNING *` to avoid separate SELECT:

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
pub async fn unrevert_session(&self, id: &str) -> Result<Session, StorageError> {
    let session = self.get(id).await?;
    let revert_data: serde_json::Value = session.revert.ok_or_else(|| ...)?;
    // ...
    let mut msg_query: QueryBuilder<_> = QueryBuilder::new("INSERT INTO message (id, session_id, time_created, time_updated, data) ");
    msg_query.push_values(messages.iter(), |mut b, msg| {
        b.push_bind(msg.id)
            .push_bind(&session.id)
            .push_bind(msg.time_created)
            .push_bind(msg.time_updated)
            .push_bind(&msg.data);
    });
    let msg_result = msg_query.build().execute(&self.pool).await?;
    // ...
}
```

## Transaction Safety

**Important**: When using transactions, ALL database operations must use `&mut *tx`, not `&self.pool`: 

```rust
// ✅ Good - all operations use the transaction
let mut tx = self.pool.begin().await?;
sqlx::query("...").bind(id).execute(&mut *tx).await?;
tx.commit().await?;

// ❌ Bad - mixing transaction and pool usage
let mut tx = self.pool.begin().await?;
self.get(id).await?;  // Uses pool, not transaction!
sqlx::query("...").bind(id).execute(&mut *tx).await?;
tx.commit().await?;  // May timeout due to pool exhaustion
```

## SQL LIKE Escaping

Search functions use `escape_sql_like()` to handle special characters:

```rust
fn escape_sql_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
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

    #[error("import error: {0}")]
    Import(String),

    #[error("export error: {0}")]
    Export(String),
}
```