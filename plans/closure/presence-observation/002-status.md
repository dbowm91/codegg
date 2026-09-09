# Presence and Observation Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/presence-observation/002-tui-collaborator-presence-surface.md`

Source subsystem roadmap:

- `plans/subsystems/presence-observation-roadmap.md#M002--TUI-collaborator-and-presence-surface`

Repository baseline reviewed: `bc29a9a6b0e9776397a1eb653e17d5523379b9254`

Implementation commits or pull requests:

- `bc29a9a6` — feat(presence): M002 TUI collaborator presence surface (reducer, header/panel, refresh, 10 unit + 12 integration tests, tui/presence docs).

## 1. Executive finding

M002 is closed. Authorized daemon-owned presence renders in each
project tab as a bounded header count and a collaborator panel with
stable ordering, coarse activity labels, and explicit
empty/loading/error/unavailable states. Switching, reconnect, expiry,
and privacy behave per plan: stale completions drop, resync replaces
stale presentation from the authoritative snapshot, rapid switching
cannot leak across projects, and unauthorized or feature-absent data
renders identically (hidden) with no local-cache leakage. The TUI owns
no presence truth and consumes only the M001 capability
(`PresenceCapabilities` + `PresenceSnapshotGet` + `PresenceUpdated`
hint shape). No unresolved high, medium, or low M002 finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Presence state adapter/reducer keyed by project + sequence/generation (package A) | `src/tui/app/state/presence.rs`: `PresenceState` keyed by `project_id`, per-project `request_id` + global `reconnect_epoch` stale guards, local `sequence` ordering, `needs_resync` flag, LRU eviction at 16 projects | pass | `ProjectTabId` never appears in presence keys. |
| Project header count/status (package B) | `App::render_header_content` + `PresenceState::header_summary`: `👥 N` (+ `· agent running` / `(stale)`) only for authorized `Ready` data; `None` (hidden) for loading/unavailable/unauthorized | pass | Hidden projects never leak through the header. |
| Collaborator panel/list with stable ordering, bounds, empty/loading/error states (package B) | `PresenceState::panel_lines` + `InfoType::Collaborators` dialog via `/collaborators`: activity-rank + id order, 32-row display bound with `+N more`, per-row `display_line` (identity + session/client counts + coarse status), empty/loading/error/unavailable branches | pass | Labels coarse (`active`/`idle`/`observing`/`agent running`); no reasoning, prompts, content, or secrets. |
| Activity labels coarse and bounded (package B) | `activity_label`, `display_principal` (48-char bound), `CollaboratorEntry::display_line`; `activity_labels_are_coarse_and_bounded` test | pass | Handoff note enforced. |
| Navigation/focus without hard-coding (package C) | `/collaborators` (`/presence`, `/team`, `refresh` subcommand) in `src/tui/command.rs` + `execute_command` arm; standard info-dialog focus (`j`/`k` scroll, `Esc`/`Enter` close); help entries in `src/tui/input.rs`; no `Space p` hard-code (command-registry convention differs) | pass | Opening never mutates sessions (pinned by focus test). |
| Reconnect/project-switch/privacy regression + docs (package D) | `switch_active_tab` (both `App` + picker paths) bounded refresh; tab-close `clear_project` when unheld; `on_projection_reconnect` bumps presence + routing epochs and re-fetches active; `on_presence_hint` coalescing seam; `architecture/presence.md` M002 section + `architecture/tui.md` collaborator section | pass | No polling storm; no task per update. |
| Protocol: consume M001 capability only | `src/tui/commands/presence.rs` uses only `PresenceCapabilities` + `PresenceSnapshotGet`; `PresenceHint` carries only `project_id` (same shape as `PresenceUpdated`) | pass | No new protocol; old daemons degrade to unavailable. |
| Security: never render hidden data after auth loss | `apply_error(unauthorized)` clears principals to `Unavailable`; `clear_project` on tab close/auth loss; header/panel identical for unauthorized vs absent (pinned) | pass | See §8. |
| Docs: keybindings/help | `src/tui/input.rs` `/collaborators` entries; `/collaborators` registry description; architecture docs above | pass | — |

## 3. Production implementation evidence

- `src/tui/app/state/presence.rs` (new, ~830 lines with 10 tests):
  `activity_label` / `activity_rank`, `display_principal` (48-char),
  `CollaboratorEntry::display_line`, `PresenceStatus`
  (`Unknown`/`Loading`/`Ready`/`Unavailable`/`Error`),
  `ProjectPresence` (principals sorted, `truncated`, `as_of_ms`,
  `sequence`, `current_request_id`, `needs_resync`), `PresenceState`
  (16-project LRU, `capability_supported`, `reconnect_epoch`,
  `begin_refresh` / `needs_refresh` / `apply_snapshot` with
  project-routing guard / `apply_error` with unauthorized vs transient
  split / `note_hint` coalescing / `on_reconnect` / `clear_project` /
  `header_summary` / `panel_lines` with 32-row + 8-session display
  bounds).
- `src/tui/app/state/mod.rs`: `pub mod presence` + re-exports.
- `src/tui/commands/presence.rs` (new): `start_refresh_presence`
  (capability negotiation + snapshot in one `TuiTaskKind::Command`
  registered task), `apply_presence_snapshot_loaded` (stale/epoch
  guards), `show_collaborators` (fetch-on-demand + info dialog),
  `refresh_collaborators` (explicit re-fetch). No polling loop; hints
  never spawn a task per update.
- `src/tui/app/mod.rs`: `App.presence: PresenceState` (both
  constructors); `TuiCommand::RefreshPresence` /
  `PresenceSnapshotLoaded` / `PresenceHint`; `refresh_presence` /
  `apply_presence_snapshot` / `presence_supported` /
  `show_collaborators` / `refresh_collaborators` façades;
  `render_header_content` presence span (hidden when unavailable);
  `/collaborators` (`/presence`, `/team`, `refresh`) arm;
  `set_session` presence refresh; `switch_active_tab` presence refresh;
  `on_projection_reconnect` (presence + routing epoch bump + active
  re-fetch); `on_presence_hint` seam.
- `src/tui/commands/project_picker.rs`: `switch_active_tab` bounded
  refresh for the new active project; `close_active_project_tab`
  drops unheld projects.
- `src/tui/runtime/command_dispatch.rs`: `RefreshPresence` /
  `PresenceSnapshotLoaded` / `PresenceHint` arms (`PresenceHint`
  auto-refreshes only the active project; inactive tabs refresh on
  foreground).
- `src/tui/command.rs`: `/collaborators` (`/presence`, `/team`)
  built-in (count 113 → 114).
- `src/tui/components/dialogs/info.rs`: `InfoType::Collaborators`
  (` Collaborators ` title).
- `src/tui/components/component.rs`: `DialogType::Collaborators`
  (focus slot; maps to `Dialog::None` since info dialogs render via
  the focus stack).
- `src/tui/input.rs`: `/collaborators` + `/collaborators refresh`
  help entries.
- `tests/presence_m002_collaborators.rs` (new, 12 tests): multi-project
  routing, ordering/bounds, stale drop, reconnect/resync (reducer +
  app epochs), unauthorized == absent identical rendering, feature-absent
  hides-without-breaking-tabs, old-daemon round-trip, bounded snapshot
  round-trip, identical names across projects, dialog focus without
  session mutation, inactive-tab bound.
- Docs: `architecture/presence.md` M002 section; `architecture/tui.md`
  collaborator-presence section.

Distinguished as absent (downstream, not M002 scope): observation action
implementation (M003 authorized read-only observation over projection
replay), project chat, membership administration, avatars/general
social UI, live `CoreEvent::PresenceUpdated` transport forwarding into
the TUI event loop (the `on_presence_hint` / `PresenceHint` seam is the
bounded entry point; current refreshes are driven by open/switch/
reconnect/user actions with hint coalescing, creating no second
streaming stack).

## 4. Verification executed

All local (no hosted `CI / verify` claimed).

### Commands run

```bash
cargo test --workspace tui --no-fail-fast
cargo test --workspace presence --no-fail-fast
cargo test -p codegg --lib tui::app::state::presence
cargo test --test presence_m002_collaborators
cargo test --test presence_m001_leases
cargo test --test tui_render
cargo test --test tui
cargo test --test tui_project_tabs
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_authorization_matrix.py
bash scripts/check-core-boundary.sh
bash scripts/check_projection_disclosure.sh
python3 scripts/check_tui_project_authority.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
scripts/verify.sh quick
```

### Results

- `cargo test --workspace tui --no-fail-fast`: pass (no failures;
  workspace-wide run filtered by `tui`).
- `cargo test --workspace presence --no-fail-fast`: pass (no failures;
  workspace-wide run filtered by `presence`).
- `cargo test -p codegg --lib tui::app::state::presence`: 10 passed.
- `cargo test --test presence_m002_collaborators`: 12 passed
  (multi-project routing, ordering/bounds, stale, reconnect/resync ×2,
  unauthorized/absent identical, feature-absent, old-daemon round-trip,
  bounded snapshot round-trip, identical names, focus/key, inactive bound).
- `cargo test --test presence_m001_leases`: 15 passed (M001 regression
  still green).
- `cargo test --test tui_render`: 99 passed (no render regression;
  header presence span is additive and hidden when unavailable).
- `cargo test --test tui`: 164 passed.
- `cargo test --test tui_project_tabs`: 20 passed.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass.
- `check_authorization_matrix.py`: all invariants verified (no matrix
  change; M002 consumes the M001 `project.observe` gate).
- `check-core-boundary.sh`: passed.
- `check_projection_disclosure.sh`: OK.
- `check_tui_project_authority.py`: passed.
- `check_execution_ownership.py`: ok (no new spawn site; presence uses
  the existing `spawn_registered_tui_task` command path).
- `check_scheduler_bypass.py`: ok.
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution-ownership,
  `cargo check --workspace --all-targets --locked`).

No verification was skipped without a substitute. Plan §11 lists
`cargo test --workspace tui` / `presence` as bare filters; both were run
exactly as written above (they are workspace-wide filtered runs, not
single-crate selections).

## 5. Invariant review

| Plan invariant | Evidence | Result |
|---|---|---|
| No TUI-side authority or durable presence | Reducer is a pure projection of `PresenceSnapshotDto`; no `PresenceService` in TUI; no storage migration; `clear_project`/`clear_all` drop on close/loss; no presence read in any authorize path | pass |
| Events route to the correct project tab | `apply_snapshot` rejects `snapshot.project_id != project_id`; header/panel read only the active `project_id`; `multi_project_routing_*` + switch-refresh hook | pass |
| Inactive tabs remain bounded | 16-project LRU (`evict_if_needed`); 32-row panel bound; `inactive_*_bounded` tests | pass |
| Unauthorized/absent data renders identically | `Unavailable` clears principals for both; `panel_lines` identical; `header_summary` hidden (`None`) for both; pinned by reducer + integration tests | pass |
| Focus/key behavior does not mutate sessions unexpectedly | `/collaborators` opens a read-only info dialog; `j`/`k`/`Esc` convention; focus test pins session id unchanged across open + close | pass |

## 6. Failure and recovery review

- Duplicate delivery: snapshot applies are idempotent renewals keyed by
  `(project_id, request_id, reconnect_epoch)`; hints only set
  `needs_resync` and coalesce while loading (pinned by
  `hint_coalesces_while_loading`).
- Cancellation races: closing a tab clears unheld presence and bumps
  view-switch/routing epochs; stale completions with old request ids or
  epochs drop (pinned by stale + reconnect tests).
- Daemon restart: `on_reconnect` drops in-flight bindings and flags
  every project for resync; stale presentation is replaced by the next
  authoritative snapshot, never fabricated.
- Partial persistence failure: N/A (no durable presence writes).
- Stale generation/lease: daemon-side M001 semantics unchanged; TUI
  replaces stale rows on the next snapshot and shows `(stale)` in the
  header / `Stale — refresh to resync` in the panel until then.
- Contention/resource release: capacity is daemon-enforced; TUI adds
  display bounds (32 rows, 8 sessions/row) plus the 16-project LRU; no
  per-update task exists (`PresenceHint` never spawns).
- Malformed input: unknown activity strings fail closed daemon-side
  (M001 `parse` → `None`); TUI renders only the four coarse labels.
- Bounded event/artifact behavior: hints carry only `project_id`;
  snapshots truncate with the daemon `truncated` flag surfaced as
  `+N more (bounded)`; errors truncate to 256 bytes.

## 7. Migration and compatibility review

- No durable migration: no schema change, no `STORAGE_LAYOUT_VERSION`
  bump.
- Protocol is M001-additive only: `PresenceCapabilities` /
  `PresenceSnapshotGet` / `PresenceUpdated { project_id }`. Older
  daemons answering error or `supported: false` show the generic
  unavailable panel; project tabs keep working (pinned by
  `feature_absent_*` + old-daemon round-trip tests). No
  `PROTOCOL_VERSION` bump.
- Config: no new settings surface.
- Rollback: dropping the binary clears TUI presence (ephemeral
  projection); daemon restart clears leases per M001.

## 8. Security review

- Authorization precedes lookup: daemon M003 gate enforces
  `project.observe` before any presence read (M001 evidence reused;
  no matrix change in M002).
- Principal derived from connection: unchanged from M001; TUI DTO
  handling carries no principal/role/capability (snapshots are
  read-only projections).
- Privacy: single-project denials (`project_not_found`) and genuinely
  absent projects both clear to `Unavailable` with identical panel
  (`Collaborators unavailable` + shared second line) and hidden header
  (pinned by `unauthorized_and_*_identically` reducer + integration
  tests); tab-close/auth-loss clears the cache.
- Secrets: snapshots/panels contain ids/activity/counts only;
  `activity_labels_are_coarse_and_bounded` pins absence of content
  leakage; display truncation (48-char principals, 256-byte errors)
  prevents log/panel blowup.
- Path validation: project/principal use the typed identity lexical
  contract daemon-side; TUI keys by opaque `project_id` strings and
  never interprets them as paths.
- DoS bounds: 16 projects, 32 display rows, 8 sessions/row, coalesced
  refresh (no duplicate while loading), registered-task pipeline.
- Audit: presence reads remain liveness projections (no audit events),
  consistent with M001.

## 9. Documentation and operations

- Updated: `architecture/presence.md` (M002 surface: reducer/fetch/
  header/panel/lifecycle/compatibility/testing),
  `architecture/tui.md` (collaborator-presence section).
- Help/registry: `/collaborators` (`/presence`, `/team`, `refresh`)
  in `src/tui/command.rs` + `execute_command` + `src/tui/input.rs`
  help entries.
- Operator diagnostics: header `👥 N` (+ `· agent running` /
  `(stale)`); panel `Stale — refresh to resync` and
  `Refresh failed: … — showing last known`; `presence_supported()`
  gate for the unavailable panel.
- Static guards: core-boundary, authorization-matrix,
  projection-disclosure, TUI-project-authority, execution-ownership,
  scheduler-bypass all green; no new CI lane added per verification
  policy.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

No critical/high/medium/low findings remain.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. Presence M003
(authorized read-only observation) is unblocked to `ready`
(session-projections M012 interface already satisfied; identity/audit
M005 already closed). Project-collaboration M001 remains blocked on
presence-observation M003 (unchanged).

## 12. Registry updates

- `plans/registry.md`: Presence row `M002 ready` → `M003 ready`;
  dependency-ready table: remove M002 collaborator-surface row, add M003
  authorized-observation row (unblocked by this closure);
  execution order §3 reworded (M002 closed, M003 may proceed); blocked
  work: remove `Presence M003` blocker row (now ready; blocker
  satisfied), keep project-collaboration M001 blocked on
  presence-observation M003; closure control points: add M002 closed row.
- `plans/subsystems/presence-observation-roadmap.md`: M002 `ready` →
  `closed` with closure link; M003 `blocked` → `ready` with plan link
  and cleared blocker (session-projections M012 already satisfied).
- `plans/implementation/presence-observation/002-tui-collaborator-presence-surface.md`:
  `ready for handoff` → `implemented` with closure link.
- `plans/implementation/presence-observation/003-authorized-read-only-observation.md`:
  `blocked` → `ready for handoff` with unblocked-by-M002 note.
