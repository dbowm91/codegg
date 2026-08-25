# Session Module Architecture

## Purpose

The session module handles SQLite-backed persistence for AI coding
conversations: creation, retrieval, search, fork, import/export,
analytics, checkpoints, and event journaling. It also provides
provider-connection lifecycle selection, legacy model-string resolution,
and a derived TUI session state reconstructed from typed events.

## Where It Lives

All source is in `crates/codegg-core/src/session/`:

```
session/
├── mod.rs               # Public re-exports, constants, JSON helpers
├── schema.rs            # Database migrations (v1–v35), all in sqlx
├── store.rs             # SessionStore, TodoStore, MessageStore,
│                        # PartStore, PermissionStore, UsageStore,
│                        # EventStore
├── models.rs            # Session, CreateSession, UpdateSession,
│                        # SessionAnalytics, UsageRecord, TodoItem,
│                        # PermissionEntry, LegacyResolution
├── message.rs           # Message, MessageData, Part, PartInfo,
│                        # PartData, ToolStatus
├── checkpoint.rs        # CheckpointStore, Checkpoint, WorkingFile,
│                        # SHA-256 checksum utilities
├── import.rs            # SessionImport types, validate_import_size,
│                        # redact_for_export
├── row.rs               # SessionRow, MessageRow, PartRow, TodoRow,
│                        # PermissionRow — sqlx FromRow conversions
├── status.rs            # SessionStatus enum, SessionState struct
├── events.rs            # 20 typed SessionEvent variants, EventMeta,
│                        # ToolRisk, ToolCallStatus, PlanItemStatus
├── state.rs             # TuiSessionState — derived from events
├── selection_catalog.rs # Read-only model/health catalog helpers
└── legacy_resolution.rs # Legacy provider/model string resolver
```

## How It Works

### Database Schema

The schema is managed by `schema.rs` which contains **35 sequential
migrations** (v1–v35), each wrapped in an explicit `BEGIN IMMEDIATE`
transaction. The `migration_version` table tracks the current version.
On startup, `migrate()` checks the version and runs all unapplied
migrations in order.

Migrations create and evolve **52+ CREATE TABLE statements** across
these table groups:

**Core session tables (v1):**
`project`, `session`, `message`, `part`, `todo`, `permission`,
`session_share`

**Supporting tables (v3–v15):**
`cached_models` (v3), `task` (v9), `checkpoints` (v10),
`snapshot` (v13), `usage` (v15)

**Goal and event tables (v16–v21):**
`goal` (v16), `session_events` (v17), `research_run` (v18),
`user_preferences` (v19), `core_event_log` (v20),
`notification_history` (v21)

**Workspace and identity (v22, v25):**
`workspace` (v22), `logical_project`, `repository`,
`project_repository`, `workspace_project_binding`,
`session_project_binding`, `identity_diagnostic` (v25)

**Provider connections (v24, v26, v27, v31):**
`provider_connections` (v24), `provider_provisioning`,
`provider_connection_health`, `provider_connection_models` (v26),
session selection columns (v27), lifecycle/reference/tombstone/audit
tables (v31)

**Durable jobs (v23):**
`job`, `job_attempt`, `job_dependency`, `schedule`,
`schedule_occurrence`

**Tool Programs (v33–v35):**
`tool_program`, `tool_program_call` (v33),
`tool_program_notification` (v34), nullable lineage columns on `job`
(v35)

**Projections (v32):**
`projection_stream`, `projection_event`, `projection_checkpoint`

**Discovery (v28–v29):**
`project_locator`, `project_health`,
`legacy_catalog_association_marker` (v28), `discovery_root`,
`discovery_scan`, `discovery_observation` (v29)

**Runtime assets (v30):**
`runtime_asset_refresh`

### Column additions to session table

The `session` table gains columns across multiple migrations:
v12 adds `time_deleted`, v22 adds `workspace_id`, v27 adds
`provider_connection_id`, `provider_connection_revision`,
`model_catalog_revision`, `selected_model_id`, `agent`, `model`.

### Session Columns constant

The `SESSION_COLUMNS` constant (`session/mod.rs:39`) includes all 22
session columns including the v27 provider connection selection fields.
Qualified queries use `SESSION_COLUMNS_QUALIFIED` (`session/mod.rs:46`).

### Session Lifecycle

1. **Creation**: `SessionStore::create()` generates a UUID, creates
   the project row if missing, inserts the session with slug.
2. **Canonical binding**: `create_with_binding()` atomically creates
   a session with its `session_project_binding` row in one transaction.
3. **Fork**: Copies messages/parts/todos with new IDs, preserves
   parent_id and workspace binding.
4. **Revert**: Truncates messages after a pivot point, saves removed
   messages/parts as JSON in `session.revert`.
5. **Soft delete**: Sets `time_deleted`; queries filter `IS NULL`.
6. **Archive**: Sets `time_archived`; `list()` excludes archived;
   `list_all()` includes them.
7. **Share**: Generates a 7-day share URL (configurable via
   `CODEGG_SHARE_DURATION_DAYS`) with a random token.

### Event System

`EventStore` (`session/store.rs:2598`) persists typed `SessionEvent`
variants into the `session_events` table. Events are:
- `GoalSet`, `PlanUpdated`, `PlanItemUpdated`
- `AgentMessage`, `UserMessage`
- `ToolCallStarted`, `ToolCallFinished`
- `ToolProgramNotification` (with semantic equality for crash recovery)
- `PermissionRequested`, `PermissionResolved`
- `FileChanged`, `TestRunStarted`, `TestRunFinished`
- `ContextCompacted`, `ModelRouted`
- `SubagentStarted`, `SubagentFinished`
- `FindingRaised`, `CheckpointCreated`, `SessionExported`

`EventStore::append_idempotent()` provides exactly-once semantics.
ToolProgramNotification events use `semantic_equals()` which ignores
`meta.created_at` (stamped on crash recovery) while requiring all
identity and content fields to match.

### TUI Session State

`TuiSessionState` (`session/state.rs:111`) is a fully derived
in-memory representation reconstructed from events via
`from_events()`. It tracks: goal, plan, active/recent tool calls
(capped at 50), changed files, test state, context state, model state,
findings, and subagent summaries.

`TestState` transitions to `Stale` when a `FileChanged` event arrives
after a `Passed` state (but not after `Failed`).

### Provider Connection Lifecycle

`SessionLifecycleGet` provides the current state, health timestamp,
selected model, and bounded removed-model list. Disable, tombstone,
missing credentials, stale health, and removed models are surfaced as
lifecycle diagnostics and never trigger fallback to another connection.

### Legacy Resolution

`legacy_resolution.rs` resolves a legacy `provider/model` string
against the durable connection catalog. It is deterministic,
non-fallbacking, and read-only. Outcomes:
- `Unset` — empty string
- `Resolved` — single active match
- `UnresolvedLegacyProvider` — no match
- `AmbiguousLegacyProvider` — multiple matches
- `DisabledLegacyConnection` — match is disabled/tombstoned/error
- `MissingCredentialLegacyConnection` — match lacks credential

## Key Types & APIs

### Session (`session/models.rs:6`)

```rust
pub struct Session {
    pub id: String,
    pub project_id: String,          // legacy string projection
    pub workspace_id: Option<String>,
    pub parent_id: Option<String>,
    pub slug: String,
    pub directory: String,           // filesystem locator, not a project ID
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
    // Provider Connections Milestone 3 fields:
    pub provider_connection_id: Option<String>,
    pub provider_connection_revision: Option<u64>,
    pub model_catalog_revision: Option<String>,
    pub selected_model_id: Option<String>,
    pub agent: Option<String>,        // legacy, not authoritative
    pub model: Option<String>,        // legacy "provider/model"
    pub time_created: i64,
    pub time_updated: i64,
    pub time_compacting: Option<i64>,
    pub time_archived: Option<i64>,
    pub time_deleted: Option<i64>,
}
```

### SessionStore (`session/store.rs:48`)

| Method | Line | Description |
|--------|------|-------------|
| `create(CreateSession)` | 61 | Create with auto UUID |
| `create_with_id(id, input)` | 68 | Stable ID for imports |
| `create_with_binding(input, pid, wid, src)` | 164 | Atomic session + binding |
| `create_from_template(template, pid, dir)` | 303 | From config template |
| `get(id)` | 326 | Get by ID (excludes soft-deleted) |
| `list(pid, limit)` | 338 | Active sessions, paginated |
| `list_with_offset(pid, limit, offset)` | 371 | Offset-based pagination |
| `list_all(pid, limit)` | 437 | Includes archived, excludes deleted |
| `list_all_with_offset(pid, limit, offset)` | 469 | Offset-based for all |
| `list_by_canonical_project(pid, limit)` | 345 | Via session_project_binding |
| `list_all_sessions(limit)` | 448 | Cross-project (import tooling) |
| `list_deleted(pid)` | 1067 | Soft-deleted sessions |
| `search(pid, query)` | 502 | LIKE title/slug/directory |
| `search_all(pid, query)` | 523 | Also searches message content |
| `find_by_tag(pid, tag)` | 545 | JSON tag matching |
| `all_tags(pid)` | 564 | Tags with counts |
| `session_count(pid)` | 390 | Active session count |
| `message_count(sid)` | 401 | Messages in session |
| `message_counts(sids)` | 410 | Batch message counts |
| `export_session(sid)` | 587 | Full JSON export |
| `import_session(data, new_pid)` | 668 | Import with ID remapping |
| `import_session_with_binding(data, pid, wid, src)` | 678 | Import + binding |
| `update(id, UpdateSession)` | 949 | Partial update (COALESCE) |
| `delete(id)` | 1031 | Soft delete |
| `soft_delete(id)` | 1036 | Set time_deleted |
| `restore(id)` | 1052 | Clear time_deleted |
| `set_tags(id, tags)` | 1079 | Replace tags |
| `fork(id)` | 1097 | Copy with new IDs |
| `archive(id)` | 1315 | Set time_archived |
| `unarchive(id)` | 1331 | Clear time_archived |
| `share_session(sid)` | 1627 | Generate share URL (7-day) |
| `unshare_session(sid)` | 1699 | Remove share |
| `revert_to_message(sid, mid)` | 1374 | Truncate + save revert |
| `unrevert_session(sid)` | 1730 | Restore from revert state |
| `generate_summary(provider, sid)` | 1525 | LLM summary |
| `generate_title(provider, sid)` | 1561 | LLM title |
| `get_analytics(pid)` | 1862 | Aggregate statistics |
| `children(id)` | 1362 | Child sessions |
| `set_share_url(id, url)` | 1346 | Direct share URL set |

### TodoStore (`session/store.rs:1917`)

Methods: `list`, `set`, `add`, `update`, `remove`, `clear`

### MessageStore (`session/store.rs:2122`)

Methods: `create`, `create_with_id`, `get`, `list`, `count`,
`update`, `delete`

### PartStore (`session/store.rs:2258`)

Methods: `create`, `get`, `list_by_message`, `list_by_session`,
`update`, `delete`

### PermissionStore (`session/store.rs:2376`)

Methods: `get`, `upsert`, `delete`

### UsageStore (`session/store.rs:2441`)

Methods: `insert`, `get_session_usage`, `get_all_usage`,
`get_session_cost_summary`

### EventStore (`session/store.rs:2598`)

Methods: `append`, `append_idempotent`, `list_for_session`,
`has_event`, `confirm_existing`

### CheckpointStore (`session/checkpoint.rs:48`)

Methods: `save`, `load`, `load_latest`, `list`, `delete`,
`delete_all`, `has_checkpoint`

### Data Models

**MessageData** (`session/message.rs:13`):
```rust
pub struct MessageData {
    pub id: String,
    pub session_id: String,
    pub message_id: String,     // renamed "messageID"
    pub parts: Vec<PartInfo>,
}
```

**PartData** (`session/message.rs:38`):
`Text`, `Reasoning`, `ToolCall`, `Image`, `File`

**ToolStatus** (`session/message.rs:63`):
`Pending` (default), `Running`, `Completed`, `Error`

**SessionAnalytics** (`session/models.rs:159`):
```rust
pub struct SessionAnalytics {
    pub total_sessions: u64,
    pub total_messages: u64,
    pub total_tool_calls: u64,
    pub avg_session_duration_ms: u64,
}
```

**UsageRecord** (`session/models.rs:193`):
`id`, `session_id`, `provider`, `model`, `input_tokens`,
`output_tokens`, `cached_tokens`, `cost_usd`, `timestamp`

## Configuration Surface

- `CODEGG_SHARE_DURATION_DAYS` env var overrides the 7-day share
  expiry (parsed at `session/store.rs:1629`)
- Import size limits (`session/import.rs:68-70`):
  `MAX_IMPORT_MESSAGES = 100_000`, `MAX_IMPORT_PARTS = 500_000`,
  `MAX_TOTAL_IMPORT_BYTES = 500 MB`

## Invariants & Gotchas

1. **ID generation**: `uuid::Uuid::new_v4()` for all entities.
   Timestamps are Unix milliseconds (`Utc::now().timestamp_millis()`).
2. **Foreign keys**: `ON DELETE CASCADE` for session-related tables.
3. **Soft delete**: Queries filter `time_deleted IS NULL`. The `delete()`
   method is an alias for `soft_delete()`, not a hard delete.
4. **Partial updates**: `COALESCE(?, field)` pattern allows selective
   updates; passing `None` leaves the field unchanged.
5. **Fork copies with redaction**: Forked messages/parts are run through
   `redact_for_export()` before insertion — sensitive tool_call data
   is stripped.
6. **Revert state**: Removed messages/parts are stored as JSON in
   `session.revert` for potential restoration via `unrevert_session()`.
7. **Canonical binding required**: Executable sessions need a resolved
   project/workspace binding via `session_project_binding`. Legacy
   directory strings become executable only when they resolve to one
   existing active binding.
8. **ToolProgramNotification semantic equality**: `meta.created_at` is
   ignored during conflict reconciliation because crash recovery stamps
   a fresh timestamp. All other fields must match exactly.
9. **Test state staleness**: `TestState::Passed` transitions to `Stale`
   on any `FileChanged` event. `TestState::Failed` does not go stale.
10. **Message/Part ordering**: All queries use `ORDER BY time_created
    ASC, id ASC` for deterministic ordering.
11. **Slug generation**: Lowercase, alphanumeric only, spaces → hyphens,
    fallback "untitled".
12. **Redact tool names**: `bash`, `write`, `read`, `edit`, `replace`,
    `multiedit`, `terminal`, `git`, `webfetch`, `apply_patch` — input
    and output are redacted on export. Specific keys (`command`, `path`,
    `content`, etc.) are individually redacted within the input object.
    Note: the code uses `terminal`, not `tail` as some docs claim.

## Testing

Narrowest test targets:
```bash
cargo test -p codegg-core --lib session          # session unit tests
cargo test -p codegg-core --lib session::message  # message model tests
cargo test -p codegg-core --lib session::events   # event tests
cargo test -p codegg-core --lib session::state    # TUI state tests
cargo test -p codegg-core --test session_crud     # integration CRUD
cargo test -p codegg-core --lib session::legacy_resolution
cargo test -p codegg-core --lib session::store::event_store_idempotency_tests
```

## Related Docs

- [storage.md](storage.md) — Database initialization, connection
  pooling, `DaemonPaths`
- [workspace_services.md](workspace_services.md) — Workspace-local
  run store, catalog migration
- [core.md](core.md) — `ExecutionContext`, workspace binding semantics
- `crates/codegg-core/src/identity.rs` — Typed project/session
  bindings
- `crates/codegg-core/src/workspace.rs` — Workspace registry
