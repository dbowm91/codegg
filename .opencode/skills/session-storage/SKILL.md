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
| Schema | `crates/codegg-core/src/session/schema.rs` | Sequential `migrate_vN` chain and schema declarations; each migration in `BEGIN IMMEDIATE` |
| Storage | `crates/codegg-core/src/storage/mod.rs`, `paths.rs`, `preferences.rs` | Canonical `STORAGE_LAYOUT_VERSION`, pool init, WAL, `DaemonPaths`, `UserPreferences` |
| Events | `crates/codegg-core/src/session/events.rs` | Typed `SessionEvent` variants + `EventMeta`; the enum definition owns the current variant set |
| Catalog | `crates/codegg-core/src/project_catalog.rs`, `project_storage.rs` | Daemon-owned project list/get/register/archive/restore; path-independent identity |
| Identity | `crates/codegg-core/src/identity.rs` | Opaque string newtypes (UUIDv4), validated parsing, lexical contract |
| Run artifacts | `crates/codegg-core/src/run_store.rs`, `src/run_rerun.rs` | Persistent run index + artifact storage; rerun linkage |

## Hard Rules

1. **`STORAGE_LAYOUT_VERSION` tracks the highest migration.** When adding
   `migrate_vN`, bump the constant in the same change (guard:
   `scripts/check_project_catalog_invariants.py`).
2. **Storage tests use `isolated_pool()`.** Migrations run inside; never
   add an extra `migrate()` call.
3. **Use only the canonical pool entry points.** `init_daemon_catalog`,
   `init_migrated_daemon_catalog` (production bootstrap authority),
   `init_legacy_project_store`, and `init_pool_at`. Deprecated `init()` is
   tests-only; new code MUST NOT use it.
4. **Sessions resolve through workspace binding.** `CoreDaemon::
   bind_runtime_for_session` resolves `session_id` via `SessionStore` +
   `WorkspaceRegistry`; `TurnSubmit`/`AgentSelect`/`ModelSelect` reject
   unbound sessions. Never bypass with direct store access when a
   `CoreRequest` already exists.
5. **Exports are redacted and bounded.** `validate_import_size` +
   `redact_for_export` in `session/import.rs` are load-bearing.
6. **Checkpoints carry SHA-256.** `CheckpointStore` checksums in
   `session/checkpoint.rs`; verify on restore.

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
- `.skills/core/SKILL.md` — session lifecycle on the core facade
- `.skills/bus-projection/SKILL.md` — durable replay vs derived views
