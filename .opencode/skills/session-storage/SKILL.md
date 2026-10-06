---
name: session-storage
description: SQLite session persistence, storage layout, project catalog, and typed identity in codegg
version: 1.0.0
tags:
  - session
  - storage
  - catalog
  - identity
---

# Session and Storage Guide

Operational guide for changing persistence code. The full contracts live
in `architecture/session.md`, `architecture/storage.md`,
`architecture/project_catalog.md`,
`architecture/project_identity_storage.md`, and
`architecture/identity.md`; this skill covers the invariants that are
easy to violate.

Current schema, migration, table, and event totals are deliberately not copied
here. When a count matters, derive it from the owning source or the verified
census in `architecture/overview.md` rather than turning this operational guide
into a second source of truth.

## Ownership Map

| Layer | Location | Role |
|-------|----------|------|
| Session stores | `crates/codegg-core/src/session/store.rs`, `models.rs`, `message.rs`, `row.rs`, `status.rs`, `state.rs` | `SessionStore`, `TodoStore`, `MessageStore`, `PartStore`, `PermissionStore`, `UsageStore`, `EventStore`; `TuiSessionState` derived from events |
| Schema | `crates/codegg-core/src/session/schema.rs` | Sequential `migrate_vN` chain and schema declarations; each version runs in its own transaction via `migrate_and_record` |
| Storage | `crates/codegg-core/src/storage/mod.rs`, `paths.rs`, `preferences.rs` | Canonical `STORAGE_LAYOUT_VERSION`, pool init, WAL, `DaemonPaths`, `UserPreferences` |
| Events | `crates/codegg-core/src/session/events.rs` | Typed `SessionEvent` variants + `EventMeta`; the enum definition owns the current variant set |
| Catalog | `crates/codegg-core/src/project_catalog.rs`, `project_storage.rs` | Daemon-owned project list/get/register/archive/restore; path-independent identity |
| Identity | `crates/codegg-core/src/identity.rs` | Opaque string newtypes (UUIDv4), validated parsing, lexical contract |
| Run artifacts | `crates/codegg-core/src/run_store.rs`, `src/run_rerun.rs` | Persistent run index + artifact storage; rerun linkage |

## Hard Rules

1. **`STORAGE_LAYOUT_VERSION` tracks the highest migration.** When adding
   `migrate_vN`, bump the constant in the same change (guard:
   `scripts/check_project_catalog_invariants.py`).
2. **Storage tests use a pool helper, not a raw connection.** `shared_pool()`
   (process-wide, migrations once) or `isolated_pool()` (fresh in-memory DB
   per call) in `tests/common/pool.rs`; `codegg-core` integration tests
   carry local copies. Migrations run inside the helper; never add an extra
   `migrate()` call.
3. **Use only the canonical pool entry points.** `init_daemon_catalog`,
   `init_migrated_daemon_catalog` (production bootstrap authority),
   `init_legacy_project_store`, `init_migrated_legacy_project_store`,
   `init_pool_at`, and the migration-only `init_pool_at_for_migration`.
   Deprecated `init()` has no in-tree callers; new code MUST NOT use it.
   Note the deliberate split: `init_legacy_project_store` does **not** migrate,
   because `migrate_legacy_project_database` uses it to probe whether a file is
   a genuine legacy DB — migrating first would make the probe self-fulfilling.
   Consumers that need a migrated store must call
   `init_migrated_legacy_project_store`.
4. **Sessions resolve through workspace binding.** `CoreDaemon::
   bind_runtime_for_session` (`src/core/daemon_refresh.rs`) resolves
   `session_id` via `SessionStore` + `WorkspaceRegistry`; `TurnSubmit` and
   `AgentSelect` return `session_unbound` when binding fails. Never bypass
   with direct store access when a `CoreRequest` already exists.
5. **Exports are redacted and bounded.** `validate_import_size` +
   `redact_for_export` in `session/import.rs` are load-bearing.
6. **Working-file state is checksummed.** `compute_checksum` /
   `create_working_file` / `verify_file` in `session/checkpoint.rs` carry
   SHA-256 for `WorkingFile` entries on a `Checkpoint`. Note the boundary:
   `CheckpointStore::save`/`load` themselves do not verify checksums, and
   nothing in-tree calls `verify_file` yet.

## Static Guards

```bash
bash scripts/check-core-boundary.sh                # codegg-core stays UI/server/plugin/auth-free
python3 scripts/check_project_catalog_invariants.py # layout version + catalog invariants
```

## Testing

```bash
cargo test -p codegg-core session::
cargo test -p codegg-core storage::
cargo test --test checked_restore_integration
```

New `#[tokio::test]`s default to `current_thread`; use
`flavor = "multi_thread", worker_threads = 2` only for real
concurrency/subprocesses.

## See Also

- `architecture/session.md`, `architecture/storage.md`, `architecture/project_catalog.md`
- `.opencode/skills/core/SKILL.md` — session lifecycle on the core facade
- `.opencode/skills/bus-projection/SKILL.md` — durable replay vs derived views

## Source verification

Verified 2026-10-06 against `crates/codegg-core/src/session/{mod,store,models,message,row,status,state,schema,events,import,checkpoint}.rs`,
`crates/codegg-core/src/storage/{mod,paths,preferences}.rs`,
`crates/codegg-core/src/{project_catalog,project_storage,identity,run_store}.rs`,
`src/run_rerun.rs`, `src/core/daemon_refresh.rs`, `tests/common/pool.rs`,
`scripts/check-core-boundary.sh`,
`scripts/check_project_catalog_invariants.py`, and
`tests/checked_restore_integration.rs`. Corrected the schema row, which
claimed each migration runs in `BEGIN IMMEDIATE` (it runs in a plain
`pool.begin()` transaction in `migrate_and_record`); the pool rule, which
called deprecated `init()` tests-only when it has no in-tree callers at
all, and omitted `init_pool_at_for_migration`; the checkpoint rule, which
implied `CheckpointStore` verifies checksums on restore (it does not, and
nothing in-tree calls `verify_file`); the workspace-binding rule, which
included `ModelSelect` among the unbound-rejecting requests; and the
`isolated_pool()` rule, which named only one of the two helpers in
`tests/common/pool.rs`. Confirmed accurate as written: `STORAGE_LAYOUT_VERSION`
= 68 tracking `migrate_v68`, migration v22 as the workspace-table
migration, all seven store types, `SessionEvent`/`EventMeta`,
`ProjectCatalog`, `ProjectStorage`, the `typed_identity!` macro,
`validate_import_size`/`redact_for_export`, `RunStore`, the rerun linkage
marker, and all three `cargo test` targets. Claims without a traceable
source were removed rather than guessed.

Third pass: added `init_migrated_legacy_project_store` to the pool rule and
recorded why the non-migrating initializer is deliberately kept separate —
`migrate_legacy_project_database` calls `init_legacy_project_store` to probe
whether a file is a genuine legacy DB, so migrating there would make the probe
self-fulfilling. All six canonical entry points verified present in
`crates/codegg-core/src/storage/mod.rs` (`:119`, `:130`, `:144`, `:163`,
`:175`, `:226`); `init()` at `:297` confirmed to have no in-tree callers; and
`verify_file` confirmed to be defined and re-exported
(`session/mod.rs:25`) but never called in-tree. Normalized `.skills/` skill
cross-references to `.opencode/skills/`.
