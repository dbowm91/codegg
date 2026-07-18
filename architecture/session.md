# Session Module Architecture

## Overview

The session module (`src/session/`) handles session storage, retrieval, and management for AI coding conversations. Sessions track conversation history, metadata, and analytics, enabling sessions to be archived, resumed, forked, and shared across CLI invocations.

```
src/session/
├── mod.rs          # Public exports, constants, JSON parsing helpers
├── schema.rs       # Database migrations (15 versions)
├── store.rs        # SessionStore, TodoStore, MessageStore, PartStore, PermissionStore, UsageStore
├── models.rs       # Session, CreateSession, UpdateSession, SessionAnalytics, UsageRecord, TodoItem
├── message.rs      # Message, MessageData, Part, PartInfo, PartData (Text/Reasoning/ToolCall/Image/File)
├── checkpoint.rs  # CheckpointStore, Checkpoint, WorkingFile, checksum utilities
├── import.rs      # SessionImport types, validate_import_size, redact_for_export
├── row.rs          # Database row types (SessionRow, MessageRow, PartRow, TodoRow, PermissionRow)
└── status.rs       # SessionStatus enum, SessionState struct
```

---

## Database Schema

### Tables

#### `project`
| Column | Type | Constraints |
|--------|------|-------------|
| id | TEXT | PRIMARY KEY |
| worktree | TEXT | NOT NULL |
| vcs | TEXT | |
| name | TEXT | |
| icon_url | TEXT | |
| icon_color | TEXT | |
| time_created | INTEGER | NOT NULL |
| time_updated | INTEGER | NOT NULL |
| time_initialized | INTEGER | |
| sandboxes | TEXT | NOT NULL |

#### `session`
| Column | Type | Constraints |
|--------|------|-------------|
| id | TEXT | PRIMARY KEY |
| project_id | TEXT | NOT NULL, FOREIGN KEY -> project(id) ON DELETE CASCADE |
| workspace_id | TEXT | |
| parent_id | TEXT | |
| slug | TEXT | NOT NULL |
| directory | TEXT | NOT NULL |
| title | TEXT | NOT NULL |
| version | TEXT | NOT NULL |
| share_url | TEXT | |
| summary_additions | INTEGER | |
| summary_deletions | INTEGER | |
| summary_files | INTEGER | |
| summary_diffs | TEXT | |
| revert | TEXT | |
| permission | TEXT | |
| tags | TEXT | |
| time_created | INTEGER | NOT NULL |
| time_updated | INTEGER | NOT NULL |
| time_compacting | INTEGER | |
| time_archived | INTEGER | |
| time_deleted | INTEGER | DEFAULT NULL |

### Typed identity compatibility

`codegg-core::identity::SessionBinding` is the in-memory relation for
`Session -> ProjectId + WorkspaceId`. Schema migration v25 adds the durable
`session_project_binding` table with typed-ID validation at hydration,
status/provenance, and an optimistic-concurrency revision. The `project_id`,
`workspace_id`, and `directory` columns remain string-backed compatibility
projections; path-valued `project_id` strings are never reinterpreted as
canonical IDs.

#### `message`
| Column | Type | Constraints |
|--------|------|-------------|
| id | TEXT | PRIMARY KEY |
| session_id | TEXT | NOT NULL, FOREIGN KEY -> session(id) ON DELETE CASCADE |
| time_created | INTEGER | NOT NULL |
| time_updated | INTEGER | NOT NULL |
| data | TEXT | NOT NULL (JSON serialized MessageData) |

#### `part`
| Column | Type | Constraints |
|--------|------|-------------|
| id | TEXT | PRIMARY KEY |
| message_id | TEXT | NOT NULL, FOREIGN KEY -> message(id) ON DELETE CASCADE |
| session_id | TEXT | NOT NULL |
| time_created | INTEGER | NOT NULL |
| time_updated | INTEGER | NOT NULL |
| data | TEXT | NOT NULL (JSON serialized PartData) |
| part_type | TEXT | GENERATED ALWAYS AS (json_extract(data, '$.type')) STORED |

#### `todo`
| Column | Type | Constraints |
|--------|------|-------------|
| session_id | TEXT | NOT NULL, FOREIGN KEY -> session(id) ON DELETE CASCADE |
| content | TEXT | NOT NULL |
| status | TEXT | NOT NULL |
| priority | TEXT | NOT NULL |
| position | INTEGER | NOT NULL, PRIMARY KEY (session_id, position) |
| time_created | INTEGER | NOT NULL |
| time_updated | INTEGER | NOT NULL |

#### `permission`
| Column | Type | Constraints |
|--------|------|-------------|
| project_id | TEXT | PRIMARY KEY, FOREIGN KEY -> project(id) ON DELETE CASCADE |
| time_created | INTEGER | NOT NULL |
| time_updated | INTEGER | NOT NULL |
| data | TEXT | NOT NULL (JSON) |

#### `session_share`
| Column | Type | Constraints |
|--------|------|-------------|
| session_id | TEXT | PRIMARY KEY, FOREIGN KEY -> session(id) ON DELETE CASCADE |
| id | TEXT | NOT NULL |
| secret | TEXT | NOT NULL |
| url | TEXT | NOT NULL |
| share_expires_at | INTEGER | |
| time_created | INTEGER | NOT NULL |
| time_updated | INTEGER | NOT NULL |

#### `cached_models`
| Column | Type | Constraints |
|--------|------|-------------|
| id | TEXT | PRIMARY KEY |
| provider | TEXT | NOT NULL |
| name | TEXT | NOT NULL |
| context_window | INTEGER | |
| max_output_tokens | INTEGER | |
| supports_tools | INTEGER | NOT NULL DEFAULT 1 |
| supports_vision | INTEGER | NOT NULL DEFAULT 0 |
| fetched_at | INTEGER | NOT NULL |

#### `task`
| Column | Type | Constraints |
|--------|------|-------------|
| id | INTEGER | PRIMARY KEY |
| parent_id | TEXT | |
| session_id | TEXT | NOT NULL |
| description | TEXT | NOT NULL |
| prompt | TEXT | NOT NULL |
| agent | TEXT | NOT NULL |
| status | TEXT | NOT NULL |
| result | TEXT | |
| denied_tools | TEXT | |
| allowed_paths | TEXT | |
| time_created | INTEGER | NOT NULL |
| time_updated | INTEGER | NOT NULL |

#### `checkpoints`
| Column | Type | Constraints |
|--------|------|-------------|
| id | TEXT | PRIMARY KEY |
| session_id | TEXT | NOT NULL, FOREIGN KEY -> session(id) ON DELETE CASCADE |
| timestamp | INTEGER | NOT NULL |
| state | TEXT | NOT NULL (JSON serialized Checkpoint) |

#### `snapshot`
| Column | Type | Constraints |
|--------|------|-------------|
| id | TEXT | PRIMARY KEY |
| session_id | TEXT | NOT NULL, FOREIGN KEY -> session(id) ON DELETE CASCADE |
| created_at | INTEGER | NOT NULL |
| label | TEXT | |
| data | TEXT | NOT NULL |

#### `usage`
| Column | Type | Constraints |
|--------|------|-------------|
| id | TEXT | PRIMARY KEY |
| session_id | TEXT | NOT NULL, FOREIGN KEY -> session(id) ON DELETE CASCADE |
| provider | TEXT | NOT NULL |
| model | TEXT | NOT NULL |
| input_tokens | INTEGER | NOT NULL |
| output_tokens | INTEGER | NOT NULL |
| cached_tokens | INTEGER | NOT NULL DEFAULT 0 |
| cost_usd | REAL | NOT NULL |
| timestamp | INTEGER | NOT NULL |

#### `migration_version`
| Column | Type | Constraints |
|--------|------|-------------|
| id | INTEGER | PRIMARY KEY CHECK (id = 1) |
| version | INTEGER | NOT NULL DEFAULT 0 |

### Indexes

| Index Name | Table | Columns |
|------------|-------|---------|
| session_project_idx | session | project_id |
| session_workspace_idx | session | workspace_id |
| session_parent_idx | session | parent_id |
| session_title_idx | session | title |
| session_slug_idx | session | slug |
| session_time_updated_idx | session | time_updated |
| session_tags_idx | session | tags |
| session_project_archived_idx | session | project_id, time_archived |
| idx_session_directory | session | directory |
| todo_session_idx | todo | session_id |
| message_session_time_created_id_idx | message | session_id, time_created, id |
| part_message_id_id_idx | part | message_id, id |
| part_session_idx | part | session_id |
| part_type_idx | part | part_type |
| task_session_idx | task | session_id |
| task_parent_idx | task | parent_id |
| checkpoint_session_idx | checkpoints | session_id |
| snapshot_session_idx | snapshot | session_id |
| usage_session_idx | usage | session_id |
| permission_time_idx | permission | time_created, time_updated |
| cached_models_provider_idx | cached_models | provider |

**Note**: Index names are defined by migrations. For example, `session_project_idx` is created in migration v1, `session_title_idx` and `session_slug_idx` in v2, etc.

---

## Migration System

The migration system resides in `src/session/schema.rs` and implements a sequential migration pattern with transaction support.

### Migration Flow

```
migrate() -> reads version -> for each version N not applied:
    migrate_and_record(N) -> calls migrate_vN() in transaction
                            -> updates migration_version table
```

1. `migrate()` checks current version from `migration_version` table
2. Iterates through versions 1..N sequentially, calling `migrate_and_record()` for each unapplied version
3. `migrate_and_record()` wraps each migration in a transaction:
   - BEGIN IMMEDIATE
   - Execute migration
   - Update migration_version
   - COMMIT on success, ROLLBACK on failure

### Migration Versions

| Version | Changes |
|---------|---------|
| **v1** | Creates `project`, `session`, `message`, `part`, `todo`, `permission`, `session_share` tables and initial indexes |
| **v2** | Adds `session_title_idx`, `session_slug_idx` |
| **v3** | Creates `cached_models` table |
| **v4** | Adds `session_time_updated_idx` |
| **v5** | Adds `share_expires_at` column to `session_share` |
| **v6** | Adds `permission_time_idx`, `session_project_archived_idx` |
| **v7** | Adds `tags` column to `session`, creates `session_tags_idx` |
| **v8** | Adds generated `part_type` column to `part`, creates `part_type_idx` |
| **v9** | Creates `task` table |
| **v10** | Creates `checkpoints` table |
| **v11** | Adds `idx_session_directory` index |
| **v12** | Adds `time_deleted` column to `session` (soft delete support) |
| **v13** | Creates `snapshot` table |
| **v14** | Adds `allowed_paths` column to `task` |
| **v15** | Creates `usage` table for token/cost tracking |
| **v16** | Creates `goal` table (goal lifecycle tracking) |
| **v17** | Creates `session_events` table (event journal) |
| **v18** | Creates `research_run` table (research artifact metadata) |
| **v19** | Creates `user_preferences` table (theme/model persistence) |
| **v20** | Creates `core_event_log` table (daemon core event sequence) |
| **v21** | Creates `notification_history` table (TUI notification backlog) |
| **v22** | Creates `workspace` table, adds `workspace_id` column to `session`, creates `idx_session_workspace_repair` index. Phase 2 single-daemon plan: workspace registry + execution context binding. Existing sessions are lazily resolved by canonicalizing their `directory` into a `workspace` record.

> **Phase 2 — Workspace Binding.** `project_id` and `directory` remain as
> compatibility fields. New daemon code MUST read `workspace_id` for
> workspace-scoped queries; unbound sessions (`workspace_id IS NULL`) are
> rejected at `TurnSubmit`/`AgentSelect`/`ModelSelect` until rebound via
> `CoreRequest::WorkspaceRegister`. See
> [`architecture/core.md`](core.md) for `ExecutionContext` semantics and
> `crates/codegg-core/src/workspace.rs`
> for the full contract.

---

## Data Models

### Session Struct (`src/session/models.rs:6-28`)

```rust
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

### Message Hierarchy

```
Message
├── id: String
├── session_id: String
├── time_created: i64
├── time_updated: i64
└── data: MessageData
    ├── id: String (default "")
    ├── session_id: String (default "")
    ├── message_id: String (default "")
    └── parts: Vec<PartInfo>
                ├── id: String
                ├── session_id: String
                ├── message_id: String
                └── data: PartData (enum)
```

### PartData Enum (`src/session/message.rs:36-59`)

```rust
pub enum PartData {
    Text {
        text: String,
    },
    Reasoning {
        reasoning: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
        output: Option<String>,
        status: ToolStatus,
    },
    Image {
        url: String,
    },
    File {
        path: String,
        content: String,
    },
}
```

**ToolStatus** (`src/session/message.rs:61-69`):
- `Pending` (default)
- `Running`
- `Completed`
- `Error`

### Part Struct (`src/session/message.rs:72-79`)

```rust
pub struct Part {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: serde_json::Value,  // RawPartData serialized
}
```

The `Part.data` field stores raw JSON (not parsed PartData) and is parsed lazily via `parse_json_field()`.

---

## Stores

### SessionStore (`src/session/store.rs:44-1548`)

Primary store for session CRUD operations.

**Key Methods:**
| Method | Description |
|--------|-------------|
| `create(CreateSession) -> Session` | Create new session with auto-generated slug |
| `get(&str) -> Option<Session>` | Get session by ID |
| `list(project_id, limit) -> Vec<Session>` | List active sessions (non-archived, non-deleted) |
| `list_with_offset(project_id, limit, offset) -> Vec<Session>` | Paginated session listing |
| `list_all(project_id, limit: Option<usize>) -> Vec<Session>` | List all non-deleted sessions |
| `list_all_with_offset(project_id, limit, offset) -> Vec<Session>` | Paginated listing of all non-deleted sessions |
| `list_deleted(project_id) -> Vec<Session>` | List soft-deleted sessions (time_deleted IS NOT NULL) |
| `create_from_template(template, project_id, directory) -> Session` | Create session from a SessionTemplate |
| `set_tags(id, tags) -> Session` | Set tags on a session |
| `session_count(project_id) -> usize` | Count active sessions |
| `message_count(session_id) -> usize` | Count messages in session |
| `message_counts(session_ids) -> HashMap` | Batch message count |
| `search(project_id, query) -> Vec<Session>` | Search by title/slug/directory |
| `search_all(project_id, query) -> Vec<Session>` | Search including message content |
| `find_by_tag(project_id, tag) -> Vec<Session>` | Filter sessions by tag |
| `all_tags(project_id) -> Vec<String>` | Get all tags with counts |
| `export_session(session_id) -> Value JSON` | Full session export (messages, parts, todos) |
| `import_session(data, new_project_id) -> Session` | Import from JSON |
| `update(id, UpdateSession) -> Session` | Update session fields |
| `delete(id)` | Hard delete |
| `soft_delete(id) -> Session` | Set time_deleted |
| `restore(id) -> Session` | Clear time_deleted |
| `fork(id) -> Session` | Create forked session with copy of messages/parts/todos |
| `archive(id) -> Session` | Set time_archived |
| `unarchive(id) -> Session` | Clear time_archived |
| `share_session(session_id) -> Session` | Generate share URL (7 days default) |
| `unshare_session(session_id) -> Session` | Remove share URL |
| `revert_to_message(session_id, message_id) -> Session` | Truncate to message_id, save revert state |
| `unrevert_session(session_id) -> Session` | Restore from revert state |
| `generate_summary(provider, session_id) -> Session` | LLM-generated summary |
| `generate_title(provider, session_id) -> Session` | LLM-generated title |
| `get_analytics(project_id) -> SessionAnalytics` | Aggregate statistics |
| `children(id) -> Vec<Session>` | Child sessions |

### TodoStore (`src/session/store.rs:1550-1753`)

**Key Methods:**
| Method | Description |
|--------|-------------|
| `list(session_id) -> Vec<TodoItem>` | List ordered by position |
| `set(session_id, items) -> Vec<TodoItem>` | Replace all todos |
| `add(session_id, item) -> TodoItem` | Append at end |
| `update(session_id, position, item) -> Vec<TodoItem>` | Update single item |
| `remove(session_id, position) -> Vec<TodoItem>` | Remove and reorder |
| `clear(session_id)` | Delete all todos |

### MessageStore (`src/session/store.rs:1755-1878`)

**Key Methods:**
| Method | Description |
|--------|-------------|
| `create(session_id, data) -> Message` | Insert with UUID |
| `get(session_id, id) -> Option<Message>` | Get by ID |
| `list(session_id) -> Vec<Message>` | Ordered by time_created, id |
| `count(session_id) -> usize` | Count messages |
| `update(session_id, id, data) -> Message` | Update data and time_updated |
| `delete(session_id, id)` | Delete message |

### PartStore (`src/session/store.rs:1880-1996`)

**Key Methods:**
| Method | Description |
|--------|-------------|
| `create(message_id, session_id, data) -> Part` | Insert with UUID |
| `get(id) -> Option<Part>` | Get by ID |
| `list_by_message(message_id) -> Vec<Part>` | Parts for a message |
| `list_by_session(session_id) -> Vec<Part>` | All parts for session |
| `update(id, data) -> Part` | Update data and time_updated |
| `delete(id)` | Delete part |

### PermissionStore (`src/session/store.rs:1998-2061`)

**Key Methods:**
| Method | Description |
|--------|-------------|
| `get(project_id) -> Option<PermissionEntry>` | Get by project |
| `upsert(project_id, data) -> PermissionEntry` | Insert or update |
| `delete(project_id)` | Delete permission |

### UsageStore (`src/session/store.rs:2063-2192`)

**Key Methods:**
| Method | Description |
|--------|-------------|
| `insert(record)` | Record usage |
| `get_session_usage(session_id) -> Vec<UsageRecord>` | All usage for session |
| `get_all_usage(limit) -> Vec<UsageRecord>` | All usage records |
| `get_session_cost_summary(session_id) -> (i64, i64, i64, f64)` | (input_tokens, output_tokens, cached_tokens, cost_usd) |

---

## Checkpoint Mechanism (`src/session/checkpoint.rs`)

### Checkpoint Struct

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

pub struct WorkingFile {
    pub path: String,
    pub checksum: String,       // SHA-256
    pub pre_state: Option<String>,
}
```

### CheckpointStore Methods

| Method | Description |
|--------|-------------|
| `save(checkpoint)` | Insert or replace |
| `load(id) -> Option<Checkpoint>` | Load by ID |
| `load_latest(session_id) -> Option<Checkpoint>` | Most recent checkpoint |
| `list(session_id) -> Vec<Checkpoint>` | All checkpoints, newest first |
| `delete(id)` | Delete single checkpoint |
| `delete_all(session_id)` | Delete all for session |
| `has_checkpoint(session_id) -> bool` | Check if any exist |

### Checksum Utilities

```rust
pub fn compute_checksum(content: &str) -> String  // SHA-256 hex
pub fn create_working_file(path: &str, pre_state: Option<String>) -> Option<WorkingFile>
pub fn verify_file(path: &str, expected_checksum: &str) -> bool
```

---

## Import/Export Flow

### Export (`SessionStore::export_session`)

1. Fetch session record
2. Fetch all messages ordered by time_created, id
3. Fetch all parts ordered by time_created, id
4. Fetch all todos ordered by position
5. Apply `redact_for_export()` to all message/part data
6. Return JSON: `{ session, messages, parts, todos }`

### Import (`SessionStore::import_session`)

1. `validate_import_size()` - check message/part count and total size limits
2. Deserialize to `SessionImport` type
3. Create transaction
4. Insert session with new UUID
5. Build ID mapping (old IDs -> new UUIDs)
6. Insert messages with remapped IDs
7. Insert parts with remapped message IDs
8. Insert todos with sequential positions
9. Commit transaction

### Size Limits (`src/session/import.rs:68-70`)

```rust
const MAX_IMPORT_MESSAGES: usize = 100_000;
const MAX_IMPORT_PARTS: usize = 500_000;
const MAX_TOTAL_IMPORT_BYTES: usize = 500 * 1024 * 1024;  // 500MB
```

### Redaction (`redact_for_export`)

For `tool_call` parts with sensitive tool names (`bash`, `write`, `read`, `edit`, `replace`, `multiedit`, `tail`, `git`, `webfetch`, `apply_patch`):
- Redact `input` field contents
- Redact `output` field if present
- Redact specific keys: `command`, `path`, `content`, `text`, `pattern`, `replacement`, `old_string`, `new_string`, `url`, `patch`

---

## Analytics and Usage Tracking

### SessionStore::get_analytics

Returns `SessionAnalytics`:
```rust
pub struct SessionAnalytics {
    pub total_sessions: u64,
    pub total_messages: u64,
    pub total_tool_calls: u64,          // Parts with part_type = 'tool_call'
    pub avg_session_duration_ms: u64,   // AVG(time_updated - time_created)
}
```

### UsageStore

Records per-request token usage and cost:
```rust
pub struct UsageRecord {
    pub id: String,
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cost_usd: f64,
    pub timestamp: i64,
}
```

---

## Session Status (`src/session/status.rs`)

### SessionStatus Enum

```rust
pub enum SessionStatus {
    Idle,        // Default
    Busy,
    Error,
    Compacting,
    Exporting,
}
```

### SessionState Struct

```rust
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

---

## Row Types (`src/session/row.rs`)

Database rows implement `sqlx::FromRow` and convert to domain models:

| Row Type | Domain Model | Conversion |
|----------|--------------|------------|
| SessionRow | Session | `From<SessionRow>` - parses tags, summary_diffs, revert, permission JSON |
| MessageRow | Message | `TryFrom<MessageRow>` - parses data JSON |
| PartRow | Part | `From<PartRow>` - uses `parse_json_field()` for error-tolerant parsing |
| TodoRow | TodoItem | `From<TodoRow>` |
| PermissionRow | PermissionEntry | `From<PermissionRow>` |

---

## Key Implementation Notes

1. **ID Generation**: Uses `uuid::Uuid::new_v4().to_string()` for all entity IDs

2. **Slug Generation** (`generate_slug`): Lowercase, alphanumeric only, spaces -> hyphens

3. **Timestamps**: All stored as Unix milliseconds (`Utc::now().timestamp_millis()`)

4. **Foreign Key Behavior**: `ON DELETE CASCADE` for session-related tables

5. **Soft Delete**: Sessions use `time_deleted` column; queries filter by `IS NULL`

6. **Partial Updates**: Uses `COALESCE(?, field_name)` pattern to allow selective updates

7. **Fork Operations**: Creates full copy of messages/parts/todos with new IDs, preserves parent_id reference

8. **Revert State**: Stores removed messages/parts as JSON in `session.revert` field for potential restoration

9. **Checkpoints**: Store complete session state including provider/model info and working file checksums

10. **Message/Part Ordering**: All queries use `ORDER BY time_created ASC, id ASC` for deterministic ordering
## Canonical session binding

Executable sessions require a resolved project/workspace binding. Creation and
template creation use the atomic `SessionStore::create_with_binding` path;
forks copy the canonical binding inside their transaction. Existing rows remain
loadable, but a legacy directory can become executable only when it resolves to
one existing active binding. Otherwise the daemon returns a bounded
`project_context_required`-class diagnostic.
