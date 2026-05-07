# Session Module

The `session` module handles persistent storage of conversation sessions, messages, and related data.

## Overview

**Location**: `src/session/`

**Key Responsibilities**:
- Session CRUD operations
- Message history storage
- Part storage (content parts within messages)
- Todo item tracking
- Session checkpointing

## Key Types

### Session

```rust
pub struct Session {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
    pub model: Option<String>,
    pub tags: Vec<String>,
}
```

### Message

```rust
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: MessageRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub parts: Vec<Part>,
}

pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}
```

### Part

```rust
pub struct Part {
    pub id: String,
    pub message_id: String,
    pub part_type: PartType,
    pub content: String,
}

pub enum PartType {
    Text,
    ToolCall,
    ToolResult,
    Error,
}
```

### TodoItem

```rust
pub struct TodoItem {
    pub id: String,
    pub session_id: String,
    pub content: String,
    pub completed: bool,
    pub created_at: DateTime<Utc>,
}
```

## Components

### store.rs - Storage Operations

```rust
pub struct SessionStore {
    db: SqlitePool,
}

impl SessionStore {
    // Sessions
    pub async fn list_sessions(&self) -> Result<Vec<Session>>;
    pub async fn get_session(&self, id: &str) -> Result<Option<Session>>;
    pub async fn create_session(&self, session: &Session) -> Result<()>;
    pub async fn update_session(&self, session: &Session) -> Result<()>;
    pub async fn delete_session(&self, id: &str) -> Result<()>;

    // Messages
    pub async fn list_messages(&self, session_id: &str) -> Result<Vec<Message>>;
    pub async fn add_message(&self, message: &Message) -> Result<()>;
    pub async fn delete_message(&self, id: &str) -> Result<()>;

    // Parts
    pub async fn list_parts(&self, message_id: &str) -> Result<Vec<Part>>;
    pub async fn add_part(&self, part: &Part) -> Result<()>;

    // Todos
    pub async fn list_todos(&self, session_id: &str) -> Result<Vec<TodoItem>>;
    pub async fn add_todo(&self, todo: &TodoItem) -> Result<()>;
    pub async fn update_todo(&self, todo: &TodoItem) -> Result<()>;
    pub async fn delete_todo(&self, id: &str) -> Result<()>;
}
```

### checkpoint.rs - Session Checkpointing

```rust
pub struct CheckpointManager {
    store: SessionStore,
}

impl CheckpointManager {
    pub async fn create_checkpoint(&self, session_id: &str) -> Result<Checkpoint>;
    pub async fn restore_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()>;
    pub async fn list_checkpoints(&self, session_id: &str) -> Result<Vec<Checkpoint>>;
}
```

### import.rs - Session Import/Export

```rust
pub struct SessionImporter {
    store: SessionStore,
}

impl SessionImporter {
    pub async fn import_from_file(&self, path: &Path) -> Result<Session>;
    pub async fn export_to_file(&self, session_id: &str, path: &Path) -> Result<()>;
}
```

## Database Schema

See `schema.rs` for migration definitions:

```rust
pub mod schema {
    pub const CREATE_SESSIONS: &str = r#"
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            message_count INTEGER NOT NULL DEFAULT 0,
            model TEXT,
            tags TEXT NOT NULL DEFAULT '[]'
        )
    "#;

    pub const CREATE_MESSAGES: &str = r#"
        CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id)
        )
    "#;

    pub const CREATE_PARTS: &str = r#"
        CREATE TABLE parts (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            part_type TEXT NOT NULL,
            content TEXT NOT NULL,
            FOREIGN KEY (message_id) REFERENCES messages(id)
        )
    "#;
}
```

## Event Publishing

Session changes publish events to `GlobalEventBus`:

- `SessionCreated` - New session created
- `SessionSelected` - Session activated
- `SessionDeleted` - Session deleted
- `SessionRenamed` - Session renamed
- `MessageAdded` - New message in session

## Interactions

```
TUI
├── SessionStore::list_sessions()
├── SessionStore::get_session()
└── SessionStore::add_message()

AgentLoop
├── SessionStore::add_message()
├── SessionStore::list_messages() (for context)
└── CheckpointManager::create_checkpoint()
```

## Configuration

No specific configuration - uses database path from `storage::Database`.

## See Also

- [storage.md](storage.md) - Database initialization
- [tui.md](tui.md) - TUI that displays sessions
- [agent.md](agent.md) - AgentLoop that stores messages
