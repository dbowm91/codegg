# Project Work Orders and Task View M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view/004-global-workspace-dashboard.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Repository baseline reviewed: `4fe8166f32eecb4cf3ad9a5968b4af3fe2e2ad7a`
(M003 closure tree; the plan's pre-plan baseline `3ed78561` predates
M001–M003 and the implementation developed on top of the M003 closure)

Implementation commits or pull requests:

- `8c6e8190` — work-orders M004: global Workspace dashboard and
  team-aware task projection (daemon `WorkspaceDashboard` aggregate,
  `ProjectActivitySummary` DTO/bounds/privacy, TUI overlay/route/
  render/navigation/descent, `/workspace` + hotkey, event/reconnect/
  revocation guards, focused tests, architecture/skill docs)

## 1. Executive finding

M004 is complete. `/workspace` and the `Ctrl+O` / vim-`W` hotkey open
one global bounded dashboard over every project the principal may see,
with coarse running/future/attention/permission/question state per
project and no sensitive content. The dashboard consumes a single
daemon-owned aggregate projection per refresh — never TUI-side N+1
fan-out — and opening it activates no project services and cancels no
work. The user descends dashboard → project Task view → materialized
session through existing tab/session routing, then returns
predictably. Vim-like navigation, project-tab semantics, and picker
registration behavior are unchanged. Revocation, reconnect, and stale
completion behavior is fail-closed and project-correct. This is
capability work as the plan classifies it; M005 is unblocked by this
closure.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Bounded daemon projection, no N+1 (§5) | `CoreRequest::WorkspaceDashboard` / `CoreResponse::WorkspaceDashboard`; one aggregate per refresh; fake-client counts (`dashboard_requests == 1` per open, `detail_requests == 1` per explicit expand) | pass | `expand_issues_one_bounded_detail_fetch`; daemon `tests/work_orders_m004_dashboard.rs` |
| Row contract: coarse counts/status, redacted (§5) | `ProjectActivitySummaryDto` + `is_redacted`; wire-JSON negative census (prompt/secret/reasoning/token/diff/path absent) | pass | `owner_sees_bounded_redacted_rows_with_counts` |
| No eager activation (§3, §9) | Handler touches catalog list + indexed COUNT/MAX + in-memory session registry + team store only; static forbidden-symbol test | pass | `dashboard_projection_touches_no_heavy_activation`; no storage migration |
| Enumeration auth, per-row counts gating (§6) | `workspace_dashboard` Enumeration + `project.read` preamble; per-row `session.read` check with `counts_visible == false` zeroing | pass | `member_sees_only_their_project`, `viewer_sees_counts_explicit_decision`, `presence_only_row_zeroes_counts_and_status` |
| Viewer decision explicit (§6) | Role matrix grants Viewers `session.read`, so Viewers see counts; zeroed branch reserved for future least-privilege grants, unit-pinned | pass | Closure §8; integration + unit tests |
| Coarse attention labels (§6, §10) | Closed `WORKSPACE_DASHBOARD_STATUS_CODES`; permission/question/attention/failed/running/waiting/idle/archived precedence | pass | `status_code_precedence_is_closed_vocabulary`, `attention_failed_and_permission_badges` |
| Global route/overlay, return semantics (§7) | `Dialog::WorkspaceDashboard` overlay; return tab stored; open/close preserves active tab/session; Esc pops without reload | pass | `workspace_command_and_hotkey_open_same_dashboard`, `esc_returns_to_exact_prior_tab_without_reload` |
| Enter descends to Task view/session (§7) | `open_selected_dashboard_project` → `open_or_focus_project` + M003 `open_task_view` (no second tab model, no fabricated sessions) | pass | Reuses M003-tested session-focus path |
| Future selection opens Task detail, not a session (§7) | Descent always lands in the Task view, whose Enter fetches detail for future rows (M003 semantics, unchanged) | pass | No new session-opening path added |
| Hotkey + `/workspace` (§8) | `OpenWorkspaceDashboard` action (`Ctrl+O` insert/vim, vim `W`); `/workspace` command (count 144→145); Insert/Normal/Vim help entries | pass | `dashboard_hotkey_passes_collision_audit`, `workspace_dashboard_action_is_configurable_and_helped` |
| Vim-like nav + filter (§8) | `j/k`/arrows, `PgUp/PgDn`, `g/G`, type-to-filter (picker convention), `Tab` expand, `Ctrl+R` refresh, `Esc` back | pass | Dialog key-map test; state nav/filter tests |
| No hard-coded keys outside the system (§8) | Dialog consumes only modal nav keys; the global hotkey lives in `default_bindings`/`vim_bindings` via `ActionKey` (keybind editor picks it up) | pass | `configurable_action_catalog_is_exhaustive` still green |
| Lazy details + bounds (§9) | Pages clamp to 128 (catalog bound, default 64); deterministic order (attention/running/recency/name/id); cursor paging; detail only for the explicitly expanded project (limit 8) | pass | `pagination_is_deterministic_and_bounded`, `limit` clamp in handler |
| Attention without focus theft (§10) | `note_dashboard_hint` marks rows dirty, no toast/modal; permission/question hint test pins dialog identity | pass | `inactive_permission_hint_marks_badge_without_focus_theft` |
| Event/reconnect model (§11) | Generation + reconnect-epoch guards on both completions; whole-page replace on load; `resync_dashboard_after_reconnect`; revocation clears rows/expansion | pass | `stale_generation_and_epoch_completions_drop`, `revocation_clears_row_and_detail` |
| Picker registration unchanged (§13) | No picker state touched; picker suites green | pass | `picker_registration_behavior_unchanged`; `tui_project_picker` 22 green |
| Help/docs/tests (§4) | `architecture/work_orders.md` M004 section, `architecture/tui.md` dialog/state entries, TUI skill rows, Insert/Normal/Vim help | pass | — |

## 3. Production implementation evidence

Ownership landed:

- `crates/codegg-protocol/src/work_order.rs`:
  `ProjectActivitySummaryDto` (12 coarse fields + `counts_visible`),
  `MAX_WORKSPACE_DASHBOARD_LIMIT` (128),
  `DEFAULT_WORKSPACE_DASHBOARD_LIMIT` (64),
  `WORKSPACE_DASHBOARD_STATUS_CODES` (closed 8), `is_redacted`.
- `crates/codegg-protocol/src/core.rs`: additive
  `CoreRequest::WorkspaceDashboard { cursor, limit, include_archived }`
  and `CoreResponse::WorkspaceDashboard { rows, next_cursor,
  truncated }` (`#[serde(default)]` optionals; `PROTOCOL_VERSION`
  unchanged, same additive convention as M001–M003).
- `crates/codegg-core/src/authorization/policy.rs`:
  `workspace_dashboard` (`Enumeration` + `ProjectRead`) with
  representative request (matrix guard green).
- `src/core/daemon_family.rs`: `WorkspaceDashboard => Projects`
  (same daemon-owned catalog state as `ProjectList`, no new store).
- `src/core/daemon.rs`: enumeration preamble extended to
  `workspace_dashboard` (same path as `project_list`).
- `src/core/daemon_workspace_dashboard.rs` (new): `handle_...`
  (catalog list → principal filter → per-row cheap aggregates →
  `session.read` counts decision → deterministic sort → cursor page);
  single-pass in-memory session scan; `apply_counts_visibility` pure
  redaction; `status_code` closed precedence. 5 unit tests.
- `src/core/daemon_projects.rs`: `WorkspaceDashboard` arm delegating
  to the handler; `src/core/mod.rs`: module registration.
- `src/tui/app/state/workspace_dashboard.rs` (new, pure):
  `WorkspaceDashboardState` (rows/filter/selection/generation/epoch/
  return-tab/dirty/expansion) + `dashboard_row_badge`; 12 unit tests.
- `src/tui/commands/workspace_dashboard.rs` (new): open/refresh/
  expand/descend/hint/resync/key-router + 10 behavior tests
  (fake daemon with request counters proving one-aggregate refresh
  and one bounded detail fetch).
- `src/tui/components/dialogs/workspace_dashboard.rs` (new):
  FocusManager-owned modal + snapshot + key map + narrow-terminal
  render; 3 tests incl. size matrix (80×24, 40×20, 20×5).
- Wiring: `Dialog::WorkspaceDashboard` + `DialogType` (both
  directions, round-trip test extended),
  `TuiMsg::OpenWorkspaceDashboard` + 4 dashboard msgs,
  `TuiCommand::WorkspaceDashboardLoaded/Expanded` + dispatch arms,
  `/workspace` (count 144→145), `InputAction`/`ActionKey::
  OpenWorkspaceDashboard` (`Ctrl+O` both keymaps, vim `W`),
  Insert/Normal/Vim help entries, dashboard key router in
  `handle_dialog_key`, event-hint hook in `handle_routed_event`,
  reconnect resync in `on_projection_reconnect`, close arm (UI-only
  cancel + generation bump + state clear; daemon work untouched).
- No storage migration (`STORAGE_LAYOUT_VERSION` unchanged; layout
  guard green). No new event shape (existing bus events feed hints).

Deliberate adjustments from the plan text (semantics preserved):

- The dashboard is a FocusManager-owned modal overlay, not a new
  `Route` variant — same choice M003 made for the Task view: one
  bounded panel reusing modal lifecycle, stale-close, and focus
  isolation instead of a second view system. "Return to prior view"
  holds because the prior tab/session is never left while the
  overlay is open.
- `waiting_work_order_count` counts release-pending occurrence
  instances while `future_work_order_count` counts durable
  active/paused templates: M001 creates no occurrence rows before
  release, so a delayed template is future work rather than a
  waiting instance (matches the M001 store/`summary_counts`
  convention; integration-pinned).
- Materialized-session entry from the dashboard goes through the
  Task view's canonical session-focus path rather than a second
  dashboard-owned opener: `Enter` focuses the project tab and opens
  its Task view, where running rows open sessions and future rows
  fetch detail (no duplicate ownership, no fabricated sessions).
- Refresh is `Ctrl+R` (not bare `r`) and expand is `Tab` (not `e`)
  so every other bare character stays typeable filter text under
  the picker convention (`j`/`k`/`g`/`G` navigate, as in the
  picker).
- No new HTTP route: `WorkspaceDashboard` travels the existing
  Core transports like all `CoreRequest`s; remote-TUI compatibility
  follows the standard capability/error contract (pool-less daemon
  reports `workspace_dashboard_unavailable`, never `unimplemented`).

## 4. Verification executed

### Commands run (local; no CI lane added per verification policy)

```bash
cargo test -p codegg --lib -- workspace_dashboard
cargo test -p codegg --lib -- daemon_workspace_dashboard
cargo test --test work_orders_m004_dashboard
cargo test -p codegg --lib -- project_catalog
cargo test -p codegg --lib -- tui::
cargo test --test tui_project_picker --test tui_project_tabs --test tui_project_routing
cargo test --test tui --test tui_render
cargo test --test identity_m003_daemon_authorization
cargo test --test work_orders_m001_foundation --test work_orders_m002_materialization
cargo test -p codegg-core --lib -- work_order
cargo test -p codegg-protocol
python3 scripts/check_authorization_matrix.py
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_policy_surface.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_execution_ownership.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

### Results (local)

- Dashboard suites: `workspace_dashboard` lib 30 passed (12 state +
  10 command incl. no-N+1 counters + 3 dialog + 5 daemon handler);
  `work_orders_m004_dashboard` integration 8 passed (owner/member/
  viewer/pagination/attention+permission/running/archived/
  pool-less).
- Regression: `project_catalog` lib 2; `tui::` lib 900;
  picker/tabs/routing 22 + 20 + 27; `tui` 165; `tui_render` 99;
  identity_m003 9; M001 12; M002 10; core `work_order` 35;
  protocol 183. All green.
- Guards: authorization matrix verified; catalog invariants 7/7;
  daemon-cwd ok; policy surface ok; git-forbidden PASS;
  execution-ownership ok; core-boundary pass; fmt clean; clippy
  (`--workspace --all-targets --locked`) clean; `verify.sh quick`
  pass (incl. sandbox, TUI project-authority guards, workspace
  check).
- `cargo test --test authorization` (plan §14) has no such target
  in the repo (no `tests/authorization.rs`): substituted with
  `identity_m003_daemon_authorization` (9) + the authorization
  matrix guard, both green; recorded here instead of hidden.
- `python3 scripts/check_scheduler_bypass.py` reports one finding
  in `src/agent/snapshot_capture.rs:181`, untouched by this change
  (last modified by execution-reliability M005 `34ceffe5`) and
  unrelated to any M004 path (no scheduler/executor/pool contact).
  Pre-existing; carried as a low residual below, not a closure
  blocker.

## 5. Invariant review

| Plan §3 invariant | Evidence |
|---|---|
| Dashboard is not a durable Workspace; `WorkspaceId` meaning unchanged | No migration, no new identity type, no `Workspace*` store contact; rows are display DTOs |
| Daemon-owned projection; TUI never scans/cwds/joins caches | One aggregate request per refresh (counter-pinned); no `current_dir`, no path reads (cwd guard green) |
| Catalog listing stays cheap/probe-free | Reuses `list_projects`; static no-activation test; no new queries beyond indexed COUNT/MAX |
| No LSP/Git/provider/build/workspace init per project | Static forbidden-symbol test over the handler file |
| Unauthorized rows absent / privacy-equivalent | Enumeration filter + member/viewer integration tests; denial-shaped errors unchanged |
| No secret content at global scope | Closed DTO shape + wire-JSON negative census + `is_redacted` |
| Enter/leave never cancels or mutates work | Overlay open/close preserves tab/session (test); close path drops UI continuations only + generation bump |
| Stale/reconnect completions cannot misapply | Generation + dual epoch checks on both completions; resync path; drop tests |
| Bounded for many projects | 128 clamp, cursor paging, 5-project determinism test, bounded TUI cache/render/filter |

## 6. Failure and recovery review

- Duplicate delivery: dashboard refresh is a read (idempotent by
  construction); whole-page replacement converges retries.
- Stale writers: request `finish/fail` + generation + dashboard
  epoch + live registry epoch on every completion; tab close needs
  no dashboard invalidation (global scope, no tab binding).
- Restart: dashboard state is ephemeral; reconnect bumps the epoch
  and resyncs one bounded page from the daemon; daemon aggregates
  derive from durable stores on request.
- Partial persistence: n/a (no dashboard writes exist).
- Malformed/unauthorized input: unknown cursor restarts paging;
  over-limit clamps; pool-less reports `unavailable`; member
  denials filter (integration-pinned).
- Cancellation: close/Esc drops frontend tasks only + bumps
  generation; daemon work untouched (overlay never owns any).
- Contention: no CAS surface (read-only projection); expansion
  races resolve by generation + project match.
- Bounded behavior: rows/filter/visible/expanded caps enforced in
  state (`MAX_DASHBOARD_ROWS/VISIBLE/FILTER/EXPANDED_TASKS`).

## 7. Migration and compatibility review

- No storage migration: projection derives from the M001
  `work_order`/`work_order_occurrence` tables plus catalog and
  ephemeral registries. `STORAGE_LAYOUT_VERSION` unchanged.
- Additive protocol: one request/response pair with defaulted
  optionals; `PROTOCOL_VERSION` unchanged; old clients never send
  it; old servers fail it closed through the standard unknown-op
  path (TUI surfaces `code: message`, dashboard shows the error).
- No legacy surface removed: picker, tabs, `/tasks`, `Schedule*`
  untouched (suites green).
- Rollback: downgrading drops the op; no durable residue exists.

## 8. Security review

- Authorization: `workspace_dashboard` is `Enumeration` +
  `project.read` (preamble-filtered like `project_list`);
  per-row counts require `session.read` via `TeamStore::
  has_capability` (principal from transport authority, never the
  payload). Matrix + policy-surface guards green.
- Viewer decision (explicit): current roles grant Viewers
  `session.read`, so Viewers see counts; presence-only zeroing
  (`apply_counts_visibility`, unit-pinned) is reserved for future
  least-privilege grants. Row presence reveals nothing beyond
  `ProjectList`.
- Privacy: counts and closed status codes only; permission/question
  badges never carry request content; attention diagnostics stay in
  the project Task view.
- Secrets: none added, none transported (negative wire census).
- Attribution/audit: read-only op; no audit row beyond the standard
  authorized-request envelope.
- Bounds as DoS control: 128 page clamp, cursor paging, one
  aggregate per refresh, one limit-8 detail fetch per explicit
  expand, bounded filter/render/expansion state.

## 9. Documentation and operations

- `architecture/work_orders.md`: M004 dashboard section (contract,
  auth/privacy decision, non-activation, TUI wiring, adjustments).
- `architecture/tui.md`: `DialogState.workspace_dashboard`,
  `Dialog`/`DialogType::WorkspaceDashboard`, component/command
  behavior, `/workspace` + hotkey.
- `.opencode/skills/tui/SKILL.md`: layout rows for
  `state/workspace_dashboard.rs` + `commands/workspace_dashboard.rs`.
- Help: `Ctrl+O` "Open workspace dashboard" (Insert + Normal),
  `W` (Vim); dashboard footer carries its own key legend.
- Operator diagnostics: dashboard errors surface as
  `code: message` in-view plus warning toast; `updating…`/`stale`
  markers show hint-pending state; truncation notice advises
  filter refinement.
- Static guards: authorization matrix, catalog invariants,
  daemon-cwd, policy surface, core-boundary, execution-ownership —
  all green (see §4).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `check_scheduler_bypass.py` flags `src/agent/snapshot_capture.rs:181` (pre-existing, last touched by M005) | None on M004 paths; the dashboard contacts no scheduler/executor/pool | None for M004; owning workstream to disposition |
| low | No dedicated `tests/authorization.rs` target for the plan's literal `cargo test --test authorization` line | Substituted with identity_m003 + matrix guard (same authority surface) | None; future plans should name existing targets |
| low | Expansion shows waiting/future task titles; materialized-session entry stays in the Task view | One extra descent step for session entry from the dashboard | Documented scope; no corrective pass |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and the next dependency may proceed: M004 exit
condition is satisfied (authorized users see all known projects and
coarse ongoing/future/attention state from one bounded screen and
enter the relevant project/session without N+1 heavy activation).

Blocked-work audit (registry `Blocked work` + roadmap §6 dependency
graph):

- M005 (external trigger endpoint, hard dep M002 already closed;
  held after M004 for ordered handoff clarity) → **ready**. Its
  ordering gate (M004 closure) is now satisfied; M004 is not a
  semantic dependency of the trigger service per the M005 plan §17
  note.
- M006 (agent WorkOrder tool, hard dep M002/M001 service; scheduled
  after M005) → remains **blocked**: ordered handoff clarity keeps
  it after M005. Blocker note unchanged.
- M007 (qualification, hard dep M001-M006) → remains **blocked** on
  M001-M006 closure.

## 12. Registry updates

- Move M004 (`004-global-workspace-dashboard.md`) from ready to
  closed with this closure record (implementation `8c6e8190`).
- Move M005 (`005-external-task-trigger-endpoint.md`) from blocked
  to ready: hard dependency (M002 closure) satisfied and ordering
  gate (M004 closure) now satisfied.
- Keep M006 blocked (held for ordered handoff after M005).
- Keep M007 blocked on M001-M006 closure.
- Record M004 under recently closed work; update the subsystem
  roadmap M004 status to closed and M005 to ready; mark the M004
  plan file implemented.
