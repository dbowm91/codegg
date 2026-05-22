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
    pub time_created: i64,        // milliseconds since epoch
    pub time_updated: i64,
    pub time_compacting: Option<i64>,
    pub time_archived: Option<i64>,
    pub time_deleted: Option<i64>, // soft delete timestamp
}
```

### Message (stored as JSON)

```rust
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: MessageData,  // JSON-serialized content
}

pub struct MessageData {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub parts: Vec<PartInfo>,
}

pub enum PartData {
    Text { text: String },
    Reasoning { reasoning: String },
    ToolCall { id: String, name: String, input: Value, output: Option<String>, status: ToolStatus },
    Image { url: String },
    File { path: String, content: String },
}
```

### Checkpoint

```rust
pub struct Checkpoint {
    pub id: String,
    pub timestamp: i64,
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub messages: Vec<Message>,
    pub completed_steps: Vec<String>,
    pub working_files: Vec<WorkingFile>,
}
```

### TodoItem

```rust
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

## Components

### store.rs - Storage Operations

```rust
pub struct SessionStore {
    pool: SqlitePool,
}

impl SessionStore {
    // Session CRUD
    pub async fn create(&self, input: CreateSession) -> Result<Session, StorageError>
    pub async fn get(&self, id: &str) -> Result<Option<Session>, StorageError>
    pub async fn update(&self, id: &str, input: UpdateSession) -> Result<Session, StorageError>
    pub async fn delete(&self, id: &str) -> Result<(), StorageError>  // soft delete

    // Listing and search
    pub async fn list(&self, project_id: &str, limit: usize) -> Result<Vec<Session>, StorageError>
    pub async fn list_all(&self, project_id: &str, limit: Option<usize>) -> Result<Vec<Session>, StorageError>
    pub async fn search(&self, project_id: &str, query: &str) -> Result<Vec<Session>, StorageError>
    pub async fn find_by_tag(&self, project_id: &str, tag: &str) -> Result<Vec<Session>, StorageError>
    pub async fn all_tags(&self, project_id: &str) -> Result<Vec<String>, StorageError>

    // Soft delete/restore
    pub async fn soft_delete(&self, id: &str) -> Result<Session, StorageError>
    pub async fn restore(&self, id: &str) -> Result<Session, StorageError>
    pub async fn list_deleted(&self, project_id: &str) -> Result<Vec<Session>, StorageError>

    // Archive
    pub async fn archive(&self, id: &str) -> Result<Session, StorageError>
    pub async fn unarchive(&self, id: &str) -> Result<Session, StorageError>

    // Fork
    pub async fn fork(&self, id: &str) -> Result<Session, StorageError>

    // Tags
    pub async fn set_tags(&self, id: &str, tags: Vec<String>) -> Result<Session, StorageError>

    // Revert
    pub async fn revert_to_message(&self, session_id: &str, message_id: &str) -> Result<Session, StorageError>
    pub async fn unrevert_session(&self, session_id: &str) -> Result<Session, StorageError>

    // Sharing
    pub async fn share_session(&self, session_id: &str) -> Result<Session, StorageError>
    pub async fn unshare_session(&self, session_id: &str) -> Result<Session, StorageError>

    // Import/export
    pub async fn export_session(&self, session_id: &str) -> Result<serde_json::Value, StorageError>
    pub async fn import_session(&self, data: serde_json::Value, new_project_id: Option<&str>) -> Result<Session, StorageError>

    // Analytics
    pub async fn get_analytics(&self, project_id: &str) -> Result<SessionAnalytics, StorageError>
}
```

Also provides: `MessageStore`, `PartStore`, `TodoStore`, `PermissionStore`

### checkpoint.rs - Session Checkpointing

```rust
pub struct CheckpointStore {
    pool: SqlitePool,
}

impl CheckpointStore {
    pub async fn save(&self, checkpoint: &Checkpoint) -> Result<(), StorageError>
    pub async fn load(&self, id: &str) -> Result<Option<Checkpoint>, StorageError>
    pub async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>, StorageError>
    pub async fn list(&self, session_id: &str) -> Result<Vec<Checkpoint>, StorageError>
    pub async fn delete(&self, id: &str) -> Result<(), StorageError>
    pub async fn delete_all(&self, session_id: &str) -> Result<(), StorageError>
}
```

### import.rs - Session Import/Export

```rust
pub fn validate_import_size(data: &serde_json::Value) -> Result<usize, StorageError>
// Enforces: MAX_IMPORT_MESSAGES=100,000, MAX_IMPORT_PARTS=500,000, MAX_TOTAL_IMPORT_BYTES=500MB

pub fn redact_for_export(value: serde_json::Value) -> serde_json::Value
// Redacts sensitive tool inputs/outputs for: bash, write, read, edit, replace, multiedit, terminal, git, webfetch, apply_patch
```

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

Session changes publish events to `GlobalEventBus`:

- `SessionCreated` - New session created
- `SessionSelected` - Session activated
- `SessionDeleted` - Session deleted (soft delete)
- `SessionRenamed` - Session renamed
- `MessageAdded` - New message in session

## Interactions

```
TUI
├── SessionStore::list_sessions()
├── SessionStore::get_session()
├── SessionStore::add_message()
└── SessionStore::share_session()

AgentLoop
├── SessionStore::add_message()
├── SessionStore::list_messages() (for context)
├── CheckpointStore::save()
└── CheckpointStore::load_latest()
```

## Configuration

No specific configuration - uses database path from `storage::Database`.

## See Also

- [storage.md](storage.md) - Database initialization
- [tui.md](tui.md) - TUI that displays sessions
- [agent.md](agent.md) - AgentLoop that stores messages
- [error.md](error.md) - StorageError enum definition