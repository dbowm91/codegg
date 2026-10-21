# C002 evidence 03 — Storage, migration and CAS conventions

Read-only source audit of the CodeGG storage substrate, performed against the
worktree `/home/sugarwookie/projects/codegg/.worktrees/m005-writer-audit`.
Every claim below carries a `file:line` citation. Where a fact could not be
established from source it is marked `UNPROVEN` with the reason.

> **Tree state this audit reflects.** The worktree is under concurrent C002
> implementation work. During the audit, `ImmediateWrite` was extracted from
> `crates/codegg-core/src/session/user_prompt_queue.rs` into a new shared
> module `crates/codegg-core/src/storage/immediate.rs`
> (`crates/codegg-core/src/storage/mod.rs:23` — `pub mod immediate;`), and
> `scripts/check_queue_write_transaction_shape.py` was updated to follow the
> primitive (now three checks; two of them read `IMMEDIATE_MODULE`,
> `scripts/check_queue_write_transaction_shape.py:46-48`). All citations in
> this document were re-verified against that post-extraction state. The
> guard and its `--self-test` both pass in that state (observed).

---

## 1. Where do SQLite migrations live and how are they numbered/applied?

**All schema migrations live in one file:** `crates/codegg-core/src/session/schema.rs`.
There is no separate migrations directory and no external migration tool
(sqlx's `migrate!` macro is not used).

**Entry point** — `crates/codegg-core/src/session/schema.rs:5`:

```rust
pub async fn migrate(pool: &SqlitePool) -> Result<(), StorageError> {
```

**Version bookkeeping.** `migrate()` first creates the single-row version table
(`crates/codegg-core/src/session/schema.rs:6-16`):

```sql
CREATE TABLE IF NOT EXISTS migration_version (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL DEFAULT 0
)
```

then reads the current version
(`crates/codegg-core/src/session/schema.rs:18-23`):

```sql
SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)
```

**Numbering and application.** Migrations are applied in ascending integer
order, each guarded by an `if current_version < N` fence
(`crates/codegg-core/src/session/schema.rs:25-273`). The tail of the chain is
`crates/codegg-core/src/session/schema.rs:262-273`:

```rust
    if current_version < 80 {
        migrate_and_record(pool, 80).await?;
    }
    if current_version < 81 {
        migrate_and_record(pool, 81).await?;
    }
    if current_version < 82 {
        migrate_and_record(pool, 82).await?;
    }
    if current_version < 83 {
        migrate_and_record(pool, 83).await?;
    }

    Ok(())
}
```

**The function that applies them** — `migrate_and_record`
(`crates/codegg-core/src/session/schema.rs:278-400`). Its structure is:

1. Open a transaction (`crates/codegg-core/src/session/schema.rs:279-282`):
   ```rust
   let mut tx = pool
       .begin()
       .await
       .map_err(|e| StorageError::Migration(e.to_string()))?;
   ```
2. Dispatch to the per-version function through a `match` arm
   (`crates/codegg-core/src/session/schema.rs:284-373`); the tail arms are at
   `crates/codegg-core/src/session/schema.rs:364,367,368`:
   ```rust
               79 => migrate_v79(&mut tx).await?,
   ...
               82 => migrate_v82(&mut tx).await?,
               83 => migrate_v83(&mut tx).await?,
   ```
   An unknown version is a hard error (`crates/codegg-core/src/session/schema.rs:370-373`).
3. Record the new version in the same transaction
   (`crates/codegg-core/src/session/schema.rs:375-382`):
   ```sql
   INSERT INTO migration_version (id, version) VALUES (1, ?) \
    ON CONFLICT(id) DO UPDATE SET version = excluded.version
   ```
4. Commit on success; on error the transaction rolls back on drop
   (`crates/codegg-core/src/session/schema.rs:388-399`):
   ```rust
       match result {
           Ok(()) => {
               tx.commit()
                   .await
                   .map_err(|e| StorageError::Migration(e.to_string()))?;
               Ok(())
           }
           Err(e) => {
               // `tx` rolls back on drop; no explicit ROLLBACK needed.
               Err(e)
           }
       }
   ```

**Three wirings must agree for a migration to count** — this is what the
catalog guard parses (`scripts/check_project_catalog_invariants.py:36-48`):

1. the chain: `migrate_and_record(pool, N)` (`scripts/check_project_catalog_invariants.py:46`),
2. the dispatch arm: `N => migrate_vN` (`scripts/check_project_catalog_invariants.py:47-48`),
3. the function definition `async fn migrate_vN(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>)` (e.g. `crates/codegg-core/src/session/schema.rs:3493`).

**Physical file order of the `migrate_vN` definitions is NOT the version order.**
`migrate_v83` is defined at `crates/codegg-core/src/session/schema.rs:3493`
but `migrate_v72` is defined at `crates/codegg-core/src/session/schema.rs:3556`
and `migrate_v50` at `crates/codegg-core/src/session/schema.rs:3589`. Only the
version *number* determines application order.

**Migration wiring is re-exported** from the session module
(`crates/codegg-core/src/session/mod.rs:13` — `pub mod schema;`).

---

## 2. What is `STORAGE_LAYOUT_VERSION`? Where is it, what must it equal, what enforces it?

**Definition** — `crates/codegg-core/src/storage/mod.rs:42`:

```rust
pub const STORAGE_LAYOUT_VERSION: u32 = 83;
```

**It must equal the highest migration wired into the canonical schema path.**
The guard states the contract at
`scripts/check_project_catalog_invariants.py:292-307`:

> "The canonical layout marker (`STORAGE_LAYOUT_VERSION` in
> `crates/codegg-core/src/storage/mod.rs`) must equal the highest schema
> migration actually wired into the canonical migration path in
> `crates/codegg-core/src/session/schema.rs`."

and enforces it at `scripts/check_project_catalog_invariants.py:308-326`:

```python
    if layout_version != highest:
        print(
            "FAIL: STORAGE_LAYOUT_VERSION "
            f"({layout_version}) does not match highest wired schema "
            f"migration ({highest}); bump the layout marker together with "
            "its migration, or vice versa"
        )
        return False
```

**What "highest" is derived from** — three independent wirings, and a
disagreement among them raises rather than silently passing
(`scripts/check_project_catalog_invariants.py:36-48`):

> "Three independent wirings must agree: the sequential `migrate()` upgrade
> chain (`migrate_and_record(pool, N)`), the `migrate_and_record` dispatch
> arms (`N => migrate_vN`), and the `migrate_vN` function definitions.
> Returns the shared maximum; raises `ValueError` on any disagreement so
> a half-wired migration fails the guard instead of silently passing."

**The marker is parsed, not imported** —
`scripts/check_project_catalog_invariants.py:27-33` reads
`crates/codegg-core/src/storage/mod.rs` with
`re.search(r"STORAGE_LAYOUT_VERSION\s*:\s*u32\s*=\s*(\d+)", source)`.

**A second, executable contract** exists in `tests/storage_migrations.rs`,
which asserts a fully migrated database reports exactly
`STORAGE_LAYOUT_VERSION` (referenced at
`scripts/check_project_catalog_invariants.py:299-301`).

**The repo rule** — `AGENTS.md:130-131`:

> "storage layout → `check_project_catalog_invariants.py`
> (`STORAGE_LAYOUT_VERSION` must track the highest migration in
> `crates/codegg-core/src/session/schema.rs`)"

and the module skill restates it at
`.opencode/skills/session-storage/SKILL.md:41-43`:

> "**`STORAGE_LAYOUT_VERSION` tracks the highest migration.** When adding
> `migrate_vN`, bump the constant in the same change (guard:
> `scripts/check_project_catalog_invariants.py`)."

**Current state:** `STORAGE_LAYOUT_VERSION = 83` and the highest wired
migration is `migrate_v83`. The guard passes today — `7/7 checks passed.`
(observed run of `python3 scripts/check_project_catalog_invariants.py`).

**C002 consequence:** if C002 adds `migrate_v84`, it MUST bump
`crates/codegg-core/src/storage/mod.rs:42` to `84` in the same change.

**Wiring status — IMPORTANT:** this guard is **NOT** in `scripts/verify.sh`
and **NOT** in CI. `grep -n "catalog" scripts/verify.sh` returns only
`check_provider_catalog_consistency.py` (`scripts/verify.sh:141-142`), and
`.github/workflows/ci.yml` lists its `check_*` steps at
`.github/workflows/ci.yml:106-157` with no `check_project_catalog_invariants.py`
entry. It is a **manual / change-triggered** guard (per `AGENTS.md:130-131`,
which lists it under guards "outside both" quick and CI). Closure records
that claim it "passes via `verify.sh quick`" (e.g.
`plans/closure/async-background-jobs-corrective/001-status.md:137`) are
**inaccurate against the current tree** — this audit verified the guard is
absent from `scripts/verify.sh`. The skill's own static-guards block lists it
as a separate manual invocation
(`.opencode/skills/session-storage/SKILL.md:119-122`).

---

## 3. Exact convention for a new additive migration (verbatim recent example)

### 3a. `migrate_v83` — table + index creation with a bounded idempotency key

`crates/codegg-core/src/session/schema.rs:3493-3552`, verbatim:

```rust
async fn migrate_v83(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<(), StorageError> {
    let has_agent_run_table: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'agent_run')",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| StorageError::Migration(error.to_string()))?;
    if has_agent_run_table {
        let has_shared_scratch_column: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('agent_run') WHERE name = 'shared_scratch_json')",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| StorageError::Migration(error.to_string()))?;
        if !has_shared_scratch_column {
            sqlx::query(
                "ALTER TABLE agent_run ADD COLUMN shared_scratch_json TEXT NOT NULL DEFAULT '[]'",
            )
            .execute(&mut **tx)
            .await
            .map_err(|error| StorageError::Migration(error.to_string()))?;
        }
    }
    for statement in [
        r#"
        CREATE TABLE IF NOT EXISTS agent_scratch_resource (
            scratch_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            owner_key TEXT NOT NULL,
            owner_run_id TEXT,
            alias TEXT NOT NULL CHECK (length(alias) BETWEEN 1 AND 32),
            idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 512),
            revoked INTEGER NOT NULL DEFAULT 0 CHECK (revoked IN (0,1)),
            created_at INTEGER NOT NULL,
            UNIQUE (session_id, owner_key, idempotency_key),
            UNIQUE (session_id, owner_key, alias COLLATE NOCASE),
            FOREIGN KEY (owner_run_id) REFERENCES agent_run(run_id) ON DELETE CASCADE
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_agent_scratch_resource_owner ON agent_scratch_resource(session_id, owner_key, revoked)",
        r#"
        CREATE TABLE IF NOT EXISTS agent_scratch_attempt_lease (
            scratch_id TEXT NOT NULL,
            resource_kind TEXT NOT NULL CHECK (resource_kind IN ('private_run','shared')),
            run_id TEXT NOT NULL,
            job_id TEXT NOT NULL,
            attempt_id TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (scratch_id, job_id, attempt_id),
            FOREIGN KEY (run_id) REFERENCES agent_run(run_id) ON DELETE CASCADE
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_agent_scratch_attempt_lease_attempt ON agent_scratch_attempt_lease(job_id, attempt_id)",
    ] {
        sqlx::query(statement)
            .execute(&mut **tx)
            .await
            .map_err(|error| StorageError::Migration(error.to_string()))?;
    }
    Ok(())
}
```

### 3b. `migrate_v79` — the closest existing analogue to a C002 permit table

`crates/codegg-core/src/session/schema.rs:3291-3360`, verbatim (tables and
indexes only; the statement-list runner is at `:3353-3358`):

```rust
async fn migrate_v79(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<(), StorageError> {
    for statement in [
        r#"
        CREATE TABLE IF NOT EXISTS workstream_claim (
            claim_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            workspace_id TEXT NOT NULL,
            group_id TEXT NOT NULL,
            claim_kind TEXT NOT NULL CHECK (claim_kind IN ('task','path','interface')),
            target TEXT NOT NULL CHECK (length(target) BETWEEN 1 AND 256),
            state TEXT NOT NULL CHECK (state IN ('offered','claimed','released','blocked','completed')),
            owner_kind TEXT NOT NULL CHECK (owner_kind IN ('principal','run')),
            owner_id TEXT NOT NULL CHECK (length(owner_id) BETWEEN 1 AND 128),
            version INTEGER NOT NULL CHECK (version >= 1),
            lease_generation INTEGER NOT NULL CHECK (lease_generation >= 1),
            lease_expires_at INTEGER,
            idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (group_id) REFERENCES agent_run_group(group_id) ON DELETE CASCADE,
            UNIQUE (group_id, idempotency_key)
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_workstream_claim_group_state ON workstream_claim(group_id, state, updated_at)",
        "CREATE INDEX IF NOT EXISTS idx_workstream_claim_owner ON workstream_claim(owner_kind, owner_id, state)",
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_workstream_claim_active_target
        ON workstream_claim(group_id, claim_kind, target)
        WHERE state IN ('offered','claimed','blocked')
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS workstream_claim_dependency (
            claim_id TEXT NOT NULL,
            prerequisite_claim_id TEXT,
            prerequisite_run_id TEXT,
            created_at INTEGER NOT NULL,
            CHECK ((prerequisite_claim_id IS NOT NULL) != (prerequisite_run_id IS NOT NULL)),
            PRIMARY KEY (claim_id, prerequisite_claim_id, prerequisite_run_id),
            FOREIGN KEY (claim_id) REFERENCES workstream_claim(claim_id) ON DELETE CASCADE,
            FOREIGN KEY (prerequisite_claim_id) REFERENCES workstream_claim(claim_id) ON DELETE CASCADE,
            FOREIGN KEY (prerequisite_run_id) REFERENCES agent_run(run_id) ON DELETE CASCADE
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_workstream_claim_dependency_prerequisite ON workstream_claim_dependency(prerequisite_claim_id, prerequisite_run_id)",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_workstream_claim_dependency_claim ON workstream_claim_dependency(claim_id, prerequisite_claim_id) WHERE prerequisite_claim_id IS NOT NULL",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_workstream_claim_dependency_run ON workstream_claim_dependency(claim_id, prerequisite_run_id) WHERE prerequisite_run_id IS NOT NULL",
        r#"
        CREATE TABLE IF NOT EXISTS workstream_claim_event (
            event_id TEXT PRIMARY KEY,
            claim_id TEXT NOT NULL,
            group_id TEXT NOT NULL,
            actor_kind TEXT NOT NULL CHECK (actor_kind IN ('principal','run')),
            actor_id TEXT NOT NULL CHECK (length(actor_id) BETWEEN 1 AND 128),
            action TEXT NOT NULL CHECK (action IN ('claimed','renewed','released','blocked','unblocked','completed','handed_off','offer_accepted','handoff_requested','expired','dependency_added')),
            version INTEGER NOT NULL CHECK (version >= 1),
            lease_generation INTEGER NOT NULL CHECK (lease_generation >= 1),
            created_at INTEGER NOT NULL,
            FOREIGN KEY (claim_id) REFERENCES workstream_claim(claim_id) ON DELETE CASCADE,
            FOREIGN KEY (group_id) REFERENCES agent_run_group(group_id) ON DELETE CASCADE
        )
        "#,
        "CREATE INDEX IF NOT EXISTS idx_workstream_claim_event_group ON workstream_claim_event(group_id, created_at, event_id)",
    ] {
```

### 3c. Conventions extracted (all cited)

| Convention | Evidence |
|---|---|
| Every migration is `async fn migrate_vN(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<(), StorageError>` | `crates/codegg-core/src/session/schema.rs:3493`, `:3291` |
| Table names: `snake_case`, **singular**, no prefix scheme; domain prefixes group related tables (`workstream_claim`, `workstream_claim_dependency`, `workstream_claim_event`) | `crates/codegg-core/src/session/schema.rs:3294`, `:3322`, `:3338` |
| Column names: `snake_case`; the primary key is `*_id TEXT PRIMARY KEY` | `crates/codegg-core/src/session/schema.rs:3295`, `:3519` |
| Timestamps: `created_at INTEGER NOT NULL` / `updated_at INTEGER NOT NULL` (epoch millis) | `crates/codegg-core/src/session/schema.rs:3308-3309`, `:3526` |
| Monotonic counters: `version INTEGER NOT NULL CHECK (version >= 1)` | `crates/codegg-core/src/session/schema.rs:3304` |
| Generation fence: `lease_generation INTEGER NOT NULL CHECK (lease_generation >= 1)` | `crates/codegg-core/src/session/schema.rs:3305` |
| Idempotency: `idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128)` plus `UNIQUE (group_id, idempotency_key)` | `crates/codegg-core/src/session/schema.rs:3307`, `:3311` |
| Enumerations via `CHECK (col IN (...))` — never a bare TEXT | `crates/codegg-core/src/session/schema.rs:3299`, `:3301`, `:3302` |
| Booleans as `INTEGER NOT NULL DEFAULT 0 CHECK (revoked IN (0,1))` | `crates/codegg-core/src/session/schema.rs:3525` |
| Bounded strings via `CHECK (length(col) BETWEEN lo AND hi)` | `crates/codegg-core/src/session/schema.rs:3300`, `:3303`, `:3523-3524` |
| Foreign keys: `FOREIGN KEY (col) REFERENCES table(pk) ON DELETE CASCADE` | `crates/codegg-core/src/session/schema.rs:3310`, `:3329-3331`, `:3348-3349` |
| Statements are `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS` and run in a `for statement in [...]` loop | `crates/codegg-core/src/session/schema.rs:3353-3358`, `:3515-3551` |
| Errors map to `StorageError::Migration(error.to_string())` | `crates/codegg-core/src/session/schema.rs:3557` |
| Additive column changes use the `add_column_ignore_duplicate` helper (tolerates "duplicate column name") | `crates/codegg-core/src/session/schema.rs:449-465` |

**Migration helper for additive columns** —
`crates/codegg-core/src/session/schema.rs:449-465`, verbatim:

```rust
async fn add_column_ignore_duplicate(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    statement: String,
) -> Result<(), StorageError> {
    match sqlx::query(&statement).execute(&mut **tx).await {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(db_err)) => {
            if db_err.message().contains("duplicate column name") {
                Ok(())
            } else {
                Err(StorageError::Migration(db_err.message().to_string()))
            }
        }
        Err(e) => Err(StorageError::Migration(e.to_string())),
    }
}
```

---

## 4. What SQL client is used? Version, bundled or system, and the exact APIs

**Client: `sqlx`, version `0.8`.** Workspace declaration —
`Cargo.toml:46`:

```toml
sqlx = { version = "0.8", default-features = false }
```

Workspace dependency entry (features applied by members) — `Cargo.toml:159`:

```toml
sqlx = { workspace = true, features = ["runtime-tokio", "sqlite", "derive", "chrono", "json"] }
```

`codegg-core` consumes it at `crates/codegg-core/Cargo.toml:43`:

```toml
sqlx = { workspace = true, features = ["runtime-tokio", "sqlite", "derive", "chrono", "json"] }
```

**Locked versions** (`Cargo.lock`): `sqlx` 0.8.6 (`Cargo.lock:5131-5134`),
`sqlx-sqlite` 0.8.6 (`Cargo.lock:5298-5301`), `libsqlite3-sys` 0.30.1
(`Cargo.lock:3113-3116`).

**SQLite is BUNDLED (compiled from source), not system.** The `sqlite` feature
of `sqlx` maps to `sqlx-sqlite/bundled`
(`sqlx-0.8.6/Cargo.toml:478-481`).

> **Citation convention for upstream crates:** the `sqlx-*` citations below are
> to the vendored crate sources in the local cargo registry, not to files in
> this repository. All are under
> `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/<crate>/`, e.g.
> `sqlx-core-0.8.6/src/transaction.rs` =
> `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sqlx-core-0.8.6/src/transaction.rs`.
> The versions cited are the ones pinned in this repo's `Cargo.lock`.

```toml
sqlite = [
    "_sqlite",
    "sqlx-sqlite/bundled",
    "sqlx-macros?/sqlite",
```

and `sqlx-sqlite`'s `bundled` feature is `libsqlite3-sys/bundled`
(`sqlx-sqlite-0.8.6/Cargo.toml:146`):

```toml
bundled = ["libsqlite3-sys/bundled"]
```

**Opening a pool.** The canonical opener is `init_pool_at`
(`crates/codegg-core/src/storage/mod.rs:176`), which builds a
`SqlitePoolOptions` with the repo's standard pragmas
(`crates/codegg-core/src/storage/mod.rs:243-276`):

```rust
        .busy_timeout(Duration::from_secs(30))
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        ...
        .max_connections(max_connections)
```

with post-connect PRAGMAs at `crates/codegg-core/src/storage/mod.rs:269-276`:

```sql
        PRAGMA journal_mode=WAL;
        PRAGMA busy_timeout=5000;
        ...
        PRAGMA foreign_keys = ON;
```

**Running a transaction — `pool.begin()` is DEFERRED, not IMMEDIATE.**
`sqlx-core-0.8.6/src/transaction.rs:277-283`:

```rust
pub fn begin_ansi_transaction_sql(depth: usize) -> Cow<'static, str> {
    if depth == 0 {
        Cow::Borrowed("BEGIN")
    } else {
        Cow::Owned(format!("SAVEPOINT _sqlx_savepoint_{depth}"))
    }
}
```

This is what `pool.begin()` issues at depth 0
(`sqlx-sqlite-0.8.6/src/connection/worker.rs:224`).

**How to open a `BEGIN IMMEDIATE` — two real in-tree idioms.**

*Idiom A — through sqlx's transaction manager (keeps sqlx's depth counter
correct).* This is the **shared, reusable primitive** the tree now exposes as
`codegg_core::storage::immediate::ImmediateWrite`
(`crates/codegg-core/src/storage/mod.rs:23` — `pub mod immediate;`).
The struct is at `crates/codegg-core/src/storage/immediate.rs:59-65` and the
constructor at `:72-89`, verbatim:

```rust
/// A write transaction that holds SQLite's write lock from `BEGIN` to commit.
pub struct ImmediateWrite {
    /// `Option` so [`Drop`] can take the connection out; it is `Some` for the
    /// whole time the transaction is usable.
    conn: Option<sqlx::pool::PoolConnection<Sqlite>>,
    settled: bool,
}

impl ImmediateWrite {
    /// Open an immediate write transaction against `pool`.
    ///
    /// The caller is responsible for committing or letting the transaction roll
    /// back; see [`ImmediateWrite::commit`].
    pub async fn begin(pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let mut conn = pool.acquire().await?;
        // `TransactionManager::begin` takes the statement to run. Handing it
        // `BEGIN IMMEDIATE` is what makes this transaction immediate — and going
        // through the manager rather than issuing raw SQL is what keeps sqlx's
        // own transaction **depth** counter correct. A raw `BEGIN` leaves that
        // counter at zero, so the queued rollback below became a no-op and the
        // connection went back to the pool still holding its write lock.
        SqliteTx::begin(
            &mut conn,
            Some(std::borrow::Cow::Borrowed("BEGIN IMMEDIATE")),
        )
        .await?;
        Ok(Self {
            conn: Some(conn),
            settled: false,
        })
    }
```

The queue store's thin local wrapper over it is
`crates/codegg-core/src/session/user_prompt_queue.rs:1082-1084`:

```rust
async fn begin_immediate(pool: &SqlitePool) -> Result<ImmediateWrite, UserPromptQueueError> {
    ImmediateWrite::begin(pool).await.map_err(storage_error)
```

*Idiom B — raw SQL on a pooled connection.* `crates/codegg-core/src/workstream_claim.rs:384-387`
(and identically at `:523`, `:710`, `:823`):

```rust
        let mut conn = self.pool.acquire().await.map_err(storage_error)?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(storage_error)?;
```

Idiom B does not update sqlx's transaction depth counter, which is the exact
defect Idiom A's comment warns about
(`crates/codegg-core/src/storage/immediate.rs:74-79`).

**Note a live doc/code discrepancy.** `architecture/session.md:51-52` states
each migration is "wrapped in an explicit `BEGIN IMMEDIATE` transaction", and
`crates/codegg-core/src/migration.rs:407-409` refers to "the `BEGIN IMMEDIATE`
in the session schema migration". Both are wrong: `migrate_and_record` uses a
plain `pool.begin()` (`crates/codegg-core/src/session/schema.rs:279-282`),
which is a deferred `BEGIN`. The module skill already corrected this
(`.opencode/skills/session-storage/SKILL.md:150-153`):

> "Corrected the schema row, which claimed each migration runs in
> `BEGIN IMMEDIATE` (it runs in a plain `pool.begin()` transaction in
> `migrate_and_record`)"

but `architecture/session.md` was not updated. **C002 must not copy the
`architecture/session.md` claim; the code is deferred.**

---

## 5. How does a codegg-core store get constructed, and where does its database file live?

**Store construction is a thin `new(pool)` wrapper.** The canonical example —
`crates/codegg-core/src/workstream_claim.rs:237-245`, verbatim:

```rust
#[derive(Clone)]
pub struct WorkstreamClaimStore {
    pool: SqlitePool,
}

impl WorkstreamClaimStore {
    pub fn new(pool: SqlitePool) -> Arc<Self> {
        Arc::new(Self { pool })
    }
```

Wiring at the call sites: `src/agent/worker.rs:1426-1428` and
`src/tool/factory.rs:266-268`:

```rust
            codegg_core::workstream_claim::WorkstreamClaimStore::new(pool.clone())
```

**Where the database file lives.** The daemon catalog DB path comes from
`DaemonPaths::catalog_db_path()`; the platform conventions are documented at
`crates/codegg-core/src/storage/mod.rs:115-119`:

> "- macOS: `~/Library/Application Support/codegg/codegg.db`
> - Linux: `$XDG_DATA_HOME/codegg/codegg.db` (or `~/.local/share/`)"

and the opener is `crates/codegg-core/src/storage/mod.rs:120-123`:

```rust
pub async fn init_daemon_catalog(paths: &DaemonPaths) -> Result<SqlitePool, StorageError> {
    let db_path = paths.catalog_db_path();
    init_pool_at(&db_path).await
}
```

**Production bootstrap authority** — `crates/codegg-core/src/storage/mod.rs:131-136`:

```rust
pub async fn init_migrated_daemon_catalog(paths: &DaemonPaths) -> Result<SqlitePool, StorageError> {
    let migration_pool = init_pool_at_for_migration(&paths.catalog_db_path()).await?;
    crate::session::schema::migrate(&migration_pool).await?;
    migration_pool.close().await;
    init_daemon_catalog(paths).await
}
```

Called at `src/main.rs:3948` and `src/main.rs:4120`. The migration pool is
deliberately single-connection (`crates/codegg-core/src/storage/mod.rs:127-130`):

> "Migration deliberately uses a single-connection pool because the schema
> migrator runs its statements inside an explicit transaction."

**Canonical pool entry points** (skill rule, `.opencode/skills/session-storage/SKILL.md:49-58`):
`init_daemon_catalog`, `init_migrated_daemon_catalog` (production bootstrap
authority), `init_legacy_project_store`, `init_migrated_legacy_project_store`,
`init_pool_at`, and the migration-only `init_pool_at_for_migration`. Deprecated
`init()` has no in-tree callers and new code MUST NOT use it
(`.opencode/skills/session-storage/SKILL.md:49-53`).

**`isolated_pool()` — the test helper.** Canonical definition is
`tests/common/pool.rs:52`:

```rust
pub async fn isolated_pool() -> SqlitePool {
```

`codegg-core` integration tests carry **local copies** (skill rule,
`.opencode/skills/session-storage/SKILL.md:44-48`). The copy relevant to a
storage store is `crates/codegg-core/tests/user_prompt_queue.rs:21-39`,
verbatim:

```rust
async fn isolated_pool() -> SqlitePool {
    let name = format!("user_prompt_queue_{}", uuid::Uuid::new_v4().simple());
    let opts = SqliteConnectOptions::from_str(&format!("file:{name}?mode=memory&cache=shared"))
        .expect("sqlite options")
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        // Multi-connection so the concurrent-reader test exercises real
        // interleaving rather than a single serialized connection.
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("connect sqlite");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    pool
}
```

All nine `isolated_pool()` definitions in the tree:
`tests/common/pool.rs:52`, `tests/checked_restore_integration.rs:11`,
`tests/edit_checkpoint_integration.rs:18`,
`crates/codegg-core/tests/human_shell_promotion.rs:11`,
`crates/codegg-core/tests/work_plan_foundation.rs:18`,
`crates/codegg-core/tests/user_prompt_queue_delivery.rs:48`,
`crates/codegg-core/tests/user_prompt_queue.rs:21`,
`crates/codegg-core/tests/user_prompt_queue_mutation.rs:43`,
`crates/codegg-core/tests/work_plan_projection_arbiter.rs:23`,
`crates/codegg-core/tests/continuation_checkpoint.rs:16`.

**"Migrations run inside the helper; never add an extra `migrate()` call."**
`.opencode/skills/session-storage/SKILL.md:44-48`. The helper's own
`migrate()` call is at `crates/codegg-core/tests/user_prompt_queue.rs:35-37`.

---

## 6. The `begin_immediate` / C002 transaction rule in practice

### 6a. The guard's rule, quoted

`scripts/check_queue_write_transaction_shape.py:16-24`:

> "This guard exists so the shape cannot come back one `pool.begin()` at a time:
>
> 1. Every transaction in the store is opened either by `begin_immediate` (which
>    issues `BEGIN IMMEDIATE`) or is on an explicit, reasoned allowlist of deferred
>    paths whose first statement is already a write.
> 2. No function that writes queue tables may open a deferred transaction.
> 3. The rollback contract holds: an uncommitted `ImmediateWrite` queues a
>    `ROLLBACK` on its connection and never closes it."

`AGENTS.md:129-137` states the repo rule:

> "Queue **store** transaction shape → `python3 scripts/check_queue_write_transaction_shape.py`
> (C002). A queue write path that reads before it writes must open its transaction
> with `begin_immediate` (`BEGIN IMMEDIATE`), not `pool.begin()`. A deferred
> transaction takes no lock, so the first `SELECT` holds a shared read lock and the
> first `UPDATE` has to upgrade it — and SQLite refuses that upgrade **without
> waiting**, so two such callers can both fail with `SQLITE_BUSY` instead of one
> winning and the other waiting out `busy_timeout`. Measured at 204 of 256
> concurrent claim rounds before C002."

### 6b. Real production call sites that open a `BEGIN IMMEDIATE`

**Site 1 (idiom A, recommended, and now a shared public primitive).**
`crates/codegg-core/src/storage/immediate.rs:80-84`:

```rust
        SqliteTx::begin(
            &mut conn,
            Some(std::borrow::Cow::Borrowed("BEGIN IMMEDIATE")),
        )
        .await?;
```

This is the **one** place in the tree that issues `BEGIN IMMEDIATE`; it lives
in the shared primitive `codegg_core::storage::immediate::ImmediateWrite`
(`crates/codegg-core/src/storage/mod.rs:23`). The queue store reaches it
through a thin local wrapper at
`crates/codegg-core/src/session/user_prompt_queue.rs:1082-1084`:

```rust
async fn begin_immediate(pool: &SqlitePool) -> Result<ImmediateWrite, UserPromptQueueError> {
```

and calls it at `crates/codegg-core/src/session/user_prompt_queue.rs:651`,
`:793`, `:869`, `:929`, `:968`, `:1021`, `:1647`, `:1795`.

**Site 2 (idiom B, raw SQL).** `crates/codegg-core/src/workstream_claim.rs:523`:

```rust
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(storage_error)?;
```

(also `crates/codegg-core/src/workstream_claim.rs:384`, `:710`, `:823`).

### 6c. The rollback contract (`Drop`)

`crates/codegg-core/src/storage/immediate.rs:122-144`, verbatim:

```rust
impl Drop for ImmediateWrite {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // Every refusal below is a `return Err(..)` with nothing committed, and it
        // arrives here as a drop. `Drop` cannot `await` a `ROLLBACK`, so this
        // queues one on the connection's worker thread — the same thing
        // `sqlx::Transaction` does on its own drop, and for the same reason: a
        // future dropped at an arbitrary `.await` must not leave an open
        // transaction for the next borrower of this connection.
        //
        // It deliberately does **not** close the connection to force the
        // rollback. Closing works for a file-backed database and is destructive
        // for a shared-cache in-memory one, where dropping the last connection
        // destroys the database: the mutation suite caught exactly that, as an
        // unowned-queue refusal turning into `no such table` two operations
        // later.
        if let Some(mut conn) = self.conn.take() {
            SqliteTx::start_rollback(&mut conn);
        }
    }
}
```

The guard enforces this shape at
`scripts/check_queue_write_transaction_shape.py:166-194` (requires
`impl Drop for ImmediateWrite`, `start_rollback`, and forbids `detach`).

### 6d. A deferred path that is NOT a violation (its first statement is a write)

`crates/codegg-core/src/session_control.rs:607` opens a **deferred**
transaction:

```rust
        let mut tx = self.pool.begin().await?;
```

This is **not** the deadlock shape, because the transaction's *first* statement
is a write — a `DELETE` at `crates/codegg-core/src/session_control.rs:608-613`:

```rust
        sqlx::query(
            "DELETE FROM turn_admission_receipt WHERE status != 'accepted' AND updated_at_ms <= ?",
        )
        .bind(now_ms.saturating_sub(TURN_ADMISSION_RECEIPT_IDEMPOTENCY_TTL_MS))
        .execute(&mut *tx)
        .await?;
```

so the write lock is taken at that first statement, before the later `SELECT`
at `:614-621` and the `INSERT` at `:650-665`. This is exactly the reasoning the
guard's allowlist applies to `list_live`
(`scripts/check_queue_write_transaction_shape.py:53-62`):

> "`list_live` does write (the lazy expiry sweep), but its **first** statement is
> that sweep, so it already takes the write lock at that statement. Taking it
> earlier would buy nothing and would serialize every queue read in the daemon."

**This is the key distinction for C002:** a deferred transaction is safe when
its first statement is a write. A permit acquisition that must first *read*
the current permit holder has a read as its first statement, so it **cannot**
use this shape and must use `BEGIN IMMEDIATE`.

---

## 7. Pattern for a monotonic revision / CAS update

### 7a. Primary example — `workstream_claim` double-fence CAS (closest analogue to a C002 permit)

**The CAS request struct** — `crates/codegg-core/src/workstream_claim.rs:196-205`,
verbatim:

```rust
#[derive(Debug, Clone)]
pub struct ClaimCas {
    pub claim_id: WorkstreamClaimId,
    pub actor: ClaimActor,
    pub expected_version: u64,
    pub expected_lease_generation: u64,
    pub transition: ClaimTransition,
    pub new_owner: Option<ClaimActor>,
    pub lease_expires_at: Option<i64>,
}
```

**The read-then-CAS** — `crates/codegg-core/src/workstream_claim.rs:730-812`.
Fence check at `:757-767`:

```rust
        if claim.version != cas.expected_version
            || claim.lease_generation != cas.expected_lease_generation
        {
            return Err(WorkstreamClaimError::StaleRevision);
        }
        if claim
            .lease_expires_at
            .is_some_and(|deadline| deadline <= now_ms())
        {
            return Err(WorkstreamClaimError::StaleRevision);
        }
```

The CAS write at `crates/codegg-core/src/workstream_claim.rs:780-799`,
verbatim:

```rust
        let result = sqlx::query(
            "UPDATE workstream_claim SET state=?, owner_kind=?, owner_id=?, version=version+1, \
             lease_generation=lease_generation+?, lease_expires_at=?, updated_at=? \
             WHERE claim_id=? AND version=? AND lease_generation=?",
        )
        .bind(state.as_str())
        .bind(owner.as_ref().unwrap_or(&claim.owner).kind())
        .bind(owner.as_ref().unwrap_or(&claim.owner).id())
        .bind(if bump_generation { 1_i64 } else { 0_i64 })
        .bind(deadline)
        .bind(now)
        .bind(claim.claim_id.as_str())
        .bind(cas.expected_version as i64)
        .bind(cas.expected_lease_generation as i64)
        .execute(&mut **conn)
        .await
        .map_err(storage_error)?;
        if result.rows_affected() != 1 {
            return Err(WorkstreamClaimError::StaleRevision);
        }
```

Key properties of this pattern:
- `version` is the **monotonic revision** (`version=version+1`),
  `lease_generation` is the **generation fence** (`lease_generation=lease_generation+?`),
- the `WHERE` clause carries **both** expected values,
- the caller must check `rows_affected() != 1` and convert that to a typed
  stale-revision error,
- the whole read-then-write runs **inside a `BEGIN IMMEDIATE` transaction**
  opened at `crates/codegg-core/src/workstream_claim.rs:823`.

### 7b. Second example — `session_turn_controller` revision CAS

`crates/codegg-core/src/session_control.rs:819-836`, verbatim:

```rust
        let result = sqlx::query(
            "UPDATE session_turn_controller SET turn_id = ?, controller_principal = ?, \
              revision = revision + 1, updated_at = ?, last_action = 'transferred', \
              last_actor = ?, last_reason = ? \
              WHERE session_id = ? AND turn_id = ? AND revision = ? AND controller_principal = ?",
        )
        .bind(turn_id)
        .bind(to.as_str())
        .bind(now_ms)
        .bind(actor.as_str())
        .bind(reason.clone())
        .bind(session_id)
        .bind(turn_id)
        .bind(expected)
        .bind(from.as_str())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() > 0 {
```

Note: this one is a **single-statement CAS with no explicit transaction** — the
`WHERE ... AND revision = ?` predicate is the whole fence, and
`rows_affected() > 0` is the success test.

### 7c. Third example — `user_prompt_queue_owner` revision

`crates/codegg-core/src/session/user_prompt_queue.rs:1035-1043` performs the
queue-ownership transfer with a `revision = revision + 1` CAS:

```sql
            "UPDATE user_prompt_queue_owner \
             SET owner_principal = ?, revision = revision + 1, updated_at_ms = ? \
             WHERE session_id = ? AND owner_principal = ? AND revision = ?"
```

It runs inside the `ImmediateWrite` transaction opened at
`crates/codegg-core/src/session/user_prompt_queue.rs:1020`:

```rust
        let mut tx = begin_immediate(&self.pool).await?;
```

and the stale write is diagnosed by re-reading the owner **inside the same
transaction** (`crates/codegg-core/src/session/user_prompt_queue.rs:1045-1066`),
yielding a typed `UserPromptQueueError::OwnerRevisionConflict { expected,
current }`. The same `begin_immediate(&self.pool)` idiom appears at
`crates/codegg-core/src/session/user_prompt_queue.rs:650`, `:792`, `:868`,
`:928`, `:967`.

---

## 8. How are stores reopened across a process restart? / restart reconciliation

### 8a. Reopen path

1. `src/main.rs:3947-3948` / `src/main.rs:4118-4121`:
   ```rust
   let catalog_paths = storage::DaemonPaths::default();
   let pool = match storage::init_migrated_daemon_catalog(&catalog_paths).await {
   ```
   `init_migrated_daemon_catalog` migrates on a single-connection pool, closes
   it, and reopens the runtime pool
   (`crates/codegg-core/src/storage/mod.rs:131-136`).
2. Stores are then constructed from the reopened pool, e.g.
   `src/main.rs:4121`:
   ```rust
   let session_store = Arc::new(SessionStore::new(pool.clone()));
   ```

### 8b. Re-reading persisted state on open

Two patterns exist in the tree.

**Pattern 1 — hydrate an in-memory index at construction.**
`crates/codegg-core/src/workspace.rs:424-441`:

```rust
impl WorkspaceRegistry {
    /// Build a registry backed by the given store. Hydrates the in-memory
    /// index from the store so that subsequent lookups are O(1).
    pub async fn load(store: Arc<dyn WorkspaceStore>) -> Result<Arc<Self>, WorkspaceError> {
        let records = store.list(true).await?;
        let registry = Arc::new(Self {
            store,
            by_id: DashMap::new(),
            by_root: DashMap::new(),
            register_lock: Mutex::new(()),
        });
        for record in records {
            registry
                .by_id
                .insert(record.id.as_str().to_string(), Arc::new(record.clone()));
            registry
                .by_root
                .insert(record.canonical_root.clone(), record.id.clone());
```

**Pattern 2 — no cache; read on demand.**
`WorkstreamClaimStore::new` stores only the pool
(`crates/codegg-core/src/workstream_claim.rs:237-245`) and every read goes to
SQL, e.g. `crates/codegg-core/src/workstream_claim.rs:255-257`:

```rust
        let mut conn = self.pool.acquire().await.map_err(storage_error)?;
        validate_group_actor(&mut conn, group_id, project_id, workspace_id, actor).await?;
        let rows = sqlx::query(
```

This pattern is the simpler fit for a C002 permit store: a permit's
authoritative state is the row, and the store is a thin pool wrapper.

### 8c. Restart reconciliation — existing patterns

**Pattern A — mark nonterminal rows non-replayable at recovery.**
`crates/codegg-core/src/session_control.rs:744-758`, verbatim:

```rust
    /// Called once during daemon recovery. Any receipt still marked accepted
    /// belongs to an earlier process and must not be replayed automatically.
    pub async fn interrupt_incomplete_turn_receipts(
        &self,
        now_ms: i64,
    ) -> Result<u64, SessionControlError> {
        let result = sqlx::query(
            "UPDATE turn_admission_receipt SET status = 'interrupted', updated_at_ms = ? \
             WHERE status = 'accepted'",
        )
        .bind(now_ms)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
```

**Pattern B — delete orphaned leases by set-difference.**
`crates/codegg-core/src/agent_run.rs:2105-2111`, verbatim:

```rust
    async fn reconcile_scratch_attempt_leases(&self) -> Result<u64, AgentRunStoreError> {
        let result = sqlx::query("DELETE FROM agent_scratch_attempt_lease AS lease WHERE NOT EXISTS (SELECT 1 FROM job_attempt AS attempt WHERE attempt.id = lease.attempt_id AND attempt.job_id = lease.job_id AND attempt.state IN ('created','admitted','running'))")
            .execute(&self.pool)
            .await
            .map_err(storage_error)?;
        Ok(result.rows_affected())
    }
```

(Declared as a trait method at `crates/codegg-core/src/agent_run.rs:581` and
implemented at `crates/codegg-core/src/agent_run.rs:1310` and `:2105`.)

**Pattern C — pure classifier, no I/O.**
`crates/codegg-core/src/agent_convergence.rs:942` — `classify_reconciliation`
decides from persisted state alone whether a run needs reconciliation; it does
not touch the database.

**Pattern D — bounded listing of partially-materialized occurrences.**
`crates/codegg-core/src/work_order/store.rs:2632` —
`list_incomplete_occurrences` surfaces work-order occurrences that are
incomplete at startup so the caller can re-drive them.

**Pattern E — expired-lease sweep, lazily.**
`crates/codegg-core/src/session/user_prompt_queue.rs` — the `list_live` path
runs a lazy expiry sweep as its **first** statement, which is why it is on the
guard's deferred allowlist (`scripts/check_queue_write_transaction_shape.py:53-62`).

---

## 9. Idempotency-key / deduplication patterns a permit acquisition could mirror

### 9a. `workstream_claim` — the best mirror (keyed read, same-payload replay, conflict)

Schema: `idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128)`
and `UNIQUE (group_id, idempotency_key)`
(`crates/codegg-core/src/session/schema.rs:3307`, `:3311`).

Store logic — `crates/codegg-core/src/workstream_claim.rs:576-597`, verbatim:

```rust
        if let Some(existing) = sqlx::query(
            "SELECT claim_id, project_id, workspace_id, group_id, claim_kind, target, state, \
             owner_kind, owner_id, version, lease_generation, lease_expires_at, created_at, updated_at \
             FROM workstream_claim WHERE group_id = ? AND idempotency_key = ?",
        )
        .bind(input.group_id.as_str())
        .bind(&input.idempotency_key)
        .fetch_optional(&mut **conn)
        .await
        .map_err(storage_error)?
        {
            let claim = row_to_claim(&existing)?;
            if claim.project_id == input.project_id
                && claim.workspace_id == input.workspace_id
                && claim.target.kind() == input.target.kind()
                && claim.target.normalized()? == target
                && claim.owner == input.actor
            {
                return Ok(claim);
            }
            return Err(WorkstreamClaimError::IdempotencyConflict);
        }
```

**The shape to mirror:** look up by idempotency key *first*; if a row exists
and the payload matches, return it unchanged (replay); if it exists and the
payload differs, return a typed conflict — never overwrite.

The Rust-side bound mirrors the SQL `CHECK`:
`crates/codegg-core/src/workstream_claim.rs:514-515`:

```rust
        if input.idempotency_key.is_empty()
            || input.idempotency_key.len() > MAX_CLAIM_IDEMPOTENCY_BYTES
```

### 9b. `agent_scratch_resource` — composite-scoped idempotency key

`crates/codegg-core/src/session/schema.rs:3524` and `:3527`:

```sql
            idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 512),
            ...
            UNIQUE (session_id, owner_key, idempotency_key),
```

### 9c. `turn_admission_receipt` — key + payload digest, conflict on altered payload

`crates/codegg-core/src/session_control.rs:766-768` reads back
`submission_id, session_id, principal_id, turn_id, request_sha256, status,
accepted_at_ms, updated_at_ms`, and the receipt's `request_sha256` is the
payload digest that makes a retry with an altered payload a conflict rather
than a silent overwrite. The `AGENTS.md:150-152` rule states it:

> "Turn admission atomically commits promotion consumption, the controller lease,
> and a receipt keyed by envelope `request_id`; retry the identical request with
> that ID. Startup marks any nonterminal accepted receipt interrupted and never
> replays it."

### 9d. Related but distinct: `UNIQUE(run_id, idempotency_key)`

`crates/codegg-core/src/session/schema.rs:2056` and `:2063` — the same
idempotency-key-with-unique-constraint pattern scoped by `run_id`.

---

## 10. Index-naming and bounded-index conventions

### 10a. Index naming

The dominant convention is **`idx_<table>_<column(s)>`**. Measured over
`crates/codegg-core/src/session/schema.rs`:
**164** `CREATE INDEX`/`CREATE UNIQUE INDEX` statements use the `idx_` prefix;
**24** use the legacy `<table>_<cols>_idx` suffix form.

The 24 legacy names (all from the v1–v15 era) are:
`session_project_idx`, `session_workspace_idx`, `session_parent_idx`,
`todo_session_idx`, `message_session_time_created_id_idx`,
`part_message_id_id_idx`, `part_session_idx`, `session_title_idx`,
`session_slug_idx`, `cached_models_provider_idx`, `session_time_updated_idx`,
`permission_time_idx`, `session_project_archived_idx`, `session_tags_idx`,
`part_type_idx`, `task_session_idx`, `task_parent_idx`,
`checkpoint_session_idx`, `snapshot_session_idx`, `usage_session_idx`,
`goal_session_status_idx`, `goal_project_status_idx`.

**Every new index must use `idx_<table>_<cols>`.** Examples:
`crates/codegg-core/src/session/schema.rs:3314` —
`idx_workstream_claim_group_state`; `:3315` — `idx_workstream_claim_owner`;
`:3317` — `idx_workstream_claim_active_target`; `:3334` —
`idx_workstream_claim_dependency_prerequisite`; `:3352` —
`idx_workstream_claim_event_group`; `:3532` —
`idx_agent_scratch_resource_owner`.

**Partial (filtered) unique indexes** are an accepted pattern:
`crates/codegg-core/src/session/schema.rs:3317-3320`:

```sql
        CREATE UNIQUE INDEX IF NOT EXISTS idx_workstream_claim_active_target
        ON workstream_claim(group_id, claim_kind, target)
        WHERE state IN ('offered','claimed','blocked')
```

and `crates/codegg-core/src/session/schema.rs:3335-3336`:

```sql
        CREATE UNIQUE INDEX IF NOT EXISTS idx_workstream_claim_dependency_claim ON workstream_claim_dependency(claim_id, prerequisite_claim_id) WHERE prerequisite_claim_id IS NOT NULL
```

### 10b. Bounded-index / bounded-payload conventions

**Schema-level bounds via `CHECK`** (each string column carries an explicit
length band):

| Column | Bound | Citation |
|---|---|---|
| `workstream_claim.target` | `length(target) BETWEEN 1 AND 256` | `crates/codegg-core/src/session/schema.rs:3300` |
| `workstream_claim.owner_id` | `length(owner_id) BETWEEN 1 AND 128` | `crates/codegg-core/src/session/schema.rs:3303` |
| `workstream_claim.idempotency_key` | `length(idempotency_key) BETWEEN 1 AND 128` | `crates/codegg-core/src/session/schema.rs:3307` |
| `workstream_claim.version` | `version >= 1` | `crates/codegg-core/src/session/schema.rs:3304` |
| `workstream_claim.lease_generation` | `lease_generation >= 1` | `crates/codegg-core/src/session/schema.rs:3305` |
| `agent_scratch_resource.alias` | `length(alias) BETWEEN 1 AND 32` | `crates/codegg-core/src/session/schema.rs:3523` |
| `agent_scratch_resource.idempotency_key` | `length(idempotency_key) BETWEEN 1 AND 512` | `crates/codegg-core/src/session/schema.rs:3524` |
| `agent_scratch_resource.revoked` | `revoked IN (0,1)` | `crates/codegg-core/src/session/schema.rs:3525` |

**Rust-side bounds mirror the SQL `CHECK`** (defense in depth):
`crates/codegg-core/src/workstream_claim.rs:514-515` compares
`input.idempotency_key.len()` against `MAX_CLAIM_IDEMPOTENCY_BYTES`.

**Bounded list queries clamp the limit.** Example:
`crates/codegg-core/src/session_control.rs:1020` clamps to `limit.clamp(1, 1000)`.
The queue store exports its own caps as public constants
(`crates/codegg-core/src/session/mod.rs:59-61`:
`USER_PROMPT_QUEUE_MAX_ENTRY_BYTES`, `USER_PROMPT_QUEUE_MAX_PENDING`,
`USER_PROMPT_QUEUE_MAX_RECONCILE`, `USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION`,
`USER_PROMPT_QUEUE_TTL_MS`) — the same "export the bound" convention used by
`crates/codegg-core/src/session/mod.rs:29-34` for the continuation store.

**No explicit "maximum index count" rule exists in the tree.** `UNPROVEN`:
I found no rule, guard, or doc statement bounding the *number* of indexes per
table or the total index count. The repo requirement is that each index is
justified by a named read path and that indexed payload columns are length-
bounded; there is no numeric index-count cap.

---

## 11. What does `scripts/check_queue_write_transaction_shape.py` enforce, and would a permit store trip it?

### 11a. What it enforces

**Scope: two files.** The queue store
(`scripts/check_queue_write_transaction_shape.py:39`):

```python
STORE = REPO_ROOT / "crates" / "codegg-core" / "src" / "session" / "user_prompt_queue.rs"
```

and the shared immediate-transaction primitive
(`scripts/check_queue_write_transaction_shape.py:41-48`):

```python
# The `BEGIN IMMEDIATE` primitive itself lives in one shared module. C002
# extracted it out of the queue store so the new writer-permit store could use
# the same mechanism instead of forking a subtle one — and this guard follows
# it, because the rollback contract is a property of the *primitive*, not of
# whichever store happens to call it.
IMMEDIATE_MODULE = (
    REPO_ROOT / "crates" / "codegg-core" / "src" / "storage" / "immediate.rs"
)
```

**Two needles** — `scripts/check_queue_write_transaction_shape.py:50-51`:

```python
DEFERRED_BEGIN = "self.pool.begin()"
IMMEDIATE_BEGIN = "begin_immediate("
```

**The write-SQL detector** — `scripts/check_queue_write_transaction_shape.py:70-73`:

```python
WRITE_SQL = re.compile(
    r"\b(?:INSERT\s+INTO|UPDATE|DELETE\s+FROM)\s+user_prompt_queue_",
    re.IGNORECASE,
)
```

**The checks it runs** — three, registered at
`scripts/check_queue_write_transaction_shape.py:252-256`:

```python
    checks = [
        ("queue write paths take the write lock at BEGIN", check_store),
        ("the immediate path issues BEGIN IMMEDIATE", check_immediate_is_immediate),
        ("an uncommitted queue transaction rolls back without closing", check_rollback_contract),
    ]
```

1. `check_store` (`scripts/check_queue_write_transaction_shape.py:156-164`)
   applies `_check_source` (`scripts/check_queue_write_transaction_shape.py:104-141`)
   to the queue store only. Within it:
   - Any function whose body contains `self.pool.begin()` and which is **not** on
     the allowlist is a violation (`:108-117`):
     ```python
             violations.append(
                 f"{name}() opens a deferred transaction. A read-then-write queue "
                 f"transaction cannot upgrade a shared read lock: SQLite returns "
                 f"SQLITE_BUSY (code 6) without waiting. Use {IMMEDIATE_BEGIN}."
             )
     ```
   - An allowlisted function that writes queue tables is a violation
     (`:118-122`).
   - A stale allowlist entry (a name that no longer opens a deferred
     transaction) is a violation — "An allowlist that lists code which does not
     exist is not a constraint." (`:127-133`).
2. `check_immediate_is_immediate`
   (`scripts/check_queue_write_transaction_shape.py:138-154`) reads
   `IMMEDIATE_MODULE` and requires the literal `BEGIN IMMEDIATE` in it — "This
   lives with the shared primitive, because that is where the statement is
   issued."
3. `check_rollback_contract`
   (`scripts/check_queue_write_transaction_shape.py:166-194`) also reads
   `IMMEDIATE_MODULE` and requires `impl Drop for ImmediateWrite`, a
   `start_rollback` call, and no `detach`:
   ```python
       if "impl Drop for ImmediateWrite" not in source:
           print("  FAIL: ImmediateWrite has no Drop, so a dropped future leaks its transaction")
           ok = False
       elif "start_rollback" not in source:
           print("  FAIL: ImmediateWrite::drop does not queue a rollback")
           ok = False
   ```

**The allowlist** — `scripts/check_queue_write_transaction_shape.py:63-66`:

```python
PERMITTED_DEFERRED = {
    "ownership": "read-only; writes nothing, so there is no read lock to upgrade",
    "list_live": "first statement is the expiry sweep, so it already takes the write lock",
}
```

**The guard has a `--self-test`** that injects a deferred read-then-write path
and a closing rollback to prove the guard rejects both
(`scripts/check_queue_write_transaction_shape.py:197-245` and following), per
`AGENTS.md:133-137`. Observed run of
`python3 scripts/check_queue_write_transaction_shape.py --self-test` in the
current tree:

```
  ok: injected deferred read-then-write path rejected
  ok: injected closing rollback rejected
  ok: unmodified tree still passes
```

and the plain run:

```
  ok: queue write paths take the write lock at BEGIN
  ok: the immediate path issues BEGIN IMMEDIATE
  ok: an uncommitted queue transaction rolls back without closing
check_queue_write_transaction_shape: all checks pass
```

### 11b. Would a new permit store trip it?

**Mechanically: NO.** The store-shape check reads exactly one file
(`scripts/check_queue_write_transaction_shape.py:39`) and its `WRITE_SQL`
regex only matches table names beginning `user_prompt_queue_`
(`scripts/check_queue_write_transaction_shape.py:71`). A permit store in a new
file with a new table name is invisible to that check.

**However, the guard now binds the permit store's transaction *mechanism*.**
Checks 2 and 3 read the shared primitive module
(`scripts/check_queue_write_transaction_shape.py:46-48`,
`:138-154`, `:166-194`), and the guard's own comment states the intent
(`scripts/check_queue_write_transaction_shape.py:41-45`):

> "C002 extracted it out of the queue store so the new writer-permit store could
> use the same mechanism instead of forking a subtle one — and this guard
> follows it, because the rollback contract is a property of the *primitive*,
> not of whichever store happens to call it."

**Therefore a C002 permit store must use
`codegg_core::storage::immediate::ImmediateWrite`** (or, at minimum, must not
fork a second `BEGIN IMMEDIATE` primitive that the guard does not cover).
Forking would leave the "the immediate path issues BEGIN IMMEDIATE" and
"rolls back without closing" checks passing while the permit store used a
different, uncovered mechanism.

**But the repo rule still binds C002**, and AGENTS.md states it as a general
rule for "a queue write path that reads before it writes". A C002 permit
acquisition is by construction a read-then-write path (read the current
permit/holder, then write the new permit/holder). The evidence in
`AGENTS.md:130-136` gives the concrete failure mode:

> "A deferred transaction takes no lock, so the first `SELECT` holds a shared
> read lock and the first `UPDATE` has to upgrade it — and SQLite refuses that
> upgrade **without waiting**, so two such callers can both fail with
> `SQLITE_BUSY` instead of one winning and the other waiting out
> `busy_timeout`. Measured at 204 of 256 concurrent claim rounds before C002."

**What C002's code must therefore do** (this is what the rule requires of any
read-then-write store path, whether or not the automated guard sees it):

1. Open the transaction with `BEGIN IMMEDIATE` — by calling
   `codegg_core::storage::immediate::ImmediateWrite::begin(&pool)`
   (idiom A, `crates/codegg-core/src/storage/immediate.rs:72-89`), **not**
   `pool.begin()`.
2. Do **not** fork a second `BEGIN IMMEDIATE` primitive; use the shared one
   so the guard's "issues BEGIN IMMEDIATE" and "rolls back without closing"
   checks actually cover the permit store's mechanism
   (`scripts/check_queue_write_transaction_shape.py:138-194`).
3. If the store keeps a deferred path, that path must be on an explicit,
   reasoned allowlist with a stated reason, and its first statement must
   already be a write (`scripts/check_queue_write_transaction_shape.py:53-66`).

**Optional but cheap:** if C002 adds its permit store under
`crates/codegg-core/src/session/`, extending the guard's `STORE` constant to
cover it would make the *store-shape* rule mechanically enforced rather than
conventional. That is a decision, not a requirement — the guard's `STORE` is a
single path today (`scripts/check_queue_write_transaction_shape.py:39`), and
its `WRITE_SQL` regex is queue-table-specific
(`scripts/check_queue_write_transaction_shape.py:70-73`).

---

## 12. What is the "project catalog invariant" check, and would adding a new table require touching it?

**The script:** `scripts/check_project_catalog_invariants.py`.

**Its seven checks** (`scripts/check_project_catalog_invariants.py:346-356`):

1. "Catalog module file exists"
2. "attach_locator only binds workspace for Local variant"
3. "No unwrap_or_default PathBuf anti-pattern"
4. "catalog/discovery migrations create tables"
5. "v28 migration adds 5 columns to logical_project"
6. "storage layout marker tracks highest wired schema migration"
7. "lib.rs re-exports project_catalog"

Observed run: `7/7 checks passed. All project catalog invariants verified.`

**Would adding a new table require touching it?**

- **The table itself: NO.** None of the seven checks enumerates the set of
  tables. Check 4 only asserts that the *catalog/discovery* migrations create
  their specific tables, and check 5 is pinned to v28's five columns. A new
  permit table in a new migration does not affect any check.
- **The migration version: YES.** If C002 adds `migrate_v84`, check 6
  compares `STORAGE_LAYOUT_VERSION` against the highest wired migration
  (`scripts/check_project_catalog_invariants.py:308-326`), so
  `crates/codegg-core/src/storage/mod.rs:42` must be bumped to `84` in the
  same change. The guard derives the highest version from three independent
  wirings and raises on disagreement
  (`scripts/check_project_catalog_invariants.py:36-48`), so a half-wired
  migration fails rather than passing vacuously.
- **The `tests/storage_migrations.rs` contract** also asserts a fully migrated
  database reports exactly `STORAGE_LAYOUT_VERSION`
  (`scripts/check_project_catalog_invariants.py:299-301`).

---

## Checklist a new durable permit store must satisfy

1. **Bump `STORAGE_LAYOUT_VERSION` in the same change as the migration.**
   `crates/codegg-core/src/storage/mod.rs:42` (`pub const STORAGE_LAYOUT_VERSION: u32 = 83;`)
   must equal the highest wired `migrate_vN` in
   `crates/codegg-core/src/session/schema.rs`. Enforced by
   `scripts/check_project_catalog_invariants.py:292-326` and by
   `tests/storage_migrations.rs` (`scripts/check_project_catalog_invariants.py:299-301`).
   *Note: this guard is manual — it is in neither `scripts/verify.sh` nor
   `.github/workflows/ci.yml`.*
2. **Wire the migration in all three places** so the guard's three-way
   derivation agrees: the `if current_version < N` chain
   (`crates/codegg-core/src/session/schema.rs:262-273`), the
   `N => migrate_vN(&mut tx).await?` dispatch arm
   (`crates/codegg-core/src/session/schema.rs:364-368`), and the
   `async fn migrate_vN(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<(), StorageError>`
   definition (`crates/codegg-core/src/session/schema.rs:3493`).
   `scripts/check_project_catalog_invariants.py:36-48` raises on disagreement.
3. **Make the migration additive and idempotent:** use
   `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`, run statements
   through the `for statement in [...]` loop, and map failures to
   `StorageError::Migration(error.to_string())`
   (`crates/codegg-core/src/session/schema.rs:3353-3358`, `:3557`). Use
   `add_column_ignore_duplicate` for `ALTER TABLE ADD COLUMN`
   (`crates/codegg-core/src/session/schema.rs:449-465`).
4. **Name everything by the established convention:** tables
   `snake_case` singular (`crates/codegg-core/src/session/schema.rs:3294`),
   columns `snake_case` with `*_id TEXT PRIMARY KEY` (`:3295`), timestamps
   `created_at`/`updated_at` `INTEGER NOT NULL` (`:3308-3309`), indexes
   `idx_<table>_<cols>` (`:3314-3315`, `:3532`; 164 of 188 in-tree indexes use
   this prefix; the 24 `<cols>_idx` names are legacy v1–v15 and must not be
   copied), foreign keys
   `FOREIGN KEY (col) REFERENCES table(pk) ON DELETE CASCADE` (`:3310`).
5. **Bound every persisted string in SQL and in Rust.** SQL:
   `CHECK (length(col) BETWEEN lo AND hi)` (`:3300`, `:3303`, `:3307`,
   `:3523-3524`); Rust: mirror the same cap as an exported constant
   (`:514-515`, `MAX_CLAIM_IDEMPOTENCY_BYTES`). Enumerations use
   `CHECK (col IN (...))` (`:3299-3302`); booleans use
   `INTEGER NOT NULL DEFAULT 0 CHECK (col IN (0,1))` (`:3525`). Clamp list
   limits (`:1020` pattern).
6. **Model the permit fence as a monotonic `version` plus a `generation`,**
   with `CHECK (version >= 1)` and `CHECK (lease_generation >= 1)`
   (`crates/codegg-core/src/session/schema.rs:3304-3305`), and bump them in the
   same `UPDATE` (`:781-783`).
7. **Do the compare-and-swap in ONE `UPDATE` whose `WHERE` clause carries the
   expected `version` **and** the expected generation, then check
   `rows_affected() != 1` and return a typed stale-revision error.**
   Reference: `crates/codegg-core/src/workstream_claim.rs:780-799`
   (with the pre-check at `:757-767`).
8. **Open the permit acquisition's read-then-write transaction with
   `BEGIN IMMEDIATE`, never `pool.begin()`.** `pool.begin()` is a *deferred*
   `BEGIN` in sqlx 0.8.6 (`sqlx-core-0.8.6/src/transaction.rs:277-283`), and a
   deferred read-then-write returns `SQLITE_BUSY` without waiting
   (`AGENTS.md:130-136`). Use the **shared** primitive
   `codegg_core::storage::immediate::ImmediateWrite::begin(&pool)`
   (`crates/codegg-core/src/storage/immediate.rs:72-89`, issuing the statement
   at `:80-84`); do not fork a second primitive. The raw-SQL form at
   `crates/codegg-core/src/workstream_claim.rs:523` is the other in-tree idiom
   but does not maintain sqlx's transaction depth counter
   (`crates/codegg-core/src/storage/immediate.rs:74-79`).
   **Do not copy `architecture/session.md:51-52`, which wrongly claims the
   schema migration uses `BEGIN IMMEDIATE`; it uses a deferred `pool.begin()`
   (`crates/codegg-core/src/session/schema.rs:279-282`).**
9. **Reuse the shared primitive's rollback `Drop` contract** rather than
   writing a new one: queue a rollback on the connection's worker thread and
   never close the connection
   (`crates/codegg-core/src/storage/immediate.rs:122-144`); the guard enforces
   that shape at `scripts/check_queue_write_transaction_shape.py:166-194`.
   `settled` is set *after* the commit returns, not before
   (`crates/codegg-core/src/storage/immediate.rs:91-101`).
10. **Make acquisition idempotent with a scoped, bounded idempotency key:**
    a `UNIQUE` constraint over the scope
    (`crates/codegg-core/src/session/schema.rs:3311`, `:3527`), a key
    lookup *before* any write, an unchanged return on an identical payload,
    and a typed conflict on a differing payload
    (`crates/codegg-core/src/workstream_claim.rs:576-597`). A retry with the
    same key must not create a second permit.
11. **Provide restart reconciliation** as an idempotent, bounded pass run at
    daemon recovery: mark nonterminal rows non-replayable
    (`crates/codegg-core/src/session_control.rs:744-758`) or delete orphaned
    rows by set-difference
    (`crates/codegg-core/src/agent_run.rs:2105-2111`). A permit left by a
    dead process must be reclaimable, and the reclamation must be safe to
    repeat.
12. **Construct the store as a thin `new(pool)` wrapper** and read state on
    demand or hydrate it explicitly at open
    (`crates/codegg-core/src/workstream_claim.rs:237-245` vs
    `crates/codegg-core/src/workspace.rs:427-441`); do not hold a stale
    in-memory copy across a restart.
13. **Use only the canonical pool entry points** — `init_migrated_daemon_catalog`
    for production bootstrap (`crates/codegg-core/src/storage/mod.rs:131-136`),
    `init_pool_at` for a pool at a path (`:175`); never the deprecated `init()`
    (`.opencode/skills/session-storage/SKILL.md:49-53`). Production pools get
    WAL, `foreign_keys(true)` and a 30 s `busy_timeout`
    (`crates/codegg-core/src/storage/mod.rs:255-257`).
14. **Test with `isolated_pool()`** (or the codegg-core local copy,
    `crates/codegg-core/tests/user_prompt_queue.rs:21-39`) and **never add an
    extra `migrate()` call** — the helper migrates
    (`.opencode/skills/session-storage/SKILL.md:44-48`).
15. **Keep `codegg-core` free of UI/server/plugin/auth dependencies.** The
    core-boundary guard is `scripts/check-core-boundary.sh`
    (`AGENTS.md:113-114`); a permit store living in `codegg-core` must not
    import server or TUI types.

---

## Conventions that would BLOCK a naive C002 implementation

1. **Writing the permit acquisition with `pool.begin()`.** sqlx 0.8.6's
   `pool.begin()` issues a plain deferred `BEGIN`
   (`sqlx-core-0.8.6/src/transaction.rs:277-283`). A read-then-write permit
   path on a deferred transaction returns `SQLITE_BUSY` *without waiting*
   (`AGENTS.md:130-136`). The permit acquisition must use the shared
   `codegg_core::storage::immediate::ImmediateWrite`
   (`crates/codegg-core/src/storage/immediate.rs:72-89`).
1b. **Forking a second `BEGIN IMMEDIATE` primitive** instead of using the
   shared one. The guard's rollback and "is actually immediate" checks read
   `IMMEDIATE_MODULE` only
   (`scripts/check_queue_write_transaction_shape.py:46-48`, `:138-194`), so a
   forked primitive would pass the guard while being uncovered by it — and the
   fork is exactly the "subtle mechanism" the extraction was meant to
   prevent (`scripts/check_queue_write_transaction_shape.py:41-45`).
2. **Following `architecture/session.md:51-52` literally.** It says every
   migration is "wrapped in an explicit `BEGIN IMMEDIATE` transaction" — the
   code uses a deferred `pool.begin()` (`crates/codegg-core/src/session/schema.rs:279-282`).
   The skill already corrected this
   (`.opencode/skills/session-storage/SKILL.md:150-153`); the architecture doc
   was not.
3. **Bumping only the migration and not `STORAGE_LAYOUT_VERSION`.**
   `scripts/check_project_catalog_invariants.py:308-326` fails on the
   mismatch. And because that guard is **not** wired into `scripts/verify.sh`
   or CI, a missing bump can reach a merge green — the change must be
   verified by running the guard explicitly.
4. **Adding the migration but not all three wirings.** The guard raises on
   any disagreement between the chain, the dispatch arm, and the definition
   (`scripts/check_project_catalog_invariants.py:36-48`).
5. **Using a deferred transaction anywhere in the permit read-then-write path
   without a reasoned allowlist entry.** The guard's rule
   (`scripts/check_queue_write_transaction_shape.py:18-24`) requires every
   deferred path to be named with its reason; the mechanical guard only covers
   `user_prompt_queue.rs` (`:39`), so the *convention* is what binds a new
   file.
6. **Reusing the legacy `<cols>_idx` index suffix.** 164 of 188 in-tree
   indexes use `idx_<table>_<cols>`; the 24 exceptions are v1–v15 legacy
   names.
7. **Persisting unbounded strings.** Every persisted text column in the
   reference tables carries a `CHECK (length(col) BETWEEN lo AND hi)`
   (`crates/codegg-core/src/session/schema.rs:3300`, `:3303`, `:3307`,
   `:3523-3524`) and the Rust side mirrors the cap
   (`crates/codegg-core/src/workstream_claim.rs:514-515`).
8. **Assuming the `crates/codegg-core/src/session_control.rs:607` deferred
   transaction is a sanctioned read-then-write pattern.** It is not the deadlock
   shape — its first statement is a `DELETE`
   (`crates/codegg-core/src/session_control.rs:608-613`), so it takes the write
   lock before reading. A C002 permit acquisition that must read the current
   holder first has a *read* as its first statement and therefore **cannot**
   use that shape.
9. **Calling `migrate()` inside a new test on top of `isolated_pool()`.**
   The helper already migrates
   (`crates/codegg-core/tests/user_prompt_queue.rs:35-37`);
   `.opencode/skills/session-storage/SKILL.md:44-48` forbids the extra call.
