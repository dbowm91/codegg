# Project Work Orders and Task View M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view/003-project-task-composer-and-view.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-roadmap.md#7-milestones`

Repository baseline reviewed: `3ed785618bfac5a85f504813bdb7fc3a923e8679`
(pre-plan baseline from the plan file; implementation developed on top
of the M002 closure tree `540e66bc`)

Implementation commits or pull requests:

- `79a00460` — work-orders M003: Task composer, scheduling sheet,
  Task view, Task-model preference (composer/sheet/view TUI,
  daemon `TaskModelPreferenceSet`, `migrate_v62`/layout 62,
  `/tasks` migration, focused tests, architecture/skill docs;
  layout-track test updates with unchanged intent)

## 1. Executive finding

M003 is complete. A developer can press `Ctrl+G` into Task composer
mode in a project, type a normal prompt, press Enter, confirm a
scheduling sheet whose default is immediate/zero delay with one
occurrence, and create a daemon-owned WorkOrder through the canonical
M001 service (the TUI never creates sessions/jobs directly). The
project Task view shows running, future/waiting, attention, and recent
work from one bounded projection, reorders future sequential tasks
under CAS (`Shift+J/K`), opens materialized sessions through canonical
project-tab/session routing, and surfaces attention without focus
theft. Last Task-model preference is a separately-scoped daemon-owned
runtime preference (`migrate_v62`), never the ordinary session
preference. `/tasks` (and `/task`) presents WorkOrders with a legacy
schedule fallback + diagnostic on older servers; `/schedules` keeps
direct low-level access and the `Schedule*` protocol is preserved. No
second scheduler, no speculative sessions, no `InputMode` overload, no
Tab collision, no legacy `Task*` protocol. This is capability work as
the plan classifies it; M004 is unblocked by this closure.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Composer mode distinct from `InputMode` (§5, §14A) | `ComposerMode::Session\|Task` in `src/tui/app/state/work_orders.rs`; `PromptState.composer_mode`; toggle preserves `InputMode` + prompt text | pass | `composer_toggle_preserves_input_mode_and_prompt`; mode chip `composer:task` in header agent/model context area |
| Collision-free Task-mode keybinding (§5, §14A) | `InputAction::ToggleComposerMode` on `Ctrl+G` in both keymaps; Tab=`SwitchAgent`, Shift+Tab=permission cycling untouched; sheet/view modals consume Tab locally | pass | `keybinding_audit_has_no_tab_collision`; Insert+Normal help entries for `Ctrl+G`; before/after table in §3 |
| Selector integration + help (§5, §14A) | `ActionKey::ToggleComposerMode` (label, `all()`, `to_input_action`, configurable), `default_help_entries` Insert+Normal | pass | `configurable_action_catalog_is_exhaustive` still green; keybind editor picks the action up automatically |
| Prompt preservation (§5, §15) | Prompt never cleared/messaged before confirm; `PendingTaskCreate` + stash-then-restore on failure exactly once | pass | `task_mode_enter_opens_sheet_without_touching_transcript`, `create_failure_restores_prompt_exactly_once`, full-loop test |
| Scheduling sheet + defaults (§6, §14B) | `TaskScheduleDialog` seeded from `TaskScheduleDraft::default()` (delay `0`, repeat `1`, no trigger); prefetch (lanes/summary/capabilities/preference) before open | pass | `default_draft_is_zero_delay_single_occurrence`; prefetch stale-guarded |
| Lane/order/wait/not-before/repeat/All-Any/trigger/model/policy (§6, §15) | Sheet fields + `validate_draft` + `build_work_order_create` + `describe_schedule` | pass | `multi_gate_draft_uses_join_and_lane` asserts the plan's example string verbatim |
| Time semantics, no guessing (§7, §15) | `parse_delay_duration` (bounded `s/m/h/d` forms, max 30d), `parse_not_before_ms` (RFC 3339 offset required; naive fails), UTC millis stored, local render at display | pass | `delay_parser_*`, `not_before_requires_explicit_timezone` |
| Unsupported gates fail visibly (§7, §15) | External trigger always disabled (M005 absent) + confirm fails closed; sequential without lane fails closed | pass | `unsupported_gates_fail_visibly` |
| Task-model preference scope (§8, §15) | `last_task_*` columns (`migrate_v62`), `set_task_model_preference` (touches only task cols), `TaskModelPreferenceSet` Global op, DTO additive fields; fallback with notice; session preference untouched | pass | `task_model_preference_is_scoped_apart_from_session_preference`, `task_model_migration_preserves_existing_rows`, `remembered_task_model_falls_back_visibly_when_unknown`; restart survival = SQLite durable (migration test) |
| Project Task view sections/bounds/lazy detail (§9, §14C) | `TaskViewDialog` + `TaskViewState` + `group_task_rows`; one `WorkOrderList` + summary + lanes; occurrence fetch per selected row only | pass | `task_rows_group_into_bounded_sections`, `snapshot_groups_attention_with_stable_labels`, `future_row_open_fetches_detail_not_a_session` |
| Reorder CAS semantics (§10, §14D) | Waiting-only moves, pinned running head, `moved_lane_order` pure math, `WorkOrderLaneReorder` with expected revision, conflict → "queue changed; retry" + refresh; never edits Jobs | pass | `lane_reorder_respects_pinned_head`, `reorder_conflict_refreshes_with_retry_notice` |
| Attention + session control (§11) | `attention_label` closed-set mapping (model/policy/predecessor/worktree/trigger), attention section, running rows redirect to session/job control; no focus theft (attention renders in place) | pass | Snapshot attention test; cancel/resume guards (`mutate_selected_task`) |
| `/tasks` migration, no N+1, no legacy protocol (§12, §14E) | `/tasks`→view (capability-gated, legacy+diagnostic fallback), `/task`→view, `/schedules`→legacy list; primary UX issues no `ScheduleGet` fan-out; `Schedule*` preserved | pass | `tasks_command_opens_view_not_schedule_list`; built-in count 142→144; grep evidence in §5 |
| Stale routing (§13, §15) | Route token + request `finish/fail` + view `generation` on every completion; scoped registry tasks; late create never undoes daemon state | pass | Stale-route matrix in §4; `stale_*`, `*_for_other_project_is_dropped` tests |
| Narrow terminals (§15) | Bounded lines, width truncation, min-area guards in both dialogs | pass | `*_renders_across_terminal_sizes_without_panic` (80×24, 40×20, 20×5) |
| Tab close never cancels daemon work (§15) | `cancel_task_submit` drops UI continuation only; view close cancels frontend requests only + bumps generation | pass | `tab_close_cancels_ui_only_never_daemon_work` |
| Docs (§14E) | `architecture/work_orders.md` M003 section, `architecture/tui.md` (state/dialogs/msgs/commands/mode/action), `architecture/storage.md` v62, `architecture/session.md` preference store, TUI skill layout rows | pass | — |

## 3. Before/after task UX and keybinding table

| Surface | Before | After |
|---|---|---|
| `Enter` on a typed prompt (Session mode) | Submit turn / create session | Unchanged |
| `Enter` on a typed prompt (Task mode) | n/a (no Task mode) | Opens scheduling sheet; prompt stays editable |
| `Tab` / `Shift+Tab` | Switch agent / permission-mode cycle | Unchanged in both keymaps (audit-tested) |
| `Ctrl+G` | Unbound | Toggle composer Session/Task (configurable, help-documented) |
| Sheet `Tab`/`Shift+Tab`, arrows | n/a | Sheet focus navigation (consumed by modal) |
| Sheet `Enter`/`Ctrl+Enter`/`Esc` | n/a | Confirm (validate → create) / confirm / cancel (prompt untouched) |
| View `j/k`/arrows, `PgUp/PgDn`, `g/G` | n/a | Selection navigation over flat section order |
| View `Shift+J/K` | n/a | CAS reorder of waiting lane members |
| View `Enter` / `d` / `r` / `x` / `u` / `Esc` | n/a | Open session / detail / refresh / cancel-waiting / resume-paused / close |
| `/tasks` | Low-level schedule list with N+1 `ScheduleGet` label fan-out | WorkOrder Task view (capability-gated; legacy list + diagnostic fallback) |
| `/task`, `/task-view` | n/a | Open Task view |
| `/schedules` | n/a | Low-level schedule diagnostics (legacy surface preserved) |
| `/task-del`, `/loop` | Schedule create/delete | Unchanged (low-level primitive retained) |

## 4. Stale-route matrix (all covered by tests)

| Completion | Stale trigger | Behavior |
|---|---|---|
| `TaskSheetPrefetched` | Newer prefetch / tab switch / rebind / reconnect / other project | Dropped (`finish` + route + project checks); prompt untouched |
| `WorkOrderCreated` (error) | Same | Dropped or, if current, prompt restored exactly once |
| `WorkOrderCreated` (success) | Stale route | WorkOrder stands (never deleted); prompt/history untouched; visible Task view for that project refreshes, otherwise nothing foregrounds |
| `TaskViewRefreshed` | Older generation / tab switch / rebind / reconnect / other project | Dropped; rows never mix across projects |
| `TaskOccurrenceLoaded` | Older generation / route mismatch | Dropped; no row mutation |
| `LaneReordered` | Older generation | Dropped; on live revision conflict: "queue changed; retry" + canonical refresh |
| `TaskMutationFinished` | Older generation / route mismatch | Dropped |
| `TaskSessionFocus` | Tab/project/epoch mismatch or session vanished | Dropped; vanished session refreshes the view instead of binding |
| `TaskModelPrefetched` | Newer prefetch / route mismatch | Dropped silently; sheet falls back with notice |
| `TaskModelPrefSaved` | Any | Warn-only; never touches prompt/transcript/WorkOrder |
| Tab/view close | Any in-flight UI request | Registry tasks cancelled, request states cancelled, view generation bumped; daemon work untouched |

## 5. Production implementation evidence

Ownership landed:

- `crates/codegg-core/src/approval.rs`: `RuntimePreference`
  `last_task_provider_connection_id`/`last_task_model_id`,
  `has_task_model_preference`, `set_task_model_preference`
  (task-cols-only CAS write), all writers preserve the opposite
  scope, fresh schema + `RUNTIME_PREFERENCE_TASK_MODEL_MIGRATION_STATEMENTS`,
  row mapping with sanitize-on-read. Tests: scoping (task↔session
  independence across model/policy writes + CAS conflict + bounds),
  pre-v62 migration preservation.
- `crates/codegg-core/src/session/schema.rs`: `migrate_v62`
  (duplicate-tolerant `ADD COLUMN` × 2) wired into chain + dispatch;
  `crates/codegg-core/src/storage/mod.rs`:
  `STORAGE_LAYOUT_VERSION` 61 → 62.
- `crates/codegg-protocol/src/core.rs`: additive
  `RuntimePreferenceDto.last_task_*` (`#[serde(default)]`, legacy
  payloads decode to `None`) + additive
  `CoreRequest::TaskModelPreferenceSet` (optional dims, CAS).
  `PROTOCOL_VERSION` unchanged (additive surface, same convention as M002).
- `crates/codegg-core/src/authorization/policy.rs`:
  `task_model_preference_set` (Global, none) + representative request
  (matrix guard green).
- `src/core/daemon_family.rs`: `TaskModelPreferenceSet → Ops`.
- `src/core/daemon_ops.rs`: handler (principal from transport
  authority; conflict/write-failure codes match existing preference
  ops) + DTO mapping + pool-less defaults.
- `src/tui/app/state/work_orders.rs` (new, pure): `ComposerMode`,
  `TaskScheduleDraft`/`GateJoin`/`TaskSheetField`,
  delay/not-before/repeat parsers, `validate_draft`,
  `build_work_order_create`, `describe_schedule`, `PendingTaskCreate`,
  `TaskModelChoice`, `TaskViewRow`/`TaskViewState`/`group_task_rows`/
  `moved_lane_order`, `attention_label`. 11 unit tests.
- `src/tui/commands/work_orders.rs` (new): toggle, sheet
  prefetch/open/confirm/create, Task-model prefetch/persist,
  view open/refresh/detail/reorder/mutate/open-session, `/tasks`
  migration entry. 14 behavior tests + 2 isolation tests (fake
  daemon incl. full create loop).
- `src/tui/components/dialogs/task_schedule.rs`,
  `task_view.rs` (new): FocusManager-owned modals + snapshots +
  key maps + narrow-terminal render; 7 tests incl. size matrix.
- Wiring: `InputAction`/`ActionKey::ToggleComposerMode` (+ `Ctrl+G`
  both keymaps, Insert+Normal help), `Dialog::TaskSchedule|TaskView`
  (+ `DialogType` both directions, modal close arms),
  `TuiMsg` composer/sheet/view variants, `TuiCommand` completion
  variants + dispatch arms, `send_prompt` Task branch, header
  `composer:task` chip + Task placeholder, `/tasks`→view (+`/task`,
  `/schedules`; count 142→144).

Deliberate adjustments from the plan text (semantics preserved):

- No new session-creation path: Task confirm reuses the existing
  `WorkOrderCreate` daemon operation (M001 service), so "never
  bypasses the WorkOrder service" holds by construction.
- Task-model "validity" is checked against the TUI's known model
  list at sheet open and re-checked at confirm (catalog moves fall
  back visibly); the daemon does not gain a Task-model catalog
  probe — the preference remains a convenience default, never
  execution authority (M002 re-resolves at claim).
- External-trigger gate is a disabled placeholder with a fail-closed
  confirm error (M005 owns the server capability; plan §6 allows a
  capability-aware disabled option).
- The Task view is a FocusManager-owned modal dialog, not a new
  `Route` variant: one full project-scoped panel (not toasts),
  reusing the established modal lifecycle, stale-close, and focus
  isolation instead of inventing a second view system.
- Approval/sandbox snapshot on the WorkOrder uses the cached
  daemon-resolved effective policy; per-turn ceilings still enforce
  at turn time and M002 narrows at claim (same arrangement as M002
  closure).

## 6. Verification executed

### Commands run (local; no CI lane added per verification policy)

```bash
cargo test -p codegg --lib -- tui::
cargo test --test tui_project_tabs --test tui_project_picker --test tui_project_routing
cargo test --test tui
cargo test --test tui_render
cargo test -p codegg-core --lib
cargo test -p codegg-core --lib -- approval::           # 19 (2 new)
cargo test -p codegg-core --lib -- work_order           # 35
cargo test -p codegg-protocol                           # 183
cargo test --test storage_migrations                    # 4
cargo test --test work_orders_m001_foundation            # 12
cargo test --test work_orders_m002_materialization       # 10
cargo test --test model_preference_convergence           # 20
cargo test --test policy_surface_m007                    # 14
cargo test -p codegg-core --test work_plan_foundation    # 11
cargo test -p codegg-core --test continuation_checkpoint # 10
cargo test --test long_horizon_trajectory_qualification  # 27
cargo test --test reliability_qualification_m008         # 39
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_policy_surface.py
cargo fmt --all -- --check
cargo clippy -p codegg --lib --features=lsp-test-support -- -D warnings
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

### Results (local)

- `codegg --lib tui::`: 875 passed (incl. 11 state + 16
  command/isolation + 7 dialog Task tests; 0 failed).
- Project tab/picker/routing: 22 + 27 + 20 passed.
- `tui` behavior: 165 passed. `tui_render`: 99 passed.
- `codegg-core --lib`: 778 passed (incl. 19 approval with the 2
  new Task-scope tests; 35 work_order).
- `codegg-protocol`: 183 passed (legacy preference JSON still
  decodes; new fields default).
- `storage_migrations`: 4 passed; catalog invariants 7/7.
- M001 foundation: 12 passed (after updating the remigration
  version assertion 61→62, same intent); M002 materialization: 10
  passed (no M002 regression from v62).
- Model-preference convergence: 20 passed (ordinary session
  preference untouched by the new scope); policy surface: 14.
- work_plan/continuation: 11 + 10 passed (after renaming the
  layout-track tests to v62 and expecting 62; same intent).
- Long-horizon trajectory: 27 passed; reliability M008: 39.
- Guards: daemon-cwd ok, git-forbidden PASS, core-boundary pass,
  execution-ownership ok, authorization matrix verified, policy
  surface ok, fmt clean, clippy (`-p codegg --lib` with
  `lsp-test-support` and full `--workspace --all-targets --locked`)
  clean, `verify.sh quick` pass (incl. TUI project-authority guard).

## 7. Invariant review

| Plan §3 invariant | Evidence |
|---|---|
| TUI is projection/controller only | View/sheet render cached state; all mutations are `CoreRequest`s; daemon owns truth; `check-core-boundary` pass |
| `InputMode` separate from composer mode | `ComposerMode` in prompt UI state; toggle test asserts `InputMode` untouched |
| No silent Tab/Shift+Tab collision | Defaults preserved in both keymaps; sheet/view consume Tab locally; audit test pins all three bindings |
| Default Task never bypasses WorkOrder service | Confirm builds `WorkOrderCreateRequest` and sends `CoreRequest::WorkOrderCreate`; no `SessionCreate`/job call in Task paths (ownership: TUI → existing WorkOrder service) |
| Running-session entry uses canonical routing + stale guards | `apply_task_session_focus`: route match + project match + session-list lookup + `set_session` machinery + `Route::Session` navigate |
| View close/switch never cancels daemon work | Close arms cancel frontend requests only + generation bump; `cancel_task_submit` drops UI state only; test asserts no daemon traffic |
| Reorder only eligible waiting rows with expected revision | Selected-row guards (running/terminal/laneless rejected with hints) + `moved_lane_order` pin math + CAS revision; conflict path refreshes |
| Last Task model is convenience, never authority | Separate daemon scope; snapshot-at-create; fallback with notice; `AgentTurn`/claim paths untouched |
| Unavailable model/policy/workspace shown as attention | `attention_label` closed-set mapping + Attention section; pre-creation fallback toasts; no silent repair |
| No second scheduler / no speculative sessions | Coordinator/scheduler untouched; waiting rows are `WorkOrderDto`s, never session rows; occurrence detail only after daemon claim |

## 8. Failure and recovery review

- Duplicate delivery: create idempotency keys (`tui-task-<uuid>`);
  daemon duplicate convergence surfaces "(converged on existing
  task)"; lane CAS admits one winner (`work_order_revision_conflict`
  → "queue changed; retry" + refresh).
- Stale writers: request `finish/fail` + route match + view
  generation on every completion; registry tasks scoped to tab/epoch.
- Restart: Task preference is SQLite-durable (migration test);
  in-flight UI continuations invalidate on reconnect (epoch guard)
  and refresh on foreground; daemon state never deleted by the UI.
- Partial persistence: prompt stays until create success; failure
  restores exactly once with stash of newer typing.
- Malformed/unauthorized input: daemon validation fails closed with
  `code: message` surfaced; unknown lanes/models fall back visibly.
- Cancellation: pre-confirm Esc clears draft only; post-create
  cancel targets waiting rows via `WorkOrderCancel`; running rows
  redirect to session control; terminal rows are no-ops with hints.
- Sequence: reorder never crosses the pinned head; failure-hold
  semantics unchanged from M002.

## 9. Migration and compatibility review

- Additive storage: `migrate_v62` adds two nullable columns;
  pre-M003 rows read back with `None` Task preference and full
  session/policy preservation (test); fresh DBs include the columns
  in `CREATE TABLE`; `STORAGE_LAYOUT_VERSION` 61 → 62 with guard +
  layout-track tests updated (same intent).
- Additive protocol: two `#[serde(default)]` DTO fields (legacy
  payloads decode) + one new request variant; `PROTOCOL_VERSION`
  unchanged; old clients ignore the variant; old servers fail the
  new op closed (TUI surfaces the error, prompt preserved).
- `/tasks` migration: capability-gated; older servers get the legacy
  schedule list plus an explicit diagnostic (no silent downgrade);
  `Schedule*` operations untouched; no `Task*` wire operation added
  or revived.
- Rollback: downgrading below this change leaves the two columns
  inert but present; new event/request shapes are ignored by older
  readers; no data migration to reverse.

## 10. Security review

- Authorization: `TaskModelPreferenceSet` is Global/none-capability
  like the other principal-scoped preference ops (principal bound
  server-side from transport authority; payloads carry no identity);
  WorkOrder ops reuse the M001 project-scoped mapping (matrix guard
  green). No new principal surface.
- Privacy: Task view rows carry titles/prompts (author-supplied work
  description, same as M001 DTOs); attention labels are bounded
  codes + truncated diagnostics; no secrets/reasoning in events,
  toasts, or snapshots.
- Secrets: none added (model identities only, ≤512 chars,
  NUL/blank-sanitized); idempotency keys are opaque retry
  identities, not credentials.
- Attribution/audit: creates go through the attributed M001
  `WorkOrderCreate` path (origin capture unchanged); daemon-side
  preference writes are principal-scoped like existing ones.
- Bounds as DoS control: view capped (`MAX_TASK_VIEW_ROWS` 200,
  section cap 50, lanes 20, models 50, sheet buffers 128 chars);
  list limits clamped; lane/member/repeat/gate bounds unchanged
  from M001/capabilities.
- Isolation truthfulness: workspace line reports the daemon default
  ("auto"); no isolation claims are made pre-claim.

## 11. Documentation and operations

- `architecture/work_orders.md`: M003 composer/sheet/view section +
  storage v62 + test commands.
- `architecture/tui.md`: Task request states, draft/view cache,
  `TaskSchedule`/`TaskView` dialogs, new `TuiMsg`/`TuiCommand`
  variants, `/tasks`→view + `/schedules`, `ComposerMode` vs
  `InputMode`, `ToggleComposerMode`.
- `architecture/storage.md`: v62 line; `architecture/session.md`:
  preference-store v62 + `set_task_model_preference` +
  `TaskModelPreferenceSet`.
- `.opencode/skills/tui/SKILL.md`: layout rows for
  `state/work_orders.rs` + `commands/work_orders.rs`.
- Help: `Ctrl+G` "Toggle composer mode (Session/Task)" in Insert +
  Normal help; sheet/view footers carry their own key legends.
- Operator diagnostics: sheet validation errors (actionable, with
  examples), "queue changed; retry", capability/compat
  diagnostics, Task-model fallback warnings, creation result line
  (`Task <id8> created: <natural-language summary>`).

## 12. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Task-model "exists" check uses the TUI's known model list, not a live catalog probe | A model removed between sheet open and confirm falls back visibly at confirm (covered), but a model unknown to the TUI yet valid daemon-side would also fall back | M007 may add a daemon-validated resolve; current behavior fails toward the visible current model, never silently |
| low | Sheet queue preview shows lane/count summaries, not per-task running/waiting rows | Placement context is coarser than the full view | The view (one key away) shows full order; no corrective pass: documented scope |
| low | No global multi-project dashboard | M004 scope, explicitly out of M003 | M004 (now ready) |
| low | No external-trigger UI beyond the disabled placeholder | M005 scope, explicitly out of M003 | M005 (held for ordered handoff after M004) |

No critical/high/medium findings. No corrective pass required.

## 13. Roadmap disposition

Milestone closed and the next dependency may proceed: M003 exit
condition is satisfied (a developer can author, order, schedule,
inspect, enter, stop, and redirect project tasks entirely from the
TUI without bypassing daemon authority).

Blocked-work audit (registry `Blocked work` + roadmap §6 dependency
graph):

- M004 (global Workspace dashboard, hard dep M003) → **ready**.
  Its sole hard dependency (M003 closure) is now satisfied.
- M005 (external trigger endpoint, hard dep M002; scheduled after
  M004 for handoff clarity) → remains **blocked**: hard dependency
  satisfied, but registry sequencing keeps one clear handoff chain
  after M004. Blocker note unchanged.
- M006 (agent WorkOrder tool, hard dep M002/M001 service; scheduled
  after M005) → remains **blocked**: hard dependencies satisfied,
  but ordered handoff clarity keeps it after M005. Blocker note
  unchanged.
- M007 (qualification, hard dep M001-M006) → remains **blocked** on
  M001-M006 closure.

## 14. Registry updates

- Move M003 from dependency-ready to closed with this closure record
  (implementation commit recorded below).
- Move M004
  (`004-global-workspace-dashboard.md`) from blocked to ready: its
  hard dependency (M003 closure) is now satisfied.
- Keep M005 blocked (hard M002 dependency satisfied; held for ordered
  handoff after M004).
- Keep M006 blocked (hard M002/M001 dependencies satisfied; held for
  ordered handoff after M005).
- Keep M007 blocked on M001-M006 closure.
- Record M003 under recently closed work.
- Update the subsystem roadmap M003 status to closed and M004 to ready.
