# Multi-Project TUI and Session Management Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-project-sessions/001-project-aware-state.md`

Source subsystem roadmap:

- `plans/subsystems/tui-project-sessions-roadmap.md#milestone-1--project-aware-state-and-catalog-client`

Repository baseline reviewed: `8100c55` (commit on
`agent/project-catalog-lazy-activation-health` at the start of this
milestone; the implementation-plan baseline `fbae374a` predates the
project-catalog closure that this milestone depends on, so the
effective baseline is the catalog-closure commit on the active
branch).

Implementation commits or pull requests:

- `62e26b1` — multi-project TUI state seam and async project catalog
  client (`ProjectTabId`, `ProjectTabs`, `ProjectTabState`,
  `ProjectCatalogState`, async catalog command pair, active-tab
  accessors, compat startup, fake-client integration tests, TUI
  architecture documentation).
- Follow-up closure commit — this record, subsystem roadmap status,
  registry update, and downstream Session Projections unblock.

## 1. Executive finding

Milestone 001 is complete. The TUI now carries:

- A typed `ProjectTabId` and an ordered `ProjectTabs` collection with
  one designated active tab.
- A `ProjectTabState` per tab that holds the daemon-typed
  project/workspace/session ids and the per-tab selected model/agent.
- A bounded `ProjectCatalogState` cache plus the async command pair
  (`start_refresh_project_catalog` /
  `apply_project_catalog_refreshed`) that round-trips through
  `CoreClient` with explicit capability negotiation.
- Active-tab accessor methods on `App` that read through the new
  collection while preserving the legacy single-project fields.

The implementation ships in a strict compatibility mode: there is
exactly one tab at startup, the legacy `App::session_state`,
`App::agent_state`, and route surface continue to drive all current
rendering, and existing single-project workflows remain functional
without modification. The `Space f` picker, tab bar, and persistent
restoration are explicitly deferred to milestones 2–4.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `ProjectTabId` stable frontend-local identity | `src/tui/app/state/project_tabs.rs` `ProjectTabId::new`; unit tests `project_tab_id_is_unique` and `project_tab_identity_is_distinct_from_daemon_ids` | pass | UUID v4; never reused as daemon-typed ids. |
| Global vs. tab vs. session vs. modal state separation | `src/tui/app/state/project_tabs.rs`; `ProjectTabs::active`, `ProjectTabs::active_mut`, `App::active_tab`, `App::active_session_id` etc. | pass | `project_tabs` and `project_catalog` are top-level `App` fields. |
| Async project list/get through `CoreClient` | `src/tui/commands/project_catalog.rs`; tests `fake_client_handles_project_list_and_get_consistently` and `catalog_capability_supported_applies_list_completion` | pass | Round-trips `ProjectCatalogCapabilities` + `ProjectList`. `ProjectGet` is also exercised by the fake client to assert wire compatibility. |
| Per-tab request-generation scaffolding | `ProjectTabState::request_state`; `ProjectCatalogState::list_request`; unit tests `stale_catalog_completion_is_dropped`, `apply_refresh_with_stale_generation_is_dropped` (existing) | pass | `AsyncUiRequestState` provides monotonic ids. |
| Compatibility migration into one initial tab | `ProjectTabs::from_compat`; `App::with_config` and `App::new_for_testing` wire it; tests `app_always_has_one_compat_tab_after_new_for_testing`, `active_accessors_reflect_compat_state`, `setting_session_updates_active_tab` | pass | The compat tab inherits `project_dir` and the default model/agent. |
| Accessor/reducer seams | `App::active_tab`, `active_project_id`, `active_workspace_id`, `active_session_id`, `active_model`, `active_agent`, `open_tab_count`, `refresh_project_catalog`, `project_catalog_supported`, `apply_project_catalog_refreshed` | pass | All read through `project_tabs.active()`. `set_session` mirrors session id, project id, and workspace id into the active tab. `TuiMsg::SelectModel`/`SelectAgent` mirror model and agent. |
| Catalog loading/error/empty state suitable for future picker | `ProjectCatalogState::apply_list`, `apply_list_error`, `set_capability`, `clear`, `last_error`, `truncated` | pass | Stale completions are dropped at apply time. |
| Handle unsupported capability, empty catalog, errors, refresh | `start_refresh_project_catalog` routes the unsupported capability response to a sticky `capability_supported = false` state; test `catalog_capability_unsupported_keeps_compat_tab_usable` | pass | Unsupported capability keeps the compat tab usable and records no entries. |
| Avoid activation or detailed session loading during list | `start_refresh_project_catalog` only issues `ProjectCatalogCapabilities` + bounded `ProjectList`; never calls `SessionList`/`SessionGet` | pass | Catalog refresh is probe-free, matching the catalog invariant from milestone 003/004. |
| Catalog data remains bounded summaries | `ProjectCatalogState::entries: Vec<ProjectSummaryDto>`; default list limit `128` | pass | Matches `MAX_PROJECT_LIST_ITEMS` and the upstream plan. |
| Identity-correct async seams | `TuiCommand::ProjectCatalogRefreshed { request_id, ... }`; `AsyncUiRequestState::is_current` guards | pass | Tests `stale_catalog_completion_is_dropped` and `catalog_refresh_spawns_completion_via_spawn_registered_tui_task`. |
| Documentation updates | `architecture/tui.md` `## Multi-Project State (Milestone 1)`; existing `Async Command Pattern` updated to mention catalog command pair | pass | Documents state hierarchy, accessors, identity contract, and compatibility mode. |
| Static guards | `python3 scripts/check_daemon_cwd_usage.py`, `python3 scripts/check_git_forbidden_patterns.py`, `bash scripts/check-core-boundary.sh` | pass | No new cwd or forbidden-pattern uses; `codegg-core` boundary still clean. |
| Clippy clean | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass | No issues. |
| Format clean | `cargo fmt -- --check` | pass | No diffs. |
| TUI lib tests | `cargo test -p codegg --lib tui::` | pass | 585 passed. |
| Project-tab focused unit tests | `cargo test -p codegg --lib tui::app::state::project_tabs` | pass | 17 passed. |
| Project-tab integration tests (fake client) | `cargo test --test tui_project_tabs` | pass | 13 passed. |
| Existing TUI regression tests | `cargo test --test tui --test tui_render` | pass | 263 passed. |
| Protocol tests | `cargo test -p codegg-protocol` | pass | 92 passed. |
| Core / catalog tests | `cargo test -p codegg-core` | pass | 247 passed across 7 binaries. |
| Session selection and lifecycle tests | `cargo test --test session_selection --test single_daemon_lifecycle --test provider_connections_lifecycle` | pass | 188 passed. |

## 3. Production implementation evidence

New state and command modules:

- `src/tui/app/state/project_tabs.rs` — `ProjectTabId`, `ProjectTabState`,
  `ProjectTabs`, `ProjectCatalogState`. Re-exported via
  `src/tui/app/state/mod.rs`.
- `src/tui/commands/project_catalog.rs` — `start_refresh_project_catalog`
  and `apply_project_catalog_refreshed` (private; façade is
  `App::refresh_project_catalog` and `App::apply_project_catalog_refreshed`).

`App` integration (`src/tui/app/mod.rs`):

- New fields `project_tabs: ProjectTabs` and
  `project_catalog: ProjectCatalogState`. Both initializers
  (`with_config`, `new_for_testing`) construct one compatibility tab
  via `ProjectTabs::from_compat`.
- New accessor methods on `App`:
  `active_tab`, `active_tab_mut`, `active_tab_id`, `active_project_id`,
  `active_workspace_id`, `active_session_id`, `active_model`,
  `active_agent`, `open_tab_count`,
  `refresh_project_catalog`, `project_catalog_supported`,
  `apply_project_catalog_refreshed`.
- `App::set_session` now mirrors the selected session id, project id,
  and workspace id into the active tab.
- `TuiMsg::SelectModel` and `TuiMsg::SelectAgent` mirror the chosen
  model and agent into the active tab.

Dispatch (`src/tui/runtime/command_dispatch.rs`):

- `TuiCommand::RefreshProjectCatalog` → `start_refresh_project_catalog`.
- `TuiCommand::ProjectCatalogRefreshed { request_id, supported, entries,
  truncated, error }` →
  `apply_project_catalog_refreshed`.

`TuiCommand` variants (`src/tui/app/mod.rs`):

- Added `ProjectCatalogRefreshed { ... }` and `RefreshProjectCatalog`.

Startup wiring (`src/main.rs`):

- After `App::load_initial_session_via_core` returns, the main binary
  calls `App::refresh_project_catalog` so the catalog cache is
  populated asynchronously as soon as the event loop starts (skipped
  for `--no-session`).

Compatibility surface:

- The compat tab inherits `project_dir` as its label, the default
  model/agent at construction time, and acquires daemon-typed
  identities as soon as `set_session` is invoked. The legacy
  `App::session_state`, `App::agent_state`, `App::session_state.session`,
  and `Route::Session(id)` continue to drive every existing rendering
  path. No existing test was modified; the legacy `Route::Home` /
  `Route::Session` model is unchanged.

No new project discovery, registration, or storage authority was
introduced. The catalog cache only holds `ProjectSummaryDto` and is
populated by `CoreRequest::ProjectList`. `CoreRequest::ProjectGet` is
exercised by the fake-client test to assert wire compatibility for
the eventual picker.

## 4. Verification executed

### Commands run

```bash
# focused unit tests
cargo test -p codegg --lib tui::app::state::project_tabs
cargo test -p codegg --lib tui::

# focused integration tests (fake CoreClient, no daemon)
cargo test --test tui_project_tabs

# broader TUI regression coverage
cargo test --test tui
cargo test --test tui_render

# protocol + core + catalog baseline
cargo test -p codegg-protocol
cargo test -p codegg-core

# session selection + lifecycle regressions
cargo test --test session_selection
cargo test --test single_daemon_lifecycle
cargo test --test provider_connections_lifecycle

# lib unit suite (full)
CARGO_BUILD_JOBS=1 cargo test -p codegg --lib -- --test-threads=14

# static guards
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh

# formatting and linting
cargo fmt -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings

# workspace check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
```

### Results

- `cargo test -p codegg --lib tui::app::state::project_tabs` — 17 passed.
- `cargo test -p codegg --lib tui::` — 585 passed, 0 failed.
- `cargo test --test tui_project_tabs` — 13 passed, 0 failed.
- `cargo test --test tui` — 164 passed, 0 failed.
- `cargo test --test tui_render` — 99 passed, 0 failed.
- `cargo test -p codegg-protocol` — 92 passed, 0 failed.
- `cargo test -p codegg-core` — 247 passed (across 7 binaries), 0 failed.
- `cargo test --test session_selection --test single_daemon_lifecycle --test provider_connections_lifecycle` — 188 passed, 0 failed.
- `CARGO_BUILD_JOBS=1 cargo test -p codegg --lib -- --test-threads=14` — 3823 passed, 0 failed.
- Static guards: `cwd usage check passed`, `forbidden-pattern checks: PASS (0 findings)`, `codegg-core boundary check passed`.
- `cargo fmt -- --check` — no diffs.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — no issues.
- `cargo check --workspace --all-features` — succeeded.
- The verification list in the implementation plan included
  `cargo test --test core_transport` and `cargo test --test single_daemon`.
  In this repository the first does not exist as a test target
  (`cargo test --test core_transport` is a no-op with no matching
  binary), and the second was renamed to `single_daemon_lifecycle`.
  The actual equivalent was run as
  `cargo test --test single_daemon_lifecycle` and passed.

## 5. Invariant review

For each source-plan invariant:

- **The TUI remains a daemon client and never becomes storage authority.** Pass. The new `project_tabs` and `project_catalog` are populated exclusively from `CoreClient` responses. `set_session` continues to delegate to the daemon. `start_refresh_project_catalog` issues only wire requests (`ProjectCatalogCapabilities`, `ProjectList`); no local file or DB I/O is added. `codegg-core boundary check passed` confirms no new backend authority moved into `codegg-core`.

- **No project authority is inferred from TUI process cwd.** Pass. `App::project_tabs` is populated from `App::session_state.project_dir` (which is a CLI argument passed in by `main.rs`) and from the resolved session's `project_id` field. There is no new `std::env::current_dir()` use in TUI code; `scripts/check_daemon_cwd_usage.py` passes.

- **Existing single-project startup and session workflows remain functional.** Pass. The compat tab is constructed at the same sites that already initialized `App`. Every existing rendering path still reads through `App::session_state` and `App::agent_state`, which were not changed. `cargo test --test tui` (164 tests) and `cargo test --test tui_render` (99 tests) both pass without modification.

- **Async completions cannot mutate the wrong project/tab/session.** Pass. `TuiCommand::ProjectCatalogRefreshed` carries `request_id: u64`. `apply_project_catalog_refreshed` delegates to `AsyncUiRequestState::is_current` and drops stale results. Unit test `stale_catalog_completion_is_dropped` and integration test `catalog_refresh_spawns_completion_via_spawn_registered_tui_task` both exercise the guard.

- **Closing or replacing a UI view does not delete durable sessions.** Pass. `ProjectTabs::remove_tab` only mutates the frontend container. The fake-client integration test and the unit tests assert that `ProjectTabs::remove_tab` never reaches the daemon; no daemon-side call exists for tab removal.

- **Heavy details are not loaded for every catalog entry or inactive tab.** Pass. The catalog refresh issues a single bounded `ProjectList` with `limit: 128` and never fetches per-project detail during a list. `ProjectGet` is intentionally deferred to milestone 2 picker work; the fake-client test demonstrates the wire path is in place when needed.

- **Existing task lifecycle, stale-generation, modal priority, and terminal restoration behavior remain correct.** Pass. `App::task_registry`, `AsyncUiRequestState`, `FocusManager`, and dialog priority logic were not modified. `cargo test --test tui --test tui_render --test session_selection` exercises these.

## 6. Failure and recovery review

- **Catalog load failure leaves the current compatibility tab usable and records an actionable error.** Pass. `ProjectCatalogState::apply_list_error` stores the error message and clears the loading flag. Unit test `catalog_state_failure_records_error_and_resets_loading` exercises the path. The compat tab is independent of catalog state.
- **Unsupported project-catalog capability enters explicit single-project compatibility mode.** Pass. `apply_project_catalog_refreshed` flips `capability_supported` to `false` when the daemon returns `supported: false` and clears the loading flag without recording an error. Integration test `catalog_capability_unsupported_keeps_compat_tab_usable` exercises the path end-to-end through `App::refresh_project_catalog`.
- **Repeated catalog refresh invalidates older completions through request generations.** Pass. `start_refresh_project_catalog` calls `project_catalog.list_request.begin()` which bumps the generation; `apply_list` and `apply_list_error` both call `finish`/`fail` which return `false` for stale ids. Unit test `stale_catalog_completion_is_dropped` and the Git sidebar test `apply_refresh_with_stale_generation_is_dropped` confirm the guard.
- **Removing a tab invalidates tab-local UI requests but does not cancel daemon jobs.** Pass. `ProjectTabs::remove_tab` only mutates the container; the spawned task completes silently on its own and the apply methods drop stale results. No tab-removal daemon call exists in this milestone.
- **Reconnect clears/revalidates daemon-derived summaries.** Deferred to milestone 3 (event routing and lifecycle correctness). The catalog cache is already keyed to a request generation so a reconnect-induced refresh correctly invalidates prior results.
- **Several TUI clients remain independent.** Pass. No frontend tab lock is introduced; the catalog cache and tab container are per-`App`.

## 7. Migration and compatibility review

No destructive schema or storage migration was introduced. No CLI
flag, config key, or persisted file format changed. The new
`project_tabs` and `project_catalog` fields are populated in-memory
only. Legacy `App::session_state.project_dir`, `App::agent_state`,
and the `Route` model are untouched. Existing single-project rendering
continues to drive through the legacy fields, and every existing
TUI test passes without modification.

Older daemons that do not advertise `project_catalog.v1` (or do not
recognize the `ProjectCatalogCapabilities` request at all) enter the
unsupported-capability compatibility path: `capability_supported`
stays `false`, the catalog stays empty, and the compat tab continues
to function. Older daemons that do not recognize the request at all
return an error variant; `start_refresh_project_catalog` treats that
as "unsupported" and the same path applies.

Rollback is the normal `git revert` of the implementation commit;
the App still constructs successfully without the new fields
(although that state is not reachable on this branch).

## 8. Security review

- The catalog cache only contains `ProjectSummaryDto`. No credentials,
  tokens, secret-bearing configuration, or filesystem paths beyond
  the pre-existing `canonical_root` field are cached.
- `project_dir` continues to be a CLI-supplied locator; it is never
  derived from `std::env::current_dir()` and never treated as
  authoritative for project identity. `scripts/check_daemon_cwd_usage.py`
  passes.
- All catalog requests go through the existing `CoreClient` boundary
  with bounded limits (`limit: 128`, `MAX_PROJECT_LIST_ITEMS`) and
  capability negotiation. The TUI cannot bypass daemon checks to
  read or mutate catalog rows.
- `codegg-core` boundary check passes — no new dependency leaks.
- `scripts/check_git_forbidden_patterns.py` passes — no new git
  secret boundary or policy drift was introduced.
- The fake-client integration test asserts that the catalog client
  surfaces only what the daemon returns and never synthesizes
  hidden projects.

## 9. Documentation and operations

Updated:

- `architecture/tui.md` — added `## Multi-Project State (Milestone 1)`
  covering `ProjectTabs`, `ProjectCatalogState`, the active-tab
  accessor seam, the catalog async command pair, the identity
  contract, the compat startup path, the unsupported-capability
  fallback, and the TUI stats/debug summary extensions. Updated
  `State Domains` to reflect 8 state domains. Updated the
  `Async Command Pattern` section to list the new
  `RefreshProjectCatalog` / `ProjectCatalogRefreshed` pair.
- `plans/subsystems/tui-project-sessions-roadmap.md` — milestone 1
  status moves from `ready` to `closed`, pointing at this record.
- `plans/registry.md` — implementation plan removed from the
  dependency-ready table; closure row added under
  `recently closed work`.

No new operational diagnostics or static guards were required: the
existing `check_daemon_cwd_usage.py`, `check_git_forbidden_patterns.py`,
and `check-core-boundary.sh` already cover the failure modes this
milestone could introduce.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Tab-local `AsyncUiRequestState` is exposed on `ProjectTabState` but no tab-local async command consumes it yet. | Future tab-scoped refreshes (e.g. per-tab session list reload) must wire to it before shipping; otherwise it is dead weight. | Milestone 2 (picker, tabs, and per-project session selection) MUST consume `tab.request_state` for tab-local operations. |
| low | The `ProjectCatalogState` `last_error` is not yet surfaced in the TUI stats panel. | Operators cannot see why a catalog refresh failed without opening logs. | Milestone 4 (badges, diagnostics, and closure) MUST surface `project_catalog.last_error` and `capability_supported` in `/stats` or equivalent. |
| low | Remote TUI snapshots still represent one active view; tab labels and per-tab selection are not yet projected. | Remote clients cannot see which tab is active. | Deferred to Session Projections milestone 1 / Multi-Project TUI milestone 4; the active-tab accessor seam is already in place. |
| low | The `ProjectGet` async path is exercised only by the fake-client integration test; no production caller exists yet. | Production wiring happens at milestone 2 picker time. | Milestone 2 MUST add `start_get_project` and route it through the catalog command module. |

No medium, high, or critical finding remains. The low items are
declared downstream integration boundaries, not regressions or
authority violations.

## 11. Roadmap disposition

Milestone closed. The next hard dependency may proceed:

- Multi-Project TUI milestone 2 (picker, tabs, and per-project
  session selection) becomes dependency-ready as soon as its
  implementation plan is registered.
- Session Projections milestone 1 (projection contracts and
  canonical reducer) — previously blocked solely on this milestone —
  is now unblocked.

## 12. Registry updates

Required updates after this record is accepted:

- `plans/subsystems/tui-project-sessions-roadmap.md` milestone table
  row for milestone 1: status `closed`, closure record pointer set to
  this file, blocker column updated to `—`.
- `plans/registry.md`:
  - Move `plans/implementation/tui-project-sessions/001-project-aware-state.md`
    from the `Dependency-ready implementation plans` table to
    `recently closed work`, citing this record and the implementation
    commit.
  - Remove the Session Projections 001 blocker from the `Blocked
    work` table (or narrow its blocker to "implementation plan not yet
    registered" if that step is taken separately).
  - Optionally register a `dependency-ready` entry for Multi-Project
    TUI milestone 2 once its implementation plan exists.

These updates are recorded as part of the same commit that lands
this closure record.