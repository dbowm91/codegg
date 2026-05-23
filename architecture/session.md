# Session Module

The `session` module handles persistent storage of conversation sessions, messages, and related data using SQLite.

## Overview

**Location**: `src/session/`

**Key Responsibilities**:
- Session CRUD operations with soft delete/restore
- Message history storage (as JSON)
- Part storage (content parts within messages)
- Todo item tracking
- Session checkpointing for resume
- Session sharing with expiring tokens
- Import/export with redaction

## Key Types

### Session

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
    pub time_created: i64,        // milliseconds since epoch
    pub time_updated: i64,
    pub time_compacting: Option<i64>,
    pub time_archived: Option<i64>,
    pub time_deleted: Option<i64>, // soft delete timestamp
}
```

### Message (stored as JSON)

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
pub struct PartInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(flatten)]
    pub data: PartData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartData {
    Text { text: String },
    Reasoning { reasoning: String },
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
        output: Option<String>,
        status: ToolStatus,
    },
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

### Checkpoint

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub timestamp: i64,
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub messages: Vec<message::Message>,
    pub completed_steps: Vec<String>,
    pub working_files: Vec<WorkingFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingFile {
    pub path: String,
    pub checksum: String,
    pub pre_state: Option<String>,
}
```

### TodoItem

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub session_id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
    pub position: i64,
    pub time_created: i64,
    pub time_updated: i64,
}
```

### SessionStatus and SessionState (status.rs)

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
    pub fn is_busy(&self) -> bool;
    pub fn is_terminal(&self) -> bool;
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

impl SessionState {
    pub fn new() -> Self;
    pub fn start(&mut self);
    pub fn idle(&mut self);
    pub fn error(&mut self, msg: String);
    pub fn compacting(&mut self);
    pub fn exporting(&mut self);
    pub fn record_turn(&mut self, tokens_in: usize, tokens_out: usize);
    pub fn duration(&self) -> Option<std::time::Duration>;
    pub fn is_idle(&self) -> bool;
    pub fn is_active(&self) -> bool;
}
```

## Components

### store.rs - Storage Operations (2061 lines)

```rust
pub struct SessionStore {
    pool: SqlitePool,
}

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

    // Children
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
    pub async fn export_session(&self, session_id: &str) -> Result<serde_json::Value, StorageError>
    pub async fn import_session(&self, data: serde_json::Value, new_project_id: Option<&str>) -> Result<Session, StorageError>

    // Analytics
    pub async fn get_analytics(&self, project_id: &str) -> Result<SessionAnalytics, StorageError>
}
```

### checkpoint.rs - Session Checkpointing (177 lines)

```rust
pub struct CheckpointStore {
    pool: SqlitePool,
}

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

### import.rs - Session Import/Export (180 lines)

```rust
pub fn validate_import_size(data: &serde_json::Value) -> Result<usize, StorageError>
// Enforces: MAX_IMPORT_MESSAGES=100,000, MAX_IMPORT_PARTS=500,000, MAX_TOTAL_IMPORT_BYTES=500MB

pub fn redact_for_export(value: serde_json::Value) -> serde_json::Value
// Redacts sensitive tool inputs/outputs for: bash, write, read, edit, replace, multiedit, terminal, git, webfetch, apply_patch
```

### message.rs - Message Types (212 lines)

Contains `Message`, `MessageData`, `PartInfo`, `PartData`, `ToolStatus`, `Part` types with comprehensive serialization tests.

### status.rs - Session Status (116 lines)

Contains `SessionStatus` enum and `SessionState` struct for tracking session UI state.

## Database Schema

See `schema.rs` for migration definitions (v1-v14):

### Core Tables

**session**:
```sql
CREATE TABLE session (
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
    tags TEXT,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_compacting INTEGER,
    time_archived INTEGER,
    time_deleted INTEGER,
    FOREIGN KEY (project_id) REFERENCES project(id) ON DELETE CASCADE
)
```

**message**:
```sql
CREATE TABLE message (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data TEXT NOT NULL,  -- JSON containing role, parts, etc.
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
)
```

**part**:
```sql
CREATE TABLE part (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    data TEXT NOT NULL,  -- JSON with type-specific content
    FOREIGN KEY (message_id) REFERENCES message(id) ON DELETE CASCADE
)
```

**todo**:
```sql
CREATE TABLE todo (
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

**checkpoints**:
```sql
CREATE TABLE checkpoints (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    state TEXT NOT NULL,  -- JSON serialized Checkpoint
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
)
```

### Supporting Tables

**session_share** (v1, with `share_expires_at` added in v5):
```sql
CREATE TABLE session_share (
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

**task** (v9, with `allowed_paths` added in v14):
```sql
CREATE TABLE task (
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

**snapshot** (v13):
```sql
CREATE TABLE snapshot (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    label TEXT,
    data TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
)
```

**cached_models** (v3):

**migration_version** (v1):
```sql
CREATE TABLE migration_version (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL DEFAULT 0
)
```

## Module Exports

```rust
pub use models::{
    CreateSession, PermissionEntry, Session, SessionAnalytics, SessionSummaryProvider, TodoItem,
    TodoItemInput, UpdateSession,
};
pub use row::{MessageRow, PartRow, PermissionRow, SessionRow, TodoRow};
pub use store::{
    escape_sql_like, generate_slug, MessageStore, PartStore, PermissionStore, SessionStore, TodoStore,
};
pub use checkpoint::CheckpointStore;
```

## Event Publishing

Events are published via `GlobalEventBus` in `src/bus/events.rs`:

- `SessionCreated` - New session created
- `MessageAdded` - New message in session

Note: `SessionSelected`, `SessionDeleted`, `SessionRenamed` are listed in the architecture but are not currently published as events.

## Helper Functions

```rust
pub fn escape_sql_like(s: &str) -> String  // Escapes SQL LIKE special characters
pub fn generate_slug(title: &Option<String>) -> String  // Creates URL-friendly slug
pub(crate) fn parse_json_field(raw: &str) -> serde_json::Value  // Graceful JSON parsing with warning on failure
pub(crate) use import::redact_for_export
```

## Query Constants

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

## Notes

- `CreateSession` has `agent` and `model` fields that are accepted during creation but not stored in the database (no corresponding columns in session table)
- `CheckpointStore::has_checkpoint()` renamed from `has_unfinished()` for clarity. The name reflects its semantic meaning: it checks whether a checkpoint exists for the session (not whether work is "unfinished").
- `PartRow` uses `parse_json_field()` helper while `MessageRow` uses direct `TryFrom` deserialization - this inconsistency in JSON error handling is known
- Session state is tracked via `SessionStatus` and `SessionState` in `status.rs` for TUI display

## See Also

- [storage.md](storage.md) - Database initialization
- [tui.md](tui.md) - TUI that displays sessions
- [agent.md](agent.md) - AgentLoop that stores messages
- [error.md](error.md) - StorageError enum definition