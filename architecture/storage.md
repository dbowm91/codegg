# Storage Module

## Purpose

The `storage` module provides SQLite database initialization, connection
pooling, WAL configuration, and the platform-resolved path layout for
the user-scoped daemon catalog. It also hosts the `UserPreferences`
key/value store backed by `user_preferences` table.

## Where It Lives

```
crates/codegg-core/src/storage/
├── mod.rs          # Database wrapper, init functions, pragmas,
│                   # STORAGE_LAYOUT_VERSION, deprecated init()
├── paths.rs        # DaemonPaths — single source of truth for
│                   # catalog and asset paths
└── preferences.rs  # UserPreferences — persistent key/value store
```

## How It Works

### Database Initialization

There are **six** entry points for getting a `SqlitePool`:

| Function | Path | Purpose |
|----------|------|---------|
| `init_daemon_catalog(paths)` | `catalog_db_path()` | User-scoped daemon catalog, no migrations |
| `init_migrated_daemon_catalog(paths)` | Same as above | Runs migrations on a single-connection bootstrap pool, closes it, then opens the normal catalog pool |
| `init_legacy_project_store(root)` | `<root>/.codegg/sessions.db` | Legacy project-local store for backward compat |
| `init_pool_at(db_path)` | Caller-supplied path | Generic pool at an arbitrary path |
| `init_pool_at_for_migration(db_path)` | Caller-supplied path | Single-connection pool reserved for migrations |
| `init(project_dir)` (deprecated) | Empty → config dir; non-empty → legacy | Retained for tests; new code MUST NOT use |

`init_migrated_daemon_catalog` is the **production daemon bootstrap
authority**. It creates a single-connection pool for migration, runs
all schema migrations, closes the migration pool, then opens the normal
10-connection pool via `init_daemon_catalog`.

### Path Layout (`DaemonPaths`)

`DaemonPaths` (`storage/paths.rs:20`) is the single source of truth.
Defaults follow OS conventions with fallbacks:

| Platform | Data root | Config root |
|----------|-----------|-------------|
| macOS | `~/Library/Application Support/codegg/` | `~/Library/Application Support/codegg/` |
| Linux | `$XDG_DATA_HOME/codegg/` or `~/.local/share/codegg/` | `$XDG_CONFIG_HOME/codegg/` or `~/.config/codegg/` |
| Fallback | `~/.codegg/` | `~/.config/codegg/` |

Override via `CODEGG_DATA_HOME` env var (data root only).

```rust
pub struct DaemonPaths {
    pub data_root: Option<PathBuf>,   // override or platform default
    pub config_root: Option<PathBuf>, // override or platform default
}
```

Public constructors/accessors: `with_overrides`
(`storage/paths.rs:32`), `default_data_root()` (`:40`),
`default_config_root()` (`:51`), `data_root()` (`:59`),
`config_root()` (`:66`), `catalog_db_path()` (`:73`),
`catalog_db_wal_path()` (`:78`), `agents_dir()` (`:88`),
`credentials_path()` (`:94`), `workspace_local_artifact_root()` (`:101`).
See `crates/codegg-core/src/storage/paths.rs` for the full listing.

Key derived paths:
- `catalog_db_path()` → `<data_root>/codegg.db`
- `catalog_db_wal_path()` → `<data_root>/codegg.db-wal`
- `agents_dir()` → `<config_root>/agents/`
- `credentials_path()` → `<config_root>/credentials.json`
- `workspace_local_artifact_root(ws)` → `<ws>/.codegg/`

### SQLite Configuration

Applied in `connect_and_configure()` (`storage/mod.rs:214`) as a
single batched query:

| Pragma | Value | Purpose |
|--------|-------|---------|
| `journal_mode` | `WAL` | Write-Ahead Logging for concurrency |
| `wal_autocheckpoint` | `1000` | Checkpoint every 1000 pages |
| `busy_timeout` | `30000` ms (builder) / `5000` (post-connect) | 30s builder timeout applied to every pooled connection; the post-connect batch re-asserts a 5s value on the opening connection |
| `synchronous` | `NORMAL` | Balanced performance/safety |
| `mmap_size` | `268435456` | 256MB memory-mapped I/O |
| `cache_size` | `-2000` | 2MB page cache |
| `temp_store` | `MEMORY` | Temp tables in RAM |
| `foreign_keys` | `ON` | FK enforcement |

> Per-connection pragmas must ride `SqliteConnectOptions`, not only a
> post-connect query: a `PRAGMA busy_timeout` executed through the pool
> touches exactly one pooled connection, leaving the rest at
> `busy_timeout=0` (instant `SQLITE_BUSY` on transient contention —
> observed as silently dropped projection publications under concurrent
> turn + subscription load in M004 E2E). The builder sets
> `busy_timeout(30s)`, `foreign_keys(true)`, `journal_mode(WAL)`, and
> `synchronous(NORMAL)` for every connection the pool opens; the
> post-connect batch then applies the remaining pragmas.

### Connection Pool

`connect_and_configure()` creates the pool via `SqlitePoolOptions`:
- Normal pools: `max_connections(10)`, `acquire_timeout(30s)`
- Migration pools: `max_connections(1)`

### Database Wrapper

```rust
pub struct Database { pool: SqlitePool }
```

Methods:
- `new(path)` — Open + migrate + WAL checkpoint + background integrity
  check (5s delay)
- `pool()` — Borrow the underlying `SqlitePool`
- `migrate()` — Re-run schema migrations (idempotent)
- `health_check()` — `SELECT 1`
- `close()` — WAL checkpoint + pool shutdown

### WAL Checkpoint and Integrity

`Database::new()` triggers:
1. `try_checkpoint_wal()` — non-fatal WAL checkpoint
2. `spawn_background_integrity_check()` — `PRAGMA quick_check` after
   5s delay

### STORAGE_LAYOUT_VERSION

The current layout version is **68**, defined by
`storage::STORAGE_LAYOUT_VERSION` (`storage/mod.rs:39`), and must track the
highest migration wired into the
canonical schema path in `session/schema.rs` (see
`scripts/check_project_catalog_invariants.py` and the
`tests/storage_migrations.rs` equality assertion). It is exported and
referenced from `MigrationMarker.storage_layout_version` for the migration
tooling that imports legacy project databases. Migration 37 adds canonical
`agent_task` and `agent_run` tables with typed string IDs, session/root/parent/
workspace/status indexes, unique delegation identity, scheduler job/attempt
links, bounded terminal references, and versioned budget JSON. Migration 46
adds the `edit_checkpoint` table for durable per-batch pre/post file states
scoped to workspace/session/turn/batch, reusing snapshot size/symlink bounds.
Migration 47 adds the `edit_restore_operation` audit table for checked
Undo/Reapply lineage, conflict/partial evidence, and restart durability.
Migration 49 adds `agent_convergence` and `agent_convergence_cycle` for the
M001 durable convergence foundation. The tables retain the exact bounded
objective/criteria specification and owner identity, plus cycle references
to existing agent groups/runs, verifier verdicts, and owner decisions. They
do not duplicate run results or store transcripts/tool output. The SQLite
store uses revision-checked lifecycle transitions and unique idempotency
keys; restart reconciliation remains a pure classifier in the core domain.
The legacy `task` table remains readable as compatibility history; it is not
backfilled with unverifiable durable-run provenance.

The durable run records intentionally retain summaries, digests, references,
and failure classifications rather than full prompts, hidden reasoning,
credentials, permission bodies, or unbounded tool output. Scheduler recovery
reconciles terminal/non-replayable delegated jobs to `Interrupted`,
`Cancelled`, or the already-recorded terminal run outcome.

## Key Types & APIs

### Database (`storage/mod.rs:41`, `impl` at `storage/mod.rs:45`)

```rust
impl Database {
    pub async fn new(path: &str) -> Result<Self, StorageError>;
    pub fn pool(&self) -> &SqlitePool;
    pub async fn migrate(&self) -> Result<(), StorageError>;
    pub async fn health_check(&self) -> Result<(), StorageError>;
    pub async fn close(self);
}
```

### DaemonPaths (`storage/paths.rs:20`)

```rust
impl DaemonPaths {
    pub fn default() -> Self;
    pub fn with_overrides(data_root, config_root) -> Self;
    pub fn data_root(&self) -> PathBuf;
    pub fn config_root(&self) -> PathBuf;
    pub fn catalog_db_path(&self) -> PathBuf;
    pub fn catalog_db_wal_path(&self) -> PathBuf;
    pub fn agents_dir(&self) -> PathBuf;
    pub fn credentials_path(&self) -> PathBuf;
    pub fn workspace_local_artifact_root(&self, ws: &Path) -> PathBuf;
}
```

### UserPreferences (`storage/preferences.rs:25`)

```rust
impl UserPreferences {
    pub fn new(pool: SqlitePool) -> Self;
    pub async fn get(&self, key: &str) -> Result<Option<String>>;
    pub async fn set(&self, key: &str, value: &str) -> Result<()>;
    pub async fn delete(&self, key: &str) -> Result<u64>;
    pub async fn updated_at(&self, key: &str) -> Result<Option<i64>>;
}
```

Known keys:
- `KEY_THEME_ACTIVE` = `"theme.active"` — active theme id
- `KEY_MODEL_LAST_USED` = `"model.last_used"` — last-used model id

### Init Functions (`storage/mod.rs`)

```rust
pub async fn init_daemon_catalog(paths: &DaemonPaths)
    -> Result<SqlitePool, StorageError>;
pub async fn init_migrated_daemon_catalog(paths: &DaemonPaths)
    -> Result<SqlitePool, StorageError>;
pub async fn init_legacy_project_store(project_root: &Path)
    -> Result<SqlitePool, StorageError>;
pub async fn init_pool_at(db_path: &Path)
    -> Result<SqlitePool, StorageError>;
pub async fn init_pool_at_for_migration(db_path: &Path)
    -> Result<SqlitePool, StorageError>;
#[deprecated] pub async fn init(project_dir: &str)
    -> Result<SqlitePool, StorageError>;
```

## Configuration Surface

- `CODEGG_DATA_HOME` env var overrides the default data root in
  `DaemonPaths::default_data_root()` (`storage/paths.rs:40`, env read at
  `storage/paths.rs:41`)

## Invariants & Gotchas

1. **Single daemon invariant**: The catalog database is user-scoped,
   not project-scoped. Exactly one daemon owns it per OS user.
2. **Migration pool isolation**: `init_migrated_daemon_catalog` uses a
   single-connection pool for migration, closes it, then opens the
   normal pool. This prevents self-deadlocking during bootstrap.
3. **init() is deprecated**: `init(project_dir)` routes to either the
   user config directory or a legacy project store. New code MUST NOT
   use it.
4. **init_daemon_catalog vs init_migrated_daemon_catalog**: The former
   returns a pool without running migrations. The latter is the
   production bootstrap path.
5. **init_pool_at creates directories**: It calls `create_dir_all` and
   checks for read-only directories before connecting.
6. **Integrity check is non-fatal**: `spawn_background_integrity_check`
   logs warnings but does not fail startup.
7. **Pool max connections is 10**: Hardcoded in `connect_and_configure`.
   Migration pools use `max_connections(1)`.
8. **Catalog DB vs workspace-local artifacts**: The catalog owns
   sessions, jobs, notification history. Workspace-local artifacts
   (run store data) live under `<workspace>/.codegg/runs/`.

## Migrations (Storage Layout Context)

Migrations are implemented in `session/schema.rs`, not in the storage
module. The storage module calls `session::schema::migrate()` during
initialization. `session/schema.rs` contains 73 `CREATE TABLE`
statements; migrations v1–v68 are all present as `migrate_v<N>` functions
(declaration order in the file is not numeric — `migrate_v67`,
`migrate_v34`, `migrate_v33` lead, and `migrate_v50`/`v47`/`v46`/`v32` are
defined at the end).

Several late migrations delegate their statements to a module-owned
schema constant rather than inlining SQL: v55 →
`collaboration::CHAT_SCHEMA_STATEMENTS`, v58 →
`approval::RUNTIME_PREFERENCE_SCHEMA_STATEMENTS`, v59 →
`work_plan::WORK_PLAN_SCHEMA_STATEMENTS`, v65 →
`session_control::SESSION_CONTROL_SCHEMA_STATEMENTS`, and v68 →
`work_plan::WORK_PLAN_REPOSITORY_BINDING_SCHEMA_STATEMENTS`.

Key storage-layout migrations:
- **v22**: Workspace table, `session.workspace_id` column — Phase 2
  workspace registry
- **v23**: Durable jobs tables — Phase 4 job orchestration
- **v24**: `provider_connections` — daemon-owned connection metadata
- **v25**: Canonical project/repository authority — additive identity
  tables
- **v26**: Provider provisioning, health, model catalog
- **v27**: Session selection columns (connection ID, revision, model)
- **v28**: Project catalog tables (locators, health, legacy markers)
- **v29**: Discovery roots, scans, observations
- **v30**: Runtime asset refresh provenance
- **v31**: Provider lifecycle, reference, tombstone, audit
- **v32**: Projection streams, events, checkpoints
- **v33–v34**: Tool Program domain, notification claims
- **v35**: Nullable typed lineage columns for child jobs
- **v36**: Durable per-job execution timeouts (`job.timeout_ms`)
- **v37**: Canonical `agent_task` + `agent_run` tables with typed string IDs, session/root/parent/workspace/status indexes, unique delegation identity, scheduler job/attempt links, bounded terminal references, versioned budget JSON
- **v38**: `agent_run_mailbox`, `agent_run_journal` — inter-run mail and ordered journal
- **v39**: `managed_worktree`, `worktree_lease` — daemon-owned worktree records and leases
- **v40**: `agent_run_result` — durable run result records
- **v41**: `agent_run_group`, `agent_run_group_member` — run grouping
- **v42**: `agent_run.depth` (bounded 1..64) for nesting
- **v43**: `agent_run_group` owner attribution (`owner_kind`, `owner_session_id`, `owner_turn_id`)
- **v44**: `agent_task.request_fingerprint` for delegation idempotency
- **v45**: `goal.revision` for CAS goal updates
- **v46**: `edit_checkpoint` for mutation attribution (pre/post Absent/Present, workspace/session/turn/batch scoped)
- **v47**: `edit_restore_operation` for checked Undo/Reapply audit (applied/conflict/partial, durable lineage, bounded paths)
- **v48**: session/event/part lookup indexes (`idx_session_project_updated`, `idx_session_events_session_created`, `part_session_idx`)
- **v49**: `agent_convergence`, `agent_convergence_cycle` — M001 durable convergence foundation
- **v50**: job/attempt and schedule lookup indexes (`idx_job_attempt_run_id`, `idx_schedule_occurrence_status`)
- **v51**: team domain — `principal`, `project_membership` (M001 durable principals/memberships)
- **v52**: `personal_auth_token` digests for team authentication (M002, never plaintext)
- **v53**: `origin_attribution` for immutable originating-principal capture (M003)
- **v54**: append-only `audit_event` + separate `audit_body` retention split (M004)
- **v55**: project chat — `chat_channel`, `chat_message` (live retention window, per-channel `seq`, idempotency keys), append-only `chat_revision` history (survives retention pruning), `chat_read_marker` (collaboration M001; composing stays ephemeral in memory)
- **v56**: structured chat actions — `chat_action` reference/status projection (`action_id`, channel/message/project locators, actor, kind, title, job, status, `(channel, idempotency_key)` unique retry backstop; collaboration M003, jobs stay canonical in scheduler/job stores)
- **v57**: durable continuation checkpoints — `continuation_checkpoint` candidates (`prepared | installed | aborted`, per-session sequence with `UNIQUE(session_id, sequence)`, explicit installed-parent lineage, SHA-256 payload digest, 128 KiB payload bound, latest/installed/lineage indexes; context-continuity M001, atomic install + `ContextCompacted` commit marker)
- **v58**: daemon-owned principal runtime preferences — approval mode, sandbox profile, reserved provider/model identity (execution-reliability M003; additive, secret-free)
- **v59**: durable revisioned WorkPlan/WorkItem foundation — `work_plan`, `work_item` with CAS revision, bounded objective/description/evidence JSON (long-horizon M002; no Goal/Todo/session backfill)
- **v60**: project work orders — `work_order` (revisioned intent, bounded prompt/gates/repeat, `(project, submission_key)` idempotency, spec digests), `work_order_occurrence` (`UNIQUE(work_order_id, occurrence_index)`, explicit 0-based indexing), `work_order_batch` retry ledger, `sequence_lane` + normalized `sequence_lane_member` ordering (project Work Orders M001; empty by default, no schedule backfill, no session rows)
- **v61**: `origin_attribution` scope rebuild admitting `work_order` — row-preserving table rebuild extending the v53 scope-kind `CHECK`; legacy attribution rows survive verbatim (project Work Orders M001)
- **v62**: `runtime_preferences` Task-composer model scope — nullable `last_task_provider_connection_id`/`last_task_model_id` (project Work Orders M003; existing rows untouched, bounds enforced in Rust)
- **v63**: external task triggers — verifier-only `task_trigger` credentials (SHA-256 hex, never plaintext) plus the `task_trigger_receipt` idempotency ledger (project Work Orders M005; existing work orders untouched)
- **v64**: project/channel chat access policy — `chat_project_policy` + `chat_project_chat_override`, `chat_channel_policy` + `chat_channel_chat_override` with optimistic revisions (team-collaboration M002, ADR-0006; absent rows preserve role defaults, no chat backfill)
- **v65**: session-control turn-scoped shared-session controller lease (team-collaboration M004)
- **v66**: Eggwork fixed-target execution target and remote handle fields
- **v67**: nullable `job_attempt.source_subject_json` for attempt-scoped execution source provenance; historical rows remain NULL and are never backfilled from a workspace
- **v68**: `work_plan_eggplan_binding`, `work_plan_eggplan_item_binding`, `work_order_eggplan_binding` (M003; append/replace only). No migration for C001: the physical `source_subject_json` column already stores versioned JSON, and the nested `ExecutionSubjectRevision` evolved additively from schema v1 (native `dirty_digest` only) to schema v2 (plus optional `eggplan_dirty_digest` in `sha256:<hex>` form). Historical v1 rows stay readable; dirty v1 fails closed for bound exact-subject evidence. The v2 Eggplan-compatible digest is captured through `eggplan_repo::capture_git_subject_fingerprint` (Eggplan pin `0dd33b761e85f1364320a9208aaebd5be281c6a5`), so the persisted value is the exact byte string Eggplan repository assessment compares against, and the column remains the only execution-subject persistence owner.

## Testing

```bash
cargo test -p codegg-core --lib storage
cargo test -p codegg-core --lib storage::paths
cargo test -p codegg-core --lib storage::preferences
cargo test -p codegg-core --test storage_migrations
```

## Related Docs

- [session.md](session.md) — Schema migrations, session tables
- [workspace_services.md](workspace_services.md) — Workspace-local
  run store, catalog migration
- `crates/codegg-core/src/migration.rs` — Legacy database import
  tooling
- `crates/codegg-core/src/jobs/` — Durable job store (v23+)

## Source verification

Verified 2026-10-06 against `crates/codegg-core/src/storage/{mod,paths,preferences}.rs`
and `crates/codegg-core/src/session/schema.rs`. Corrected: added the
missing v37–v45, v48, and v50 migration entries (the list jumped v36 → v46);
stated the layout version explicitly as 68 and fixed
`default_data_root()` ref `paths.rs:41` → `:40`; added the omitted
`init_pool_at_for_migration` entry point and corrected "four" → "six";
fixed the pragma table (the blockquote had been spliced into the middle of
it, truncating it after `busy_timeout`, and the post-connect batch
actually sets `busy_timeout=5000` while the builder sets 30s); added the
`DaemonPaths` accessor line numbers and `Database` impl ref; noted the 73
`CREATE TABLE` statements and the module-owned schema-constant delegation
used by v55/v58/v59/v65/v68. All other refs (`:39` layout const, `:41`
struct, `:214` connect fn, pool sizes, TTLs, preference keys) verified
accurate.
