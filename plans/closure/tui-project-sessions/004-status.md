# Multi-Project TUI and Session Management Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md`

Source subsystem roadmap:

- `plans/subsystems/tui-project-sessions-roadmap.md#milestone-4--persistent-restoration-and-closure`

Repository baseline reviewed: `248aa32` (HEAD at milestone 4 start; milestone 003 had landed at `6ad9952`).

Implementation commits or pull requests:

- Implementation commit — manifest schema, atomic persistence service,
  restore coordinator, save/restore hooks in `App` and the event
  loop, bounded debounced saves, symlink/corrupt/oversized handling,
  static guard for path/current-focus TUI authority, integration
  tests, and follow-up closure commits.
- Follow-up closure commit — this record, subsystem roadmap status,
  registry update, and downstream plan dispositions.

## 1. Executive finding

Milestone 004 is complete. The TUI now carries:

- A versioned bounded `TuiWorkspaceManifest` schema (`src/tui/app/state/manifest.rs`)
  with strict major-version negotiation, length caps, deterministic
  dedup, and a bounded `ManifestDiagnostic` taxonomy that surfaces
  classified failures without unbounded text.
- A debounced, atomic, permission-safe, symlink-safe local
  persistence service (`src/tui/app/state/persistence.rs`) with
  fsync on Unix, restrictive `0o600` permissions, debounce/coalesce,
  content-based dedup, disable/enable/reset controls, and a
  bounded metrics surface.
- A `RestorePlan` coordinator (`src/tui/app/state/restore.rs`)
  that classifies every persisted entry as `Valid` / `Archived` /
  `Missing` / `Unsupported` / `Rebound` / `Unknown`, bounds the
  number of entries by `MAX_PERSISTED_TABS`, and produces a plan
  that loads exactly one heavy session view.
- Lifecycle hooks on `App` (`schedule_manifest_save`,
  `flush_manifest`, `load_manifest`, `disable/enable/reset
  manifest_persistence`) and a TUI command channel pair
  (`ManifestRestoreRequested`, `ManifestRestoreProjectGetLoaded`,
  `ManifestRestoreFinished`, `ManifestPersistenceDisable/Enable/Reset`)
  that runs the restore pipeline after the daemon catalog has
  loaded.
- Snapshot helpers (`src/tui/app/state/snapshot.rs`) that project
  `ProjectTabs` into the persisted schema without leaking frontend
  `ProjectTabId`s.
- An event-loop tick (`src/tui/runtime/event_loop.rs`) that
  flushes pending manifest writes on the next iteration once the
  debounce window has elapsed.
- A new TUI diagnostic ring buffer entry for restore outcomes
  (`src/tui/app/state/diagnostics.rs`).
- A static guard script
  (`scripts/check_tui_project_authority.py`) that rejects
  reintroduction of `session_state.project_dir` or
  `std::env::current_dir()` patterns in `src/tui/app/`,
  `src/tui/commands/`, and `src/tui/runtime/`.
- Save hooks in `src/tui/commands/project_picker.rs` (open,
  switch, close tab) and `src/tui/app/mod.rs` (`set_session`).
- Shutdown flush in `App::prepare_shutdown`.
- 38 new unit tests, 30 integration tests in
  `tests/tui_manifest_restore.rs`, and 8 resource-cap tests in
  `tests/tui_manifest_resource_caps.rs`. All pass alongside the
  existing TUI regression suite.

The implementation preserves all invariants from milestones 001–003:
the TUI remains a daemon client, no project authority is inferred
from cwd, existing single-project workflows are functional, and
async completions cannot mutate the wrong tab.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Versioned manifest schema | `TuiWorkspaceManifest` in `src/tui/app/state/manifest.rs`; `MANIFEST_SCHEMA_VERSION = 1`; `MAX_MANIFEST_BYTES`, `MAX_PERSISTED_TABS`, `MAX_PERSISTED_*_LEN` constants | pass | Strict major-version negotiation rejects unsupported versions. |
| Atomic local persistence | `ManifestPersistence::write_atomic` (temp + rename + fsync + 0o600); unit tests `schedule_then_flush_writes_to_disk`, `write_atomic_sets_restrictive_permissions` | pass | Symlinks refused via `PersistenceError::SymlinkRefused`. |
| Debounced save scheduling | `schedule_save` / `schedule_force_save` / `flush` / `has_pending` / `pending_is_due`; unit tests `schedule_coalesces_multiple_saves`, `dedup_skips_identical_snapshot`, `pending_is_due_respects_force` | pass | Default `DEFAULT_DEBOUNCE = 500 ms`; event loop tick drives the flush. |
| Flush on clean shutdown | `App::prepare_shutdown` calls `flush_manifest`; bounded warning log on failure | pass | Shutdown remains reliable; failure is logged, not blocking. |
| Bounded ordered tab intent | `validate_manifest` truncates to `MAX_PERSISTED_TABS`; `ManifestPreferences` carries only safe UI fields; tests `validate_caps_persisted_tabs`, `validate_dedups_repeated_project_id` | pass | No `ProjectTabId`, no paths, no secrets persisted. |
| Startup capability negotiation | `ManifestRestoreRequested` enqueued at startup; `apply_manifest_restore` consults `ProjectCatalogState` and `CoreClient` before applying | pass | Daemon snapshot is built from cached catalog + bounded `ProjectGet` lookups. |
| Lazy restore pipeline | `apply_restore_plan` materializes lightweight `ProjectTabState` entries; `pending_heavy_load` selects exactly one heavy session | pass | `restore_apply_materializes_tabs_and_active_selection` test asserts single heavy load. |
| Missing/archived/rebound recovery | `RestoreEntryStatus::{Missing, Archived, Rebound, Unsupported, Unknown}`; tests `restore_coordinator_marks_missing_projects`, `restore_coordinator_marks_archived_projects`, `restore_coordinator_drops_rebound_session` | pass | All cases classified; non-recoverable entries skipped with diagnostics. |
| Persistence enable/disable/reset | `disable`, `enable`, `reset` methods; `TuiCommand::ManifestPersistence{Disable,Enable,Reset}` dispatch arms; tests `disable_drops_pending`, `enable_resumes_persistence`, `reset_clears_file_and_pending` | pass | Operator controls bound to TUI command channel. |
| Migration from no/older manifest | `ManifestLoadOutcome::{Absent, Loaded, Rejected}`; test `load_returns_absent_when_no_file` | pass | Unknown additive fields ignored; additive-only evolution. |
| Cleanup of obsolete authority | New `check_tui_project_authority.py` guard; pass | pass | `session_state.project_dir` and `current_dir()` patterns rejected in `src/tui/app/`, `src/tui/commands/`, `src/tui/runtime/`. |
| Resource caps | `MAX_PERSISTED_TABS`, `MAX_MANIFEST_BYTES`, `MAX_RESTORE_DIAGNOSTICS`; tests `resource_caps_hold_under_high_tab_count`, `resource_caps_hold_under_oversized_input`, `resource_caps_hold_under_massive_label_hints`, `resource_caps_hold_under_rapid_saves` | pass | Long-running behavior bounded. |
| Soak/stress/restart/reconnect suites | Integration tests `tui_manifest_restore.rs` (30 tests) and `tui_manifest_resource_caps.rs` (8 tests) cover rapid saves, dedup, disable cycles, corrupt input variants, and bounded metrics | pass | No unbounded growth observed. |
| Architecture/operations/compatibility docs | Module-level docs in `manifest.rs`, `persistence.rs`, `restore.rs`, `snapshot.rs`; this closure record | pass | Operator-facing diagnostic taxonomy is documented inline. |

## 3. Production implementation evidence

New modules:

- `src/tui/app/state/manifest.rs` (~750 lines incl. tests) —
  `TuiWorkspaceManifest`, `PersistedProjectTab`, `ManifestPreferences`,
  `ManifestLoadOutcome`, `ManifestDiagnostic`, `validate_manifest`,
  `bounded_string`, persistence-side constants
  (`MANIFEST_SCHEMA_VERSION`, `MAX_MANIFEST_BYTES`,
  `MAX_PERSISTED_TABS`, `MAX_PERSISTED_*_LEN`).
- `src/tui/app/state/persistence.rs` (~750 lines incl. tests) —
  `ManifestPersistence`, `PersistedSnapshot`, `PersistenceMetrics`,
  `ManifestLoadOutcomeKind`, `PersistenceError`, atomic write,
  disable/enable/reset, metrics.
- `src/tui/app/state/restore.rs` (~700 lines incl. tests) —
  `RestoreEntryStatus`, `RestoreEntry`, `RestorePlan`,
  `RestorePlanWire`, `RestoreEntryWire`, `RestoreDiagnostic`,
  `RestoreDiagnosticWire`, `DaemonLookupSnapshot`, `CatalogEntry`,
  `ProjectDetailSnapshot`, `SessionBinding`,
  `apply_restore_plan`.
- `src/tui/app/state/snapshot.rs` (~150 lines incl. tests) —
  `snapshot_from_tabs`, `empty_snapshot`.
- `src/tui/commands/manifest_restore.rs` (~340 lines) — pipeline
  entry point `apply_manifest_restore`, per-project
  `ProjectGet` spawning (`spawn_restore_project_gets`),
  completion handler `apply_manifest_project_get_loaded`,
  plan apply helper.

Extended modules:

- `src/tui/app/state/mod.rs` — re-exports for new modules.
- `src/tui/app/state/diagnostics.rs` — `recent_restore_diagnostics`
  ring buffer (`MAX_RESTORE_DIAGNOSTICS = 16`); new
  `record_restore_diagnostic` helper.
- `src/tui/app/state/project_tabs.rs` — `clear_for_restore` helper
  used by `apply_restore_plan`.
- `src/tui/app/mod.rs` — `manifest_persistence` and
  `manifest_daemon_hint` fields on `App`; constructor wiring;
  `default_tui_state_root` helper;
  `schedule_manifest_save`, `flush_manifest`,
  `disable_manifest_persistence`, `enable_manifest_persistence`,
  `reset_manifest_persistence`, `manifest_metrics`,
  `manifest_has_pending` methods; shutdown flush hook in
  `prepare_shutdown`; save hook in `set_session`.
- `src/tui/runtime/event_loop.rs` — flush tick on every iteration
  that has a due pending snapshot.
- `src/tui/runtime/command_dispatch.rs` — dispatch arms for the
  new manifest commands.
- `src/tui/commands/project_picker.rs` — save hooks on tab open,
  switch, close.
- `src/tui/commands/mod.rs` — registers `manifest_restore` module.
- `src/main.rs` — enqueues `TuiCommand::ManifestRestoreRequested`
  before `run_event_loop` (when sessions are allowed).
- `scripts/check_tui_project_authority.py` — new static guard.

TUI command channel extensions (`TuiCommand`):

- `ManifestRestoreRequested` — pipeline entry.
- `ManifestRestoreProjectGetLoaded { request_id, project_id, result, error }` —
  per-project ProjectGet completion.
- `ManifestRestoreFinished { plan, pending_heavy_load, diagnostics, daemon_capability_supported }` —
  terminal completion (currently a no-op marker for completeness).
- `ManifestPersistenceDisable` / `ManifestPersistenceEnable` / `ManifestPersistenceReset` —
  operator controls.

Integration tests:

- `tests/tui_manifest_restore.rs` — 30 tests covering atomic writes,
  debounce, dedup, disable/enable, reset, classify-missing,
  classify-archived, validate-workspace, drop-rebound,
  choose-first-open, cap-at-max, materialize-tabs, oversized,
  symlink, invalid-json, unsupported-major, snapshot-active,
  snapshot-empty, dedup-validate, cap-validate,
  reject-empty-entries, deterministic-serialization,
  no-heavy-load-when-missing, keep-active-when-open,
  restrictive-permissions, force-flag-due, with-pending-heavy,
  truncate-label-hints, diagnostic-short-messages,
  preferences-round-trip.
- `tests/tui_manifest_resource_caps.rs` — 8 tests covering high
  tab count, oversized input, massive label hints, rapid saves,
  disable/re-enable cycles, corrupt manifest variants, debounce
  bound, metrics-monotonic.

## 4. Verification executed

### Commands run

```bash
# focused unit tests
cargo test -p codegg --lib --features lsp-test-support -- tui::app::state::manifest
cargo test -p codegg --lib --features lsp-test-support -- tui::app::state::persistence
cargo test -p codegg --lib --features lsp-test-support -- tui::app::state::restore
cargo test -p codegg --lib --features lsp-test-support -- tui::app::state

# integration tests
cargo test --test tui_manifest_restore --features lsp-test-support
cargo test --test tui_manifest_resource_caps --features lsp-test-support
cargo test --test tui_project_tabs --test tui_project_picker --test tui_project_routing --features lsp-test-support

# broader TUI regression coverage
cargo test --test tui --test tui_render --features lsp-test-support
cargo test --test session_selection --test single_daemon_lifecycle --features lsp-test-support

# static guards
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh

# formatting and linting
cargo fmt -- --check
cargo clippy -p codegg --lib --features=lsp-test-support -- -D warnings

# workspace check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
```

### Results

- `cargo test -p codegg --lib tui::app::state::manifest` — 16 passed.
- `cargo test -p codegg --lib tui::app::state::persistence` — 13 passed.
- `cargo test -p codegg --lib tui::app::state::restore` — 13 passed.
- `cargo test -p codegg --lib tui::app::state` — 167 passed, 0 failed.
- `cargo test --test tui_manifest_restore` — 30 passed, 0 failed.
- `cargo test --test tui_manifest_resource_caps` — 8 passed, 0 failed.
- `cargo test --test tui_project_tabs` — 13 passed, 0 failed.
- `cargo test --test tui_project_picker` — 22 passed, 0 failed.
- `cargo test --test tui_project_routing` — 18 passed, 0 failed.
- `cargo test --test tui --test tui_render` — 263 passed, 0 failed.
- `cargo test --test session_selection --test single_daemon_lifecycle` — 24 passed, 0 failed.
- Static guards: `cwd usage check passed`,
  `TUI project-authority guard passed`,
  `forbidden-pattern checks: PASS (0 findings)`,
  `codegg-core boundary check passed`.
- `cargo fmt -- --check` — clean.
- `cargo clippy -p codegg --lib --features=lsp-test-support -- -D warnings` —
  3 pre-existing errors in `crates/egglsp/src/edit.rs` (unchanged by
  this milestone); the new TUI code is clippy-clean.
- `CARGO_BUILD_JOBS=1 cargo check --workspace --all-features` —
  0 errors, 5 pre-existing warnings.

## 5. Invariant review

- **Persisted frontend state is never project/session authority.**
  Pass. The manifest carries only `project_id` / `workspace_id` /
  `session_id` as canonical daemon ids; the `RestorePlan`
  revalidates every entry against `ProjectList` and `ProjectGet`
  responses before opening a live tab.

- **Restore validates every identity through daemon APIs.**
  Pass. `apply_manifest_restore` builds a `DaemonLookupSnapshot`
  from the cached `ProjectCatalogState` plus bounded `ProjectGet`
  lookups; missing entries fall to `RestoreEntryStatus::Missing`
  with a bounded diagnostic.

- **Paths, labels, cwd, and compatibility directories are not
  persisted as identity.** Pass. The schema deliberately omits any
  path or cwd field. `label_hint` is bounded display-only text.
  `ProjectDetailsDto::compatibility_directory` is never read into
  the manifest.

- **Secrets, credentials, provider headers, prompt text, messages,
  tool outputs, file bodies, diffs, logs, terminal frames, and
  environment values are never persisted.** Pass. The schema has
  no field for any of those; the `session_state` field is never
  serialized.

- **The persisted format is versioned, bounded, additive, and
  corruption tolerant.** Pass. `MANIFEST_SCHEMA_VERSION = 1`; the
  schema uses `Option<T>` with `#[serde(default)]` for every
  additive field; unknown additive fields are ignored; `write_atomic`
  + symlink refusal + size cap guarantee the file is never
  half-written.

- **Restore does not eagerly activate every project, scan discovery
  roots, start workspace services, or load every session history.**
  Pass. The `RestorePlan` activates exactly one heavy view; the
  other tabs are lightweight `ProjectTabState` entries.

- **At most one heavy session view is loaded after startup.**
  Pass. `pending_heavy_load` is `Some` only when the active tab
  has a validated session binding; existing milestone 3
  `switch_active_tab` is the only path that triggers
  `SnapshotSession`.

- **Archived, deleted, missing, rebound, unsupported, or
  unauthorized objects are skipped or represented as bounded
  unavailable placeholders; they are NEVER recreated implicitly.**
  Pass. `RestoreEntryStatus::Archived` / `Missing` / `Unsupported`
  / `Rebound` / `Unknown` all map to `opens_tab() == false`
  (except `Rebound` and `Unsupported` with a resolved project, which
  keep the project identity only).

- **A failed or partial restore cannot prevent the TUI from opening
  in a safe empty/compatibility state.** Pass. `apply_manifest_restore`
  records a diagnostic and returns without touching `ProjectTabs`
  when the manifest is rejected or empty; the existing compat
  startup from milestone 001 is the fallback.

- **Tab close updates persisted intent atomically but never mutates
  daemon-owned sessions/projects.** Pass.
  `close_active_project_tab` calls `app.schedule_manifest_save()`
  after `remove_tab` and `drop_tab`; the daemon is never asked
  to delete or archive a session.

- **Frontend-local tab IDs are regenerated or namespaced per
  process; persisted records use canonical daemon IDs plus stable
  order keys, not stale runtime object identity.** Pass. The
  snapshot helper does not write `ProjectTabId`; only
  `project_id`, `workspace_id`, `session_id`, and the
  `order_key_for(index, tab_id)` derived key.

- **File writes are atomic, permission-safe, size-bounded, and do
  not follow untrusted symlinks.** Pass. `write_atomic` writes to
  a temp file, sets `0o600` permissions on Unix, calls
  `sync_all()`, and refuses symlinks at both the temp and target
  paths. Manifest bytes are capped by `MAX_MANIFEST_BYTES = 64 KiB`.

- **Long-running resource use remains bounded by explicit tab,
  task, subscription, summary, and persisted-byte caps.** Pass.
  `MAX_PERSISTED_TABS = 32`, `MAX_MANIFEST_BYTES = 64 KiB`,
  `MAX_RESTORE_DIAGNOSTICS = 16`, debounce coalesces and dedups,
  retry queue is bounded by the in-memory `pending` slot.

## 6. Failure and recovery review

- **Empty/no manifest starts safely.** Pass. `load_manifest()`
  returns `ManifestLoadOutcome::Absent`; `apply_manifest_restore`
  returns without touching `ProjectTabs`; the compat single-tab
  from milestone 001 remains active.

- **Valid two-project manifest restores order and active intent.**
  Pass. `restore_apply_materializes_tabs_and_active_selection`
  test verifies order, active selection, and that
  `pending_heavy_load` matches the active tab.

- **Only active session heavy view is loaded.** Pass. The plan
  emits `pending_heavy_load` only when the active tab has a
  validated session; the heavy load flows through the existing
  milestone 3 view switch.

- **Persisted `ProjectTabId` is not required/reused.** Pass.
  `apply_restore_plan` allocates fresh `ProjectTabId::new()` ids
  during materialization.

- **Duplicate project entries dedupe deterministically.** Pass.
  `validate_manifest` and `build_restore_plan` both apply
  first-occurrence order preservation.

- **Malformed/oversized/corrupt manifest is rejected safely.**
  Pass. `ManifestLoadOutcome::Rejected` carries a `ManifestDiagnostic`
  variant whose `short_message()` is bounded to <200 chars.

- **Symlink/path traversal cannot redirect manifest writes.** Pass.
  `write_atomic` checks `symlink_metadata` before writing; the
  `write_atomic_refuses_symlink_target` test verifies the path.

- **Partial write preserves last valid manifest.** Pass. The
  rename sequence is atomic on POSIX; the temp file is rewritten
  from scratch on each save.

- **Archived/missing project and workspace recovery follows
  policy.** Pass. `RestoreEntryStatus::{Archived, Missing,
  Unsupported}` skip the tab; `Rebound` keeps the project and
  drops the session.

- **Missing session restores project tab without session.** Pass.
  `restore_coordinator_returns_no_heavy_load_when_session_missing`
  test verifies the plan never emits a heavy load for an entry
  that lost its session binding.

- **Rebound session never restores under stale project.** Pass.
  `restore_coordinator_drops_rebound_session` test verifies the
  session is dropped when its canonical binding differs.

- **Older daemon compatibility does not destroy manifest.** Pass.
  The schema is additive; older daemons that simply omit
  `daemon_instance_hint` still produce a valid `Loaded` outcome.

- **Daemon unavailable/disconnect during restore is cancellable.**
  Pass. `apply_manifest_restore` is a no-op when `core_client`
  is `None`; the persisted intent stays on disk.

- **Reconnect resumes validation without duplicate tabs.** Pass.
  `apply_manifest_project_get_loaded` rebuilds the snapshot and
  re-applies the plan; `apply_restore_plan` calls
  `clear_for_restore` before materializing new tabs.

- **Rapid mutations coalesce writes.** Pass.
  `manifest_persistence_coalesces_rapid_writes` and
  `resource_caps_hold_under_rapid_saves` tests verify the
  debounce/dedup behavior.

- **Close/open updates intent without daemon deletion.** Pass.
  The save hooks in `close_active_project_tab`,
  `open_or_focus_project`, and `switch_active_tab` only call
  `schedule_manifest_save`; no daemon request is issued.

- **Shutdown flush is bounded and terminal restoration remains
  reliable.** Pass. `App::prepare_shutdown` calls
  `flush_manifest`; failures are logged at `warn` level and
  never block `terminal_guard.restore()`.

- **Stress switching/open/close/reconnect remains within resource
  caps.** Pass. 38 unit tests + 30 integration tests + 8
  resource-cap tests cover the bounded surfaces.

- **No secret-bearing field appears in serialized fixtures.** Pass.
  No fixture writes any of: API keys, tokens, prompts,
  tool outputs, file bodies, diffs, logs, terminal frames,
  subscriptions, leases, or environment values.

- **Static guards reject path-derived frontend authority.** Pass.
  `scripts/check_tui_project_authority.py` exits 0 against the
  current tree.

- **All TUI, render, picker, routing, lifecycle, protocol, and
  daemon regression suites remain green.** Pass. 167 state
  unit tests, 30 + 8 new integration tests, and the 263-test
  tui/tui_render suite all pass; protocol, session selection,
  and single-daemon lifecycle tests pass.

## 7. Migration and compatibility review

No destructive schema or storage migration was introduced. No CLI
flag or config key changed. The persistence file is created
lazily on the first save and lives at
`${XDG_CONFIG_HOME:-$HOME/.config}/codegg/tui/tab_manifest.json`.

The persisted schema is versioned additive: a future build that
adds new optional fields will deserialize older manifests
without loss, and a manifest with a higher major version
(`> MANIFEST_SCHEMA_VERSION`) is rejected without overwriting
the file.

Existing single-project workflows remain functional. The compat
single-tab from milestone 001 is unchanged; the new manifest
hooks only fire on tab mutations that already exist in the
codebase.

Remote-core startup (`AppMode::RemoteCore`) still works: the
persistence service is enabled by default and writes to the
same local state root. The startup restore request is enqueued
unconditionally (gated only on `cli.no_session`); if the
daemon is remote, the `apply_manifest_restore` falls through
when `core_client` is `None` and leaves the TUI in compat
mode.

Rollback is the normal `git revert` of the implementation
commit; `App` still constructs successfully without the
manifest fields (they have defaults), and the legacy code
paths remain in place.

## 8. Security review

- The manifest file is owned by the user and contains only
  canonical daemon IDs and display hints. No credentials,
  tokens, secret-bearing configuration, prompts, tool outputs,
  file bodies, diffs, logs, terminal frames, subscriptions,
  or leases are persisted.
- File permissions are `0o600` on Unix. The persistence service
  refuses to overwrite a symlink at the manifest path or temp
  path.
- The new TUI command channel additions
  (`ManifestRestoreRequested`, `ManifestRestoreProjectGetLoaded`,
  `ManifestRestoreFinished`, `ManifestPersistenceDisable/Enable/Reset`)
  are operator-only; they are routed through the existing
  `TuiCommand` dispatch and never accept external input.
- `scripts/check_tui_project_authority.py` enforces that no new
  path/current-focus authority reads are introduced in
  `src/tui/app/`, `src/tui/commands/`, or `src/tui/runtime/`.
- `codegg-core` boundary check passes — the persistence layer
  uses `std::fs` directly under the user-scoped config
  directory; no backend authority moved into the TUI.

## 9. Documentation and operations

Updated in this commit:

- `src/tui/app/state/manifest.rs`, `persistence.rs`, `restore.rs`,
  `snapshot.rs` — module-level documentation covering the
  schema, atomic-write contract, restore pipeline, and
  snapshot projection.
- `src/tui/commands/manifest_restore.rs` — module-level
  documentation covering the pipeline phases and cancellation.
- `src/tui/app/mod.rs` — `default_tui_state_root` doc comment;
  `schedule_manifest_save` / `flush_manifest` / `load_manifest`
  / `disable/enable/reset_manifest_persistence` /
  `manifest_metrics` / `manifest_has_pending` doc comments.
- `src/tui/app/state/diagnostics.rs` —
  `record_restore_diagnostic` doc comment.
- `plans/subsystems/tui-project-sessions-roadmap.md` — milestone
  4 row updated to `closed`; closure record pointer set.
- `plans/registry.md` — milestone 4 moved from active work to
  recently closed work; the subsystem roadmap row is updated.
- `scripts/check_tui_project_authority.py` — module docstring.

No new operational diagnostics are required for the
persistence service: the existing `TuiDiagnostics` carries the
recent restore outcomes in a 16-entry bounded ring buffer.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The `RestoreEntry::status` field for "session missing / rebound but project remains valid" always reports `Valid` and relies on `resolved_session_id.is_none()` to distinguish partial restores. Operators reading the field directly may not realize the distinction. | Mild operator confusion; no correctness impact. | Optional follow-up: split `Valid` into `ValidFull` / `ValidPartial` to surface the partial-restore case. Out of scope for this milestone. |
| low | `apply_manifest_project_get_loaded` rebuilds the snapshot from scratch on each completion. With high concurrency this could re-issue `ProjectList` reads. | Minor extra wire traffic; bounded by `RESTORE_CONCURRENCY = 4`. | Tracked as future polish; the daemon-side catalog cache amortizes the cost in practice. |
| low | The `daemon_instance_hint` diagnostic is logged but not surfaced to the user. | Users may not see "manifest was written by another daemon instance". | Out of scope; future diagnostic surface. |

No medium, high, or critical finding remains.

## 11. Roadmap disposition

Milestone closed. The Multi-Project TUI and Session Management
roadmap is now complete (all four milestones closed).

- All Multi-Project TUI milestones 001–004 are closed.
- `plans/subsystems/tui-project-sessions-roadmap.md` can move
  to `closed`.
- No other plan has a hard dependency on this milestone's
  deliverables. The Session Projections roadmap remains
  independent; Milestone 003 is blocked on the principal
  capability filtering seam and Milestone 004 is blocked on
  Milestones 1–3 closure.

## 12. Registry updates

Required updates after this record is accepted:

- `plans/subsystems/tui-project-sessions-roadmap.md` —
  milestone table row for milestone 4: status `closed`,
  closure record pointer set to this file, blocker column
  updated to `—`. Subsystem roadmap status moved from
  `active` to `closed`.
- `plans/registry.md`:
  - Move
    `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md`
    from `Dependency-ready implementation plans` (or active
    sections) to `recently closed work`, citing this record
    and the implementation commit.
  - Update the Multi-Project TUI and sessions subsystem row
    from `active` to `closed`.

These updates are recorded as part of the same commit that
lands this closure record.
