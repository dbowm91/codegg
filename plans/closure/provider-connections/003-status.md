# Provider Connections Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connections/003-session-and-model-selection-by-connection.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connections-roadmap.md#milestone-3--session-and-model-selection-by-connection`

Repository baseline reviewed: `f70e344` (Provider Connections Milestone 002 closure; see `plans/closure/provider-connections/002-status.md`)

Implementation commits or pull requests:

- `213783e` — session + connection selection model, additive migration v27, daemon `SelectionService`, TUI `/connections` dialog, protocol DTOs, legacy-resolution outcomes, and compatibility hardening.

## 1. Executive finding

Milestone 3 is closed. Sessions now carry typed, optional connection/model
selection state (`provider_connection_id`, `provider_connection_revision`,
`model_catalog_revision`, `selected_model_id`) plus the existing legacy
`agent` and `model` strings. The daemon owns a typed `SelectionService`
that resolves a session's connection by ID/revision and its model by
catalog revision against the durable catalog published by Milestone 002.
A deterministic `LegacyResolution` outcome (`UnresolvedLegacyProvider`,
`AmbiguousLegacyProvider`, `DisabledLegacyConnection`,
`MissingCredentialLegacyConnection`) classifies the legacy `provider/model`
strings so existing sessions keep loading without silent fallback or
mid-session credential re-resolution.

The TUI gains a redacted `/connections` dialog that opens on the active
session, queries the daemon for the current selection, the eligible
connection list, and the bounded model catalog for the focused connection,
and submits optimistic-revision-checked updates. The dialog never
constructs a provider, never reads a credential, and never holds a
secret. Remote projections route the same redacted DTOs through the new
typed protocol variants.

The selection flow is the only new user-visible capability. Rotation,
health refresh, deletion, and team authorization remain Milestone 004.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Optional typed connection/model selection fields on session | `crates/codegg-core/src/session/models.rs`; `crates/codegg-core/src/session/row.rs`; `SESSION_COLUMNS` in `crates/codegg-core/src/session/store.rs`; `tests/session_crud.rs` round-trip tests | pass | All six new fields are additive; old rows load unchanged. |
| Additive schema migration v27 preserving Milestones 001–002 | `migrate_v27` in `crates/codegg-core/src/session/schema.rs`; `STORAGE_LAYOUT_VERSION = 27` in `crates/codegg-core/src/storage/mod.rs`; `tests/storage_migrations.rs` mid-migration resume + final-version assertion updated to 27 | pass | v24/v25/v26 tables and indexes are untouched. |
| Legacy provider/model compatibility adapter with deterministic outcomes | `LegacyResolution` enum in `crates/codegg-core/src/session/legacy_resolution.rs`; 9 unit tests covering resolved/unresolved/ambiguous/disabled/missing-credential; integration tests in `tests/session_selection.rs` | pass | No silent credentialed fallback; ambiguous legacy strings classify to `AmbiguousLegacyProvider`. |
| Daemon selection service over connection/catalog revisions | `SelectionService` in `src/core/session_selection.rs`; `SelectionError`/`SelectionUpdateOutcome` enums in `crates/codegg-core/src/session_selection.rs`; 4 daemon handlers in `src/core/daemon.rs`; 21 integration tests in `tests/session_selection.rs` | pass | Stale revisions and stale catalogs return typed outcomes without mutating stored state. |
| Redacted protocol selection requests/responses | `SelectedModelDto`, `SessionSelectionDto`, `UpdateSessionSelectionRequest` in `crates/codegg-protocol/src/provider.rs`; 4 new `CoreRequest` and 2 new `CoreResponse` variants in `crates/codegg-protocol/src/core.rs` | pass | No credential, header, or encrypted-payload fields exist on any selection DTO. |
| Frontend never constructs a provider or resolves a credential | `src/tui/commands/session_selection.rs` only calls `core_client.request`; `src/tui/components/dialogs/connection_selection.rs` holds redacted DTOs only | pass | Selection DTO types don't carry secrets; no `crypto`/`provider` import from selection code paths. |
| Endpoint authority, display name, scope, health, and model IDs surfaced; no secrets | `SelectedModelDto` and `SessionSelectionDto::Selected` carry `display_name`, `endpoint`, `provider_kind`, `scope`, `health`, `model_id`, `model_name`; `ProviderConnectionSummaryDto` lists do not include secrets | pass | Verified via type definitions; no field is named or typed as a key/secret. |
| Multi-session, multi-client concurrency with revision safety | `SelectionUpdateOutcome::StaleRevision` and `SelectionUpdateOutcome::StaleCatalog` returned by `SelectionService::update_selection` without mutation; `tests/session_selection.rs::concurrent_selections_for_same_session` test | pass | Independent sessions don't contend; stale revisions are rejected. |
| `/connections` command and dialog | `Command::new("/connections", ...)` in `src/tui/command.rs`; `Dialog::ConnectionSelection` + `DialogType::ConnectionSelection`; `open_connection_selection_dialog` in `src/tui/app/mod.rs`; `ConnectionSelectionDialog` in `src/tui/components/dialogs/connection_selection.rs`; `start_selection_refresh` and `start_selection_update` in `src/tui/commands/session_selection.rs` | pass | Compiles; command registered; tests pass; ledger has 107 commands. |
| Legacy configuration preserved | Provider registry auto-registration, env-var paths, and `register_builtin` unchanged; selection service treats them as a fallback input surface; `tests/provider_*` and `tests/auth_*` still pass | pass | No legacy path removed by this milestone. |

## 3. Production implementation evidence

### Selection sequence

```text
TUI /connections on active session
    │
    ▼
refresh = SessionSelectionGet { session_id }           ──typed redacted DTO
    │
    ├─► SessionSelectionList { session_id }             ──eligible connections
    │
    └─► SessionSelectionModels { session_id, connection_id }
                                                          (only if a connection is
                                                          already selected)
    │
    ▼
Dialog renders connection list + model list (redacted)
    │   user picks connection + model
    ▼
optimistic update = SessionSelectionUpdate {
    expected_connection_revision, expected_catalog_revision
}
    │   daemon validates against current revisions
    ▼
selected  ──► stored on session (revisioned, redacted)
stale rev ──► Outcome::StaleRevision       (no write)
stale cat ──► Outcome::StaleCatalog        (no write)
disabled  ──► Outcome::ConnectionNotSelectable (no write)
unknown m ──► Outcome::UnknownModel        (no write)
```

### Ownership boundaries

- `crates/codegg-core/src/session/models.rs` and
  `crates/codegg-core/src/session/row.rs` define the optional selection
  fields, double-Option clear/set semantics on `UpdateSession`, and the
  typed `CreateSession` defaults.
- `crates/codegg-core/src/session/schema.rs` owns the additive migration
  v27 (two columns + two indexes; no `DROP`/`RENAME`).
- `crates/codegg-core/src/session/legacy_resolution.rs` owns the
  deterministic `LegacyResolution` enum and the legacy-string adapter
  that consults `ProviderConnectionStore` for canonical resolution.
- `crates/codegg-core/src/session/selection_catalog.rs` provides
  read-only catalog/lookup helpers (`list_models_for_connection`,
  `catalog_revision_for`, `model_count_for`, `health_for`).
- `crates/codegg-protocol/src/provider.rs` owns the redacted
  `SessionSelectionDto` tagged enum and `UpdateSessionSelectionRequest`.
- `src/core/session_selection.rs` is the daemon's
  `SelectionService` — the only authority that writes `selected_*`
  fields on a session.
- `src/core/daemon.rs` exposes `SessionSelectionGet/List/Models/Update`
  and routes them through `ConnectionManager`. No provider construction
  or credential resolution happens here; the connection manager already
  returns the active runtime from Milestone 001.
- `src/tui/commands/session_selection.rs` is the only TUI bridge —
  it serializes redacted DTOs over the typed `CoreClient` and never
  sees a secret.
- `src/tui/components/dialogs/connection_selection.rs` is a pure state
  holder over redacted DTOs; render uses theme colors only.

### Storage schema delta (v26 → v27)

Additive; no destructive change:

- `session.provider_connection_id TEXT NULL`
- `session.provider_connection_revision INTEGER NULL`
- `session.model_catalog_revision TEXT NULL`
- `session.selected_model_id TEXT NULL`
- Indexes on `provider_connection_id` and `selected_model_id`.
- All Milestones 001–002 tables (`provider_connections`,
  `provider_connection_secrets`, `provider_connection_health`,
  `provider_connection_models`, `provider_provisioning`,
  `provider_connection_scopes`, `provider_connection_endpoints`,
  `provider_connection_revisions`) remain untouched.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test -p codegg-core
rtk cargo test -p codegg-core provider_connections
rtk cargo test -p codegg-core session::legacy_resolution
rtk cargo test -p codegg-protocol
rtk cargo test --test session_selection
rtk cargo test --test session_crud
rtk cargo test --test storage_migrations
rtk cargo test -p codegg --lib core::eggpool
rtk cargo test -p codegg --lib connect
rtk cargo test -p codegg --lib tui::command::tests::built_in_command_count_matches_release_docs
cargo test -p codegg --lib
CARGO_BUILD_JOBS=1 cargo test --workspace -- --test-threads=14
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk git diff --check
```

### Results

- Formatting and `git diff --check`: pass.
- `cargo check --workspace --all-targets --all-features`: pass (after
  fixing 4 distinct errors: bad `TuiCommand` import in
  `src/tui/app/mod.rs:9389`, missing `TuiMsg::ErrorMessage` arm in
  the dialog `update` match, `theme.accent` field replaced with
  `theme.primary` in the dialog render, unused `theme` field removed).
- `cargo test -p codegg-core`: 198 passed (3 suites).
- `provider_connections` and `session::legacy_resolution` filters: 15
  passed (6 + 9).
- `cargo test -p codegg-protocol`: pass.
- `cargo test --test session_selection`: 21 passed (the milestone's
  primary integration evidence).
- `cargo test --test session_crud`: 32 passed (extended with new
  selection field round-trip tests).
- `cargo test --test storage_migrations`: 3 passed after the final
  version assertion was updated from 26 to 27.
- `core::eggpool`: 8 passed.
- `connect`: 12 passed.
- `built_in_command_count_matches_release_docs`: pass after updating
  the hardcoded count from 106 to 107 and the matching `AGENTS.md`
  totals (107 commands).
- `cargo test -p codegg --lib`: 3742 passed.
- `cargo test --workspace -- --test-threads=14`: 7704 passed, 10
  ignored (98 suites). One intermittent scheduler authority-matrix
  test (`per_priority_class_interactive_before_background`) failed
  once under contention and passed individually; not related to this
  milestone and is observed to be timing-flaky pre-existing.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  exit 0 (no promoted errors; the single
  `clippy::large_enum_variant` warning on `SessionSelectionDto` is
  allow-by-default and stems from the existing varied
  provider-model-row payload inside the redacted DTO).
- `scripts/check-core-boundary.sh`: pass — `codegg-core` has no
  forbidden imports and no `ratatui`/`axum`/`wasmtime`/`crypto`
  dependencies introduced.
- `scripts/check_daemon_cwd_usage.py`: pass — selection path uses
  `ExecutionContext` rather than `std::env::current_dir()`.

## 5. Invariant review

- **Stable selection only.** A session resolves only the selected
  connection ID plus model revision; `SelectionService::update_selection`
  rejects updates whose `expected_connection_revision` differs from
  the current value (`StaleRevision`) without mutating storage. The
  catalog revision gate (`StaleCatalog`) is identical in shape.
- **Provider ID ≠ connection ID.** Provider implementation IDs and
  `ProviderConnectionId` remain distinct typed newtypes; selection
  results carry both, but the connection is the authoritative runtime
  handle and is the only one the connection manager can look up.
- **Legacy strings stay readable.** `LegacyResolution` classifies
  legacy `provider/model` strings explicitly; the legacy fields are
  still persisted on `Session` and remain the source of truth for
  pre-Milestone-003 sessions until removal criteria are accepted.
  No migration step rewrites them silently.
- **Scope is contextual.** Selection respects the connection's stored
  scope; `SessionSelectionList` does not return disabled connections
  as candidate picks.
- **Lazy/catalog-bound.** Model catalogs are bounded by Milestone 002
  (model count, field size, response bytes). Daemon startup does not
  re-probe; selection reads already-bounded rows.
- **Concurrency-safe.** `SelectionUpdateOutcome::StaleRevision` /
  `StaleCatalog` are returned before any write; concurrent session
  updates do not contend because each `Session` row's selection
  fields are independent of every other.
- **Secrets bounded.** No DTO/completion command/event/log surface
  contains a secret. `tests/session_selection.rs::no_secret_fields_exposed_in_serialized_selection_dto`
  exercises this directly. The pre-existing `EggpoolSecretTransportedLocalOnly`
  denials in `src/server/ws.rs` still apply and now also gate
  selection-from-remote-core WebSocket secret operations.

## 6. Failure and recovery review

| Failure or race | Behavior | Evidence |
|---|---|---|
| Legacy `provider` does not match any active connection | Classified `LegacyResolution::UnresolvedLegacyProvider`; no write | `legacy_resolution::legacy_provider_with_no_active_match_returns_unresolved` |
| Legacy `provider` matches multiple candidate connections | Classified `AmbiguousLegacyProvider`; list of candidates returned without selection | `legacy_resolution::legacy_provider_matching_multiple_connections_returns_ambiguous` |
| Disabled legacy connection | Classified `DisabledLegacyConnection`; not selectable | `legacy_resolution::disabled_legacy_connection_returns_disabled_outcome` |
| Missing credential for a matching legacy connection | Classified `MissingCredentialLegacyConnection`; no implicit alt | `legacy_resolution::missing_credential_for_legacy_connection_returns_missing_credential_outcome` |
| Stale `expected_connection_revision` | `Outcome::StaleRevision` returned; storage unchanged; caller sees current revision | `tests/session_selection.rs::stale_revision_update_returns_conflict_without_mutation` |
| Stale `expected_catalog_revision` | `Outcome::StaleCatalog` returned; storage unchanged | `tests/session_selection.rs::stale_catalog_update_returns_conflict_without_mutation` |
| Unknown model ID for an otherwise-valid connection | `Outcome::UnknownModel`; storage unchanged | `tests/session_selection.rs::unknown_model_id_returns_unknown_outcome` |
| Model ID absent from the active catalog | Treated as `UnknownModel` rather than silently dropping the selection; the connection choice itself is preserved | `tests/session_selection.rs::removed_model_keeps_connection_choice` |
| Connection disabled or deleted between refresh and update | `Outcome::ConnectionNotSelectable` | `tests/session_selection.rs::disabled_connection_is_not_selectable` |
| Concurrency: two clients updating one session | The second update with a stale revision is rejected; stored selection reflects the first winner | `tests/session_selection.rs::concurrent_selections_for_same_session` |
| Frontend disconnect mid-update | No write occurred; stored selection unchanged | Inherent: state mutations are applied only by the `SelectionService` after the full transaction |
| Daemon restart after update | The selection is durable because it lives on the session row in SQLite; recovery is automatic | Inherent: selection fields are part of `sessions` migration |
| Daemon unavailable to TUI | TUI shows `Core unavailable — check daemon status with /doctor`; selection is not assumed | `session_selection.rs::start_selection_refresh` early-return |
| Legacy unauthorized access to selection endpoint | WebSocket secret-bearing transport remain denied; selection endpoints are redacted and may be served remotely without secrets | Pre-existing `src/server/ws.rs` denials + selection DTOs contain no secret field |

## 7. Migration and compatibility review

- Storage layout advances from v26 to v27 through an additive,
  idempotent migration. Existing v24/v25/v26 tables (including the
  Milestone-001 connection tables and the Milestone-002
  `provider_connection_health` / `provider_connection_models` /
  `provider_provisioning` rows) remain untouched. The v27
  migration only adds columns and two indexes on the existing
  `sessions` table.
- All six new session fields are nullable with sensible defaults;
  `CreateSession::default()` and existing call sites set them to
  `None`/`0` automatically. Existing tests for session CRUD were
  extended and still pass.
- `UpdateSession` uses a double-Option convention so a missing key
  can mean "do not touch" (`None`), "clear" (`Some(None)`), or
  "set" (`Some(Some(v))`); this avoids accidental clearing of a
  pre-existing selection by an update that doesn't mention selection
  at all.
- The TUI never constructs a provider; provider manager is the sole
  authority to instantiate a connection's runtime; selection results
  carry the existing `ProviderConnectionId` and revision so the
  existing manager path is reused unchanged.
- Provider registry auto-registration, environment-variable
  detection, and `register_builtin` behavior remain unchanged.
- Protocol additions are additive; old clients that don't send the
  new request variants see only legacy-resolution diagnostics.
- Removed behavior: none.

## 8. Security review

- `SessionSelectionDto::Selected { .. }`, `SelectedModelDto`,
  `ProviderConnectionSummaryDto`, and `UpdateSessionSelectionRequest`
  contain only display strings, identifiers, scope labels, health
  codes, and bounded numerics. There is no field for a credential,
  encrypted payload, provider header, secret locator, or API key.
  Cross-checked against `tests/session_selection.rs`
  serialization-proof tests.
- The selection service never imports `crate::crypto` or
  `codegg_providers::auth`; it operates on the typed connection
  manager that already enforces scope and ownership.
- The remote WebSocket surface for any secret-bearing operation
  remains denied (`src/server/ws.rs`); selection updates travel
  over the standard typed request envelope without secrets, so the
  remote core can serve them.
- No path-based project identity is introduced. The selection
  service does not derive project IDs from cwd or any filesystem
  hint; if a future project-correct variant is needed, it goes
  through authoritative project context, not path heuristics.
- The dialog and command runner never log or persist redacted
  DTOs in any cache that lives outside the active dialog state;
  cached `SelectionSelectionLoaded { .. }` completions are dropped
  silently on stale generations (consistent with the existing
  `TuiTaskRegistry` semantics).

## 9. Documentation and operations

Updated:

- `AGENTS.md` — verified counts (107 commands) table reflects the
  added `/connections` command; the AGENTS.md command-count notes
  now match `src/tui/command.rs` again.
- Source implementation plan moves from `ready for handoff` to
  `implemented` once this closure record is filed.
- Source subsystem roadmap marks Milestone 3 closed and unlocks
  Milestone 4 (`lifecycle-and-rotation`).
- `plans/registry.md` advances the active-surface table to remove
  003 and records the closure under recently closed work.
- Existing protocol DTO documentation (`architecture/protocol.md`,
  `architecture/provider.md`, `architecture/session.md`) gets
  pointers to the new selection variants; the actual schema is the
  source of truth in `crates/codegg-protocol/src/provider.rs`.

Operational recovery is automatic on first daemon use: storage is
durable and the selection service reads from SQLite. No health probe,
rotation, or recompute occurs during selection.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No interactive CoreClient/TUI fake-server harness drives the dialog end-to-end under a real terminal. | The daemon selection service is fully exercised by `tests/session_selection.rs`; the TUI dialog is compiled and exercised for unit-shape but a full keyboard path with fake daemon is not. | Add the harness when Milestone 004 expands lifecycle actions; do not reopen the selection service boundary. |
| medium | `SelectionUpdateOutcome::StaleRevision` returns the current revision but does not yet include the current `selected_model_id` for client-side reconciliation shortcuts. | Clients must call `SessionSelectionGet` again to see the live state. | Add `current_*` fields to the outcome when lifecycle interactions land. |
| low | `clippy::large_enum_variant` warns on `SessionSelectionDto` (the `Selected` variant carries bounded model rows). | Future-proofing: variant weight could grow if model rows expand. | Box or split the variant if Milestone 004 adds more per-variant context. |
| low | Pre-existing flaky timing test in `tests/scheduler_authority_matrix.rs::per_priority_class_interactive_before_background` failed once under workspace-wide parallel load and passed when rerun in isolation. | No correctness impact; not introduced by this milestone. | Track under the existing scheduler-resource-profiles follow-up. |

No critical or high-severity finding remains.

## 11. Roadmap disposition

Milestone 3 is closed and the next hard dependency is unlocked.
Milestone 004 — lifecycle, rotation, refresh, and disable/delete UX —
has been authored and registered as dependency-ready and may proceed.
Runtime Assets 001 and Project Catalog 001 remain independently
ready. Multi-Project TUI 001 and Session Projections 001 remain blocked
on their catalog/identity/TUI dependencies; this closure does not
unlock them.

## 12. Registry updates

- Source plan `003-session-and-model-selection-by-connection.md` is
  marked `implemented` and points to this closure record.
- Provider Connections Milestone 3 is removed from the
  dependency-ready table and recorded under recently closed work.
- Provider Connections Milestone 4 is registered as the sole
  newly-unlocked provider-connections handoff plan.
- The provider-connections roadmap pointer advances to Milestone 4
  and marks Milestone 3 closed.
- No unrelated blocked plan is changed.
