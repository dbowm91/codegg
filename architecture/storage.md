# Storage Module

## Purpose

The `storage` module provides SQLite database initialization, connection
pooling, WAL configuration, the `BEGIN IMMEDIATE` write-transaction helper,
and the platform-resolved path layout for
the user-scoped daemon catalog. It also hosts the `UserPreferences`
key/value store backed by `user_preferences` table.

## Where It Lives

```
crates/codegg-core/src/storage/
├── mod.rs          # Database wrapper, init functions, pragmas,
│                   # STORAGE_LAYOUT_VERSION, deprecated init()
├── immediate.rs    # ImmediateWrite — BEGIN IMMEDIATE write transaction
│                   # helper (C002); read-then-write paths must use it
├── paths.rs        # DaemonPaths — single source of truth for
│                   # catalog and asset paths
└── preferences.rs  # UserPreferences — persistent key/value store
```

## How It Works

### Database Initialization

There are **seven** entry points for getting a `SqlitePool`:

| Function | Path | Purpose |
|----------|------|---------|
| `init_daemon_catalog(paths)` | `catalog_db_path()` | User-scoped daemon catalog, no migrations |
| `init_migrated_daemon_catalog(paths)` | Same as above | Runs migrations on a single-connection bootstrap pool, closes it, then opens the normal catalog pool |
| `init_legacy_project_store(root)` | `<root>/.codegg/sessions.db` | Legacy project-local store for backward compat, **no migrations** |
| `init_migrated_legacy_project_store(root)` | Same as above | Legacy project store **with** migrations; the consumer entry point (CLI session commands, in-process core, standalone server) |
| `init_pool_at(db_path)` | Caller-supplied path | Generic pool at an arbitrary path |
| `init_pool_at_for_migration(db_path)` | Caller-supplied path | Single-connection pool reserved for migrations |
| `init(project_dir)` (deprecated) | Empty → config dir; non-empty → legacy | Retained for tests; new code MUST NOT use |

`init_migrated_daemon_catalog` is the **production daemon bootstrap
authority**. It creates a single-connection pool for migration, runs
all schema migrations, closes the migration pool, then opens the normal
10-connection pool via `init_daemon_catalog`.

The unmigrated `init_legacy_project_store` exists so the legacy import
tooling can open the same path and decide whether the file is a *real*
legacy session database: migrating first would make that probe
self-fulfilling. Every consumer of the project store wants the schema, not
the probe, and must use `init_migrated_legacy_project_store` instead.

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

Applied in `connect_and_configure()` (`storage/mod.rs:255`) as a
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

The current layout version is defined by
`storage::STORAGE_LAYOUT_VERSION` and must track the highest migration wired
into the canonical schema path in `session/schema.rs` (see
`scripts/check_project_catalog_invariants.py` and the
`tests/core_daemon/storage_migrations.inc` equality assertion). The generated
[storage-catalog.md](storage-catalog.md) records the constant's value and the
schema it describes, so this document does not restate either. The constant is
exported and referenced from `MigrationMarker.storage_layout_version` for the
migration tooling that imports legacy project databases.

Migration 91 adds `session_working_directory`, a session-keyed relative path
and compare-and-set revision for ADR-0055 Option A. The path is only a
locator; each turn resolves it beneath the session's unchanged canonical
workspace root and refuses a missing or escaping directory.

Version 84 adds one additive nullable column, `runtime_preferences.reasoning_selection`,
holding the daemon-owned extended-thinking selection (provider reasoning
control M002). Plain `ALTER TABLE … ADD COLUMN` leaves every existing row
untouched — approval, sandbox, session-model and Task-composer preferences
are all preserved — and the new column defaults to `NULL` ("no explicit
selection"). The stored value is the protocol's own serialized selection
shape, so the durable column carries no second vocabulary; the effort label
inside it is an open, provider-scoped string carried verbatim. Bounds are
enforced in Rust on write and re-checked on read, not by the schema on a
migrated database.

Migration 70 rebuilds `cached_models` with the composite primary key
`(id, provider)`. The table is a **cross-provider** discovery cache, so the
same model id is legitimately advertised by several providers at once
(`gpt-*` on OpenAI/Azure/OpenRouter, `claude-*` on Anthropic/Bedrock,
`gemini-*` on Google/Vertex). Under the original single-column `id` key a
whole-batch insert aborted with `UNIQUE constraint failed:
cached_models.id`; because the writer clears the table before inserting,
the cache was left permanently empty and every discovery refresh re-hit
the network. Covered by
`crates/codegg-core/tests/cached_models_composite_key.rs`. Migration 37 adds canonical
`agent_task` and `agent_run` tables with typed string IDs, session/root/parent/
workspace/status indexes, unique delegation identity, scheduler job/attempt
links, bounded terminal references, and versioned budget JSON. Migration 46
adds the `edit_checkpoint` table for durable per-batch pre/post file states
scoped to workspace/session/turn/batch, reusing snapshot size/symlink bounds.
Migration 47 adds the `edit_restore_operation` audit table for checked
Undo/Reapply lineage, conflict/partial evidence, and restart durability.
Migration 69 adds `provider_connection_health.credential_status` as the
separate provider credential-verification axis (additive
`ALTER TABLE ... ADD COLUMN` with a CHECK constraint mirroring
`codegg_providers::qualification::CredentialVerification`), defaulting every
pre-existing row to `'unverified'` because no prior schema recorded
authenticated evidence.
Migration 49 adds `agent_convergence` and `agent_convergence_cycle` for the
M001 durable convergence foundation. The tables retain the exact bounded
objective/criteria specification and owner identity, plus cycle references
to existing agent groups/runs, verifier verdicts, and owner decisions. They
do not duplicate run results or store transcripts/tool output. The SQLite
store uses revision-checked lifecycle transitions and unique idempotency
keys; restart reconciliation remains a pure classifier in the core domain.
The legacy `task` table remains readable as compatibility history; it is not
backfilled with unverifiable durable-run provenance.

Migration 79 adds the bounded `workstream_claim` intent records,
`workstream_claim_dependency` edges, and append-only
`workstream_claim_event` provenance (Agent Workstream Blackboard M003).
Claims are project/workspace/group scoped, revision and lease-generation
fenced, and have an active exact-target uniqueness index; path/interface
overlap checks remain conservative advice. The records never change
filesystem, Git, sandbox, or scheduler authority. Group claim history is
capped at 128 claims and 4096 events; each claim has at most 32 prerequisite
edges. Existing agent run/task/group rows are not backfilled.

The durable run records intentionally retain summaries, digests, references,
and failure classifications rather than full prompts, hidden reasoning,
credentials, permission bodies, or unbounded tool output. Scheduler recovery
reconciles terminal/non-replayable delegated jobs to `Interrupted`,
`Cancelled`, or the already-recorded terminal run outcome.

## Key Types & APIs

### Database (`storage/mod.rs:58`, `impl` at `storage/mod.rs:62`)

```rust
impl Database {
    pub async fn new(path: &str) -> Result<Self, StorageError>;
    pub fn pool(&self) -> &SqlitePool;
    pub async fn migrate(&self) -> Result<(), StorageError>;
    pub async fn health_check(&self) -> Result<(), StorageError>;
    pub async fn close(self);
}
```

### ImmediateWrite (`storage/immediate.rs`)

```rust
pub async fn begin_immediate(pool: &SqlitePool) -> Result<ImmediateWrite, sqlx::Error>;

pub struct ImmediateWrite { /* conn, settled */ }

impl ImmediateWrite {
    pub async fn begin(pool: &SqlitePool) -> Result<Self, sqlx::Error>;
    pub async fn begin_with<E>(pool: &SqlitePool, map: impl Fn(sqlx::Error) -> E) -> Result<Self, E>;
    pub async fn commit(self) -> Result<(), sqlx::Error>;
    pub async fn commit_with<E>(mut self, map: impl Fn(sqlx::Error) -> E) -> Result<(), E>;
}
```

`ImmediateWrite::begin` opens the transaction with `BEGIN IMMEDIATE`, so
SQLite's write lock is taken **at `BEGIN`** rather than acquired later as a
read-lock upgrade. A deferred transaction takes no lock at all: the first
`SELECT` holds a shared read lock and the first `UPDATE` must upgrade it —
which SQLite refuses *without* waiting, so `busy_timeout` never applies and
two such callers can both fail with `SQLITE_BUSY` instead of one winning
and the other waiting (measured: 204 of 256 concurrent claim rounds failed
before the fix). `ImmediateWrite` is therefore mandatory for any path that
reads before it writes; `scripts/check_queue_write_transaction_shape.py`
(C002) enforces the shape on the queue store. The transaction is opened
through sqlx's `TransactionManager` (not a raw `BEGIN`) so sqlx's own
transaction depth counter stays correct — a raw `BEGIN` leaves it at zero,
which turns the queued rollback into a no-op and returns the connection to
the pool still holding its write lock. `Drop` rolls back an unsettled
transaction.

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
pub async fn init_migrated_legacy_project_store(project_root: &Path)
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
9. **Read-then-write paths need `BEGIN IMMEDIATE`**: a deferred transaction
   takes a shared read lock on its first `SELECT` and must *upgrade* it at
   the first `UPDATE`, which SQLite refuses without waiting — `busy_timeout`
   never applies and two callers can both fail. Use
   `storage::immediate::begin_immediate` (or `ImmediateWrite::begin`) for any
   path that reads before it writes. `scripts/check_queue_write_transaction_shape.py`
   (C002) enforces the shape on the queue store.
10. **Legacy-store consumers must use the migrated entry point**:
    `init_legacy_project_store` deliberately does not migrate (the legacy
    import probe must not be self-fulfilling); consumers use
    `init_migrated_legacy_project_store`.

## Migrations (Storage Layout Context)

Migrations are implemented in `session/schema.rs`, not in the storage
module. The storage module calls `session::schema::migrate()` during
initialization. Every migration is a `migrate_v<N>` function wired into one
ordered chain, and `scripts/check_project_catalog_invariants.py` fails when
the chain, the dispatch arms, and the function definitions disagree or when a
number is missing.

The **surface** each migration produces — every table, column, and index, with
the migration that introduced it — is generated by running the chain and
committed as [storage-catalog.md](storage-catalog.md). That catalog is the only
enumeration of the schema; this document states the contract.

Several late migrations delegate their statements to a module-owned
schema constant rather than inlining SQL: v55 →
`collaboration::CHAT_SCHEMA_STATEMENTS`, v56 →
`collaboration::CHAT_ACTION_SCHEMA_STATEMENTS`, v58 →
`approval::RUNTIME_PREFERENCE_SCHEMA_STATEMENTS`, v59 →
`work_plan::WORK_PLAN_SCHEMA_STATEMENTS`, v60 →
`work_order::WORK_ORDER_SCHEMA_STATEMENTS`, v63 →
`work_order::TASK_TRIGGER_SCHEMA_STATEMENTS`, v64 →
`collaboration::CHAT_POLICY_SCHEMA_STATEMENTS`, v65 →
`session_control::SESSION_CONTROL_SCHEMA_STATEMENTS`, and v68 →
`work_plan::WORK_PLAN_REPOSITORY_BINDING_SCHEMA_STATEMENTS`, v84 →
`approval::RUNTIME_PREFERENCE_NETWORK_INTENT_MIGRATION_STATEMENTS`, and v88 →
`approval::RUNTIME_PREFERENCE_REASONING_MIGRATION_STATEMENTS`.

### Migration-safety contract

- **One transaction per step.** `migrate_and_record` opens a transaction,
  applies one `migrate_v<N>`, and advances the `migration_version` marker only
  when the step commits. A failure mid-chain leaves the marker at the last
  completed step, so a restart resumes rather than repeating an applied step.
- **Migrations are additive and ordered.** A published version number is never
  reused. Two branches renumbered their migrations (to v86/v87 and to v88)
  during reconciliation with `main`, which had already published the numbers
  they first took; see the `STORAGE_LAYOUT_VERSION` doc comment.
- **A backfill is migration-local.** No migration reads a workspace, a file, or
  a service outside the database it migrates, so a legacy database can be
  migrated on a host that no longer has the workspace.
- **The layout marker tracks the highest wired migration.** The guard fails on
  either half of a mismatch, and `storage_migrations.inc` asserts a fully
  migrated database reports exactly `STORAGE_LAYOUT_VERSION`.
- **A migration that changes no structure is still a migration.** The catalog
  records it as `none`; it is never omitted, because its absence would make a
  later attribution ambiguous.

## Testing

```bash
cargo test -p codegg-core --lib storage
cargo test -p codegg-core --lib storage::paths
cargo test -p codegg-core --lib storage::preferences
# `storage_migrations` is a member of the root package's `core_daemon`
# family, not a standalone codegg-core test target.
cargo test --test core_daemon -- storage_migrations
# The generated catalog's freshness gate, and its update mode.
cargo test -p codegg-core --lib storage_catalog
CODEGG_UPDATE_STORAGE_CATALOG=1 cargo test -p codegg-core --lib storage_catalog
python3 scripts/check_storage_catalog.py
python3 scripts/check_storage_catalog.py --self-test
```

## Related Docs

- [storage-catalog.md](storage-catalog.md) — **generated**; every table,
  column, and index with its owning module and introducing migration
- [session.md](session.md) — Schema migrations, session tables
- [workspace_services.md](workspace_services.md) — Workspace-local
  run store, catalog migration
- `crates/codegg-core/src/migration.rs` — Legacy database import
  tooling
- `crates/codegg-core/src/jobs/` — Durable job store (v23+)

## Source verification

Verified 2026-10-23 against `crates/codegg-core/src/storage/{mod,paths,preferences,immediate}.rs`
and `crates/codegg-core/src/session/schema.rs`.

Corrections made in the 2026-11-11 pass:

- **The schema enumeration moved to a generated catalog.**
  `architecture/storage-catalog.md` is produced by running the real migration
  chain. The hand count this document previously carried (95 statements / 91
  tables) counted inline `CREATE TABLE` text in `session/schema.rs` and missed
  the tables created by module-owned schema constants; the chain reports
  **122 tables / 223 indexes**. The per-migration "Key storage-layout
  migrations" list is deleted — the catalog attributes every object to the
  migration that introduced it.

Corrections made in this pass:

- **Layout version 79 → 87**, and its ref `storage/mod.rs:41` → `:48`. The
  layout constant must track the highest migration; `migrate_v<N>` runs
  v1–v87 with **no gaps** (87 functions).
- **"six" init entry points → seven.** `init_migrated_legacy_project_store`
  was missing entirely, along with the reason the *unmigrated*
  `init_legacy_project_store` exists at all (the legacy-import probe must
  not be self-fulfilling by migrating first).
- **`storage/immediate.rs` added to the module tree** and given a
  Key Types entry. `ImmediateWrite` / `begin_immediate` is the C002
  `BEGIN IMMEDIATE` helper that every read-then-write path must use; it was
  absent from a doc whose subject is the storage module.
- **Ref fixes:** `connect_and_configure()` `:214` → `:247`; `Database`
  struct `:41` → `:50`; `impl Database` `:45` → `:54`.
- **Test target fix.** `cargo test -p codegg-core --test storage_migrations`
  selects nothing — `storage_migrations` is a member of the root package's
  `core_daemon` family (`tests/core_daemon/storage_migrations.inc`), and
  `crates/codegg-core/tests/` has no such file. Corrected to
  `cargo test --test core_daemon -- storage_migrations`.
- **Migration list completed.** v69–v87 were missing from the
  "Key storage-layout migrations" list (it stopped at v68), and the
  module-owned schema-constant delegation list was missing v56, v60, v63,
  v64, and v84.

Verified accurate, no change: the pragma table and the blockquote
explaining why per-connection pragmas must ride `SqliteConnectOptions` (the
builder sets `busy_timeout(30s)`/`foreign_keys(true)`/`journal_mode(WAL)`/
`synchronous(NORMAL)`; the post-connect batch re-asserts `busy_timeout=5000`),
the pool sizes (10 normal / 1 migration) and the 30 s acquire timeout, the
`Database` method set (`new`, `pool`, `migrate`, `health_check`, `close`),
the WAL-checkpoint-then-background-`quick_check` startup sequence with its
5 s delay and non-fatal policy, the `DaemonPaths` struct and all eleven
accessor line refs, the `UserPreferences` API and its two known keys, and
the `CODEGG_DATA_HOME` override.
