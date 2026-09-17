# Project Work Orders UX Fidelity Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view-corrective/001-human-task-ux-and-trigger-surface.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-ux-corrective-addendum.md#7-corrective-milestone`

Repository baseline reviewed: `85b9fadb` (plan registration; descendant of the plan's `e013acd` audit baseline with only plan docs added, no production delta)

Implementation commits or pull requests:

- `1bd77c6d` — work-orders C001: human Task UX and trigger surface (root Tab composer ownership, SwitchAgent migration, focused queue editor, external-trigger enablement, one-time secret dialog, Task-view trigger management, docs)

## 1. Executive finding

C001 finishes the human-facing Task workflow without changing the closed WorkOrder execution architecture. Bare Tab cycles Session/Task at root prompt focus, the scheduling sheet exposes a focused queue editor with direct `j/k`/Up/Down insertion movement through the existing lane CAS contract, and the scheduling/Task view exposes secure creation/display/retry/rotation of the already-landed M005 trigger capability. No second scheduler, session type, trigger service, authorization framework, durable workflow abstraction, storage migration, or protocol redesign was added. The milestone is complete.

## 2. Requirement-to-evidence matrix

### Finding A — Task mode reachable by bare Tab (was `Ctrl+G` only)

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Session is default composer mode | `ComposerMode::default() == Session` (`tui::app::state::work_orders::tests::composer_mode_toggles_and_labels`); prompt starts in Session | pass | Unchanged |
| Bare Tab at root prompt toggles Session→Task→Session in Insert | `tui::input::tests::c001_bare_tab_owns_composer_cycle_in_both_modes`; `default_bindings` Tab=`ToggleComposerMode` | pass | Both maps |
| Same semantic behavior in Normal without changing `InputMode` | Same test in Normal + `composer_toggle_preserves_normal_mode_and_selection_state` (InputMode stays Normal) | pass | `InputMode` untouched |
| Prompt/cursor/model/permission/project/session unchanged across composer Tab | `composer_toggle_preserves_input_mode_and_prompt` (prompt preserved) + `composer_toggle_preserves_normal_mode_and_selection_state` (model/project/session preserved) | pass | No state mutation beyond `ComposerMode` |
| Modal TaskSchedule Tab moves modal focus, does not toggle composer | `task_schedule::tests::tab_cycles_fields_and_never_emits_agent_msgs`; `App::on_key` routes to `FocusManager` first, `handle_dialog_key` never bubbles to root | pass | Modal-local Tab wins |
| TaskView/TriggerSecret Tab consumed, never composer toggle | `task_view::tests::vim_keys_map_to_view_actions_without_collisions` (Tab=`None`); `trigger_secret::tests::secret_dialog_keys_close_without_leaking` (Tab=`None`) | pass | Dropped, never bubbled |
| No duplicate bare-Tab action after normalization | `keybinding_audit_has_no_tab_collision` counts exactly one `(NONE, Tab)` owner in both maps | pass | Single owner |
| `SwitchAgent` remains in catalog/help, reachable by post-migration binding | Same audit asserts `Ctrl+A`=`SwitchAgent` in both maps + `Tab`/`Ctrl+A` help entries in both modes; `ActionKey::all()` still contains `SwitchAgent` | pass | Discoverable/configurable |
| Optional `Ctrl+G` alias produces identical transition, not a competing owner | Both maps assert `Ctrl+G`=`ToggleComposerMode`; `c001_bare_tab_owns_composer_cycle_in_both_modes` asserts alias equality | pass | Alias only |
| Replacement avoids terminal-normalized controls | Same test asserts `SwitchAgent` is never `Ctrl+I/H/M/J` (Tab/Backspace/Enter aliases); `Ctrl+A` (0x01) chosen, documented in `input.rs` | pass | Encoding audit |

### Finding B — external trigger human setup (was disabled placeholder)

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Trigger option disabled on server without M005, enabled on current | `TaskScheduleDraft.external_trigger_capable` seeded from `WorkOrderCapabilities.supported`; `validate_draft` fails closed with M005 message when incapable; `task_schedule::tests::external_trigger_toggle_respects_capability` (Space toggles when capable, errors when not) | pass | No protocol change; existing flag reused |
| Enabled option creates `ExternalTrigger` gate; All/Any preserved | `work_orders::tests::capable_external_trigger_builds_gate_and_join` (gate `kind=external_trigger`, `trigger_ref=external`, join `all`/`any`); `build_work_order_create` pushes gate | pass | Existing gate set |
| WorkOrder failure performs zero trigger create | `isolation_tests::work_order_failure_performs_zero_trigger_create` (failure restores prompt, no trigger request/secret/metadata) | pass | §6.4 step 3 |
| WorkOrder success + trigger success shows one-time secret exactly once | `isolation_tests::trigger_success_shows_secret_once_and_caches_metadata` (`Dialog::TriggerSecret` mounted once, metadata cached) | pass | §6.4 step 6 |
| Trigger create uses authorized project scope + deterministic bounded key | `start_trigger_create` uses `capture_route` + `TaskTriggerCreateRequest{project_id, work_order_id}` + `trigger_creation_key` (bounded ≤128); `trigger_keys_are_bounded_and_stable` | pass | Daemon re-authorizes |
| Trigger failure leaves WorkOrder intact + setup-incomplete | `isolation_tests::trigger_failure_leaves_work_order_and_marks_setup_incomplete` (rows intact, `trigger_setup_error`, retry hint, label) | pass | No frontend delete |
| Retry with no durable trigger converges to one trigger | Stable `tui-setup-<wo>` key + `trigger_setup_reconciles_before_creating` (list-first, create only when empty; duplicate converges with no secret) | pass | Idempotent |
| Ambiguous timeout reconciles via metadata before new credential | `setup_trigger_for_selected` pends + `refresh_trigger_for_selected`; `apply_triggers_listed` creates only when no active trigger, else rotate path | pass | Case D |
| Stale/lost success drops display without persisting secret; active metadata + rotate | `isolation_tests::stale_trigger_success_drops_display_without_logging_secret` + `project_tab_switch_never_displays_bearer_in_wrong_scope` | pass | Cases A/B |
| Rotate revokes old then creates one replacement, displays only new secret | `rotate_trigger_for_selected` (revoke monotonic then create with `trigger_rotation_key`); reports via `WorkOrderTriggerCreated` (new bearer only) | pass | Explicit user mutation |
| Unauthorized/viewer fails closed | `isolation_tests::unauthorized_trigger_setup_fails_closed` (daemon `forbidden` → setup-incomplete, no bearer, no leak) | pass | Frontend hint only |
| Project/tab/reconnect stale never displays bearer in wrong scope | Stale tests above + `clear_trigger_secret_on_scope_loss` on `switch_active_tab`/`on_projection_reconnect`/`close_dialog` | pass | Fail closed |
| Bearer absent from debug/transcript/history/DTOs/audit/model projections | `trigger_bearer_never_in_view_debug_or_history`; `trigger_secret` dialog `Debug` redacted; `OneTimeBearer` `Debug` redacted, no `Serialize`; M005 `trigger_metadata_dto_carries_no_secret_material` + `check_task_trigger_boundaries.py` | pass | §6.5 rules |
| POST fire latches only trigger gate, materializes via coordinator | Reused M005 evidence: `work_orders_m005_trigger` (22 passed) including `trigger_fire_reaches_coordinator_exactly_once`, `http_post_fire_accepted_then_replayed`; no TUI session/job creation (`scheduling_placement_creates_no_session_or_job`) | pass | Narrow POST only |

### Finding C — queue reorder direct in scheduling queue (was modifier-only)

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Focused queue `j`/Down moves eligible future row later | `task_schedule::tests::focused_queue_jk_moves_insertion_with_pinned_head` (j/Down +1, clamped) | pass | Direct, no modifier |
| Focused queue `k`/Up moves it earlier | Same test (k/Up −1) | pass | Direct |
| Pinned running/claimed predecessor never moves | `queue_marker_never_crosses_pinned_head` (position 0 forbidden when head pinned) + `TaskViewState::moved_lane_order` pinned tests | pass | Pinned marker rendered |
| Boundary movement is no-op with stable selection | Same focused test (boundary stays, focus unchanged) + `queue_movement_does_not_change_focus_or_emit_agent_msgs` | pass | Stable |
| Stale revision yields refresh + `queue changed; retry`, no speculative order | Reused `reorder_conflict_refreshes_with_retry_notice` + placement uses fresh `LaneList` revision then CAS; conflict path refreshes | pass | CAS-first-writer-wins |
| Two concurrent reorders admit one CAS winner | `queue_concurrent_reorder_admits_one_cas_winner` (divergent intents, loser recomputes from winner) | pass | Pure + daemon CAS |
| Scheduling placement creates no fake session/Job dependency | `scheduling_placement_creates_no_session_or_job` (only `WorkOrderCreate`, no session, no extra command) | pass | Insertion marker only |

### Regression (M003/M005/M007 + surfaces)

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Ordinary Session Enter unchanged | `task_mode_enter_opens_sheet_without_touching_transcript` (Task only) + existing session tests in full suite | pass | — |
| Task-model preference separate from session preference | `remembered_task_model_falls_back_visibly_when_unknown` + `create_success_..._remembers_model` (Task pref path only) | pass | Unchanged |
| `/tasks`, `/task`, `/schedules`, `/workspace` unchanged except new trigger actions | `tasks_command_opens_view_not_schedule_list`; M004 dashboard untouched; Task view footer adds `t/T/X/e` only | pass | Additive keys |
| TaskTool vs `work_order` tool separation intact | No tool changes; `check_execution_ownership.py` ok | pass | — |
| M005 replay/revoke/expiry/max-fire green | `cargo test --features server --test work_orders_m005_trigger` 22 passed | pass | Backend untouched |
| M007 15-plan/recovery/ownership trajectory green | `cargo test --test work_orders_m007_trajectory` 14 passed | pass | Harness-only, no delta |

## 3. Production implementation evidence

Ownership (no second scheduler/session/trigger service/auth framework/workflow abstraction):

- `src/tui/input.rs` — root Tab owns `ToggleComposerMode` in both maps; `SwitchAgent` migrates to `Ctrl+A` with `Ctrl+G` alias; encoding audit documents why `Ctrl+I/H/M/J` are forbidden; help entries updated (Tab composer, `Ctrl+A` agent).
- `src/tui/app/state/work_orders.rs` — `TaskScheduleDraft` gains `external_trigger_capable`, `queue_insert_position`, `queue_expected_revision`; `TaskSheetField` gains `Queue`, `ExternalTrigger`; `validate_draft` allows trigger only when capable; `build_work_order_create` mints one `external_trigger` gate (`trigger_ref: external`); `describe_schedule_full` renders human All/Any summaries; queue helpers (`queue_insert_bounds`, `clamp/move_queue_insert_position`); secret types (`OneTimeBearer` redacted `Debug`, no `Serialize`; `OneTimeTriggerSecret` transient); deterministic keys (`trigger_creation_key`, `trigger_rotation_key`); `TriggerSetupStatus`.
- `src/tui/components/dialogs/task_schedule.rs` — seed carries per-lane order/revision/pinned + `trigger_capable`; dialog holds `external_trigger` + insertion marker; Queue focus consumes `j/k`/Up/Down (plus Shift+J/K alias) as direct movement with pinned clamp; Space toggles trigger only when capable; render shows queue preview (`pinned`/`▸new`) + capability-aware trigger row; confirm carries queue/trigger fields.
- `src/tui/components/dialogs/task_view.rs` — `t`/`T`/`X`/`e` trigger keys; footer updated; `TaskViewSnapshot.trigger_labels` (metadata only) rendered under rows.
- `src/tui/components/dialogs/trigger_secret.rs` (new) — one-time bearer dialog (ID, bearer, POST path, curl with `Authorization: Bearer` + `Idempotency-Key`, loss warning, `${CODEGG_BASE_URL}` template); Tab consumed; `Debug` redacted.
- `src/tui/commands/work_orders.rs` — `confirm_task_schedule` threads queue/trigger through draft/pending; `apply_work_order_created` chains placement + trigger create; `place_new_work_order_in_lane` (fresh `LaneList` + CAS `LaneReorder`); `start/apply_trigger_created` (one-time display, stale drop, duplicate→rotate hint, failure→setup-incomplete); `setup/refresh/apply_triggers_listed` (pending-set reconciliation before create); `revoke/apply_revoked`; `rotate` (revoke-then-create with fresh key); `close/clear_trigger_secret_on_scope_loss`; `trigger_labels_for_view`; `sync_task_view_dialog` attaches labels.
- `src/tui/app/types.rs` + `commands.rs` + `runtime/command_dispatch.rs` + `app/input.rs` — `Dialog::TriggerSecret`, `TuiMsg::TriggerSecretClose/TaskTriggerSetup/Revoke/Rotate/Refresh`, `TuiCommand::WorkOrderTriggerCreated/TriggersListed/TriggerRevoked` with route/generation guards.
- `src/tui/app/state/dialog.rs` + `app/mod.rs` (both constructors) + `project_session.rs` — `trigger_secret`, `trigger_create/manage_request`, `trigger_metadata`, `trigger_setup_error`, `trigger_setup_pending`; `close_dialog` drops bearer on secret close; tab switch/reconnect clear scope-loss bearer without logging.
- `src/tui/app/render.rs`, `commands/work_orders.rs` toggle toast — placeholder/help text updated from `Ctrl+G` to Tab.
- `src/tui/components/component.rs` — `DialogType::TriggerSecret` + round-trip test update.

Storage/protocol/compatibility: none. Reused M001 WorkOrder/lane tables, M003 runtime preference fields, M005 trigger/receipt tables; no migration. Reused `WorkOrderCreate`, lane list/reorder, `WorkOrderTriggerCreate/List/Get/Revoke`, POST fire; no new request family. Capability discovery reuses existing `WorkOrderCapabilities.supported` as the M005 hint (current daemons bundle them); older servers render the row disabled. Unknown additive trigger metadata still decodes compatibly. No version bump.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib -- tui::commands::work_orders
cargo test -p codegg --lib -- tui::app::state::work_orders
cargo test -p codegg --lib -- tui::components::dialogs::task
cargo test -p codegg --lib -- tui::components::dialogs::trigger_secret tui::input
cargo test --features server --test work_orders_m005_trigger
cargo test --test work_orders_m007_trajectory
cargo test --test tui_project_tabs
cargo test --test tui_project_picker
cargo test --test tui_project_routing
python3 scripts/check_task_trigger_boundaries.py
python3 scripts/check_work_order_coordinator.py
python3 scripts/check_tui_project_authority.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_execution_ownership.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

`work_orders_m003_foundation` does not exist as a test target in this repository (plan §13 anticipates this). Mapping used instead: `tui::commands::work_orders` (composer/sheet/view/trigger command layer) + `tui::app::state::work_orders` (composer/validation/gates/queue/secret pure state) + `tui::components::dialogs::task*` (sheet queue/trigger rows, view trigger keys, secret dialog) + `tui::input` (root Tab ownership). No test target was created solely to make the literal exist.

### Results

- `tui::commands::work_orders`: 30 passed (includes new C001 composer-preservation, queue-concurrency, placement-no-session, trigger success/duplicate/failure/stale/reconcile/redaction/labels/auth/scope tests).
- `tui::app::state::work_orders`: 16 passed (includes new capable-gate/join, queue bounds, key stability, bearer redaction, status labels).
- `tui::components::dialogs::task*` + `trigger_secret` + `tui::input`: 92 passed across the combined filter (includes new Tab-ownership, queue-movement, trigger-toggle, view trigger keys, secret redaction/curl tests).
- `work_orders_m005_trigger` (server): 22 passed (verifier-only storage, one-time secret, auth matrix, replay/race/restart, HTTP POST-only/bounds/privacy, coordinator exactly-once).
- `work_orders_m007_trajectory`: 14 passed (15-plan end-to-end, lane/restart/reorder-hold, gate joins, repeat, fault windows, agent batches, permission/model drift, trigger replay/security, team contention).
- `tui_project_tabs`: 20 passed. `tui_project_picker`: 22 passed. `tui_project_routing`: 27 passed.
- `check_task_trigger_boundaries.py`: ok. `check_work_order_coordinator.py`: ok. `check_tui_project_authority.py`: passed. `check_authorization_matrix.py`: all invariants verified. `check_execution_ownership.py`: ok. `check-core-boundary.sh`: passed.
- `cargo fmt --all -- --check`: clean (after `cargo fmt --all`).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- `./scripts/verify.sh quick`: passed (fmt, agent schema, core-boundary, sandbox, execution-ownership, TUI authority, workspace check).

No verification was skipped or simulated. Hosted CI was not invoked as a new lane (per plan §13 and registry verification policy).

## 5. Invariant review

| Plan invariant | Evidence |
|---|---|
| TUI is not durable authority; no direct WorkOrder/trigger table writes | TUI uses only `CoreRequest` via `CoreClient`; `check_tui_project_authority.py` passed; `check_work_order_coordinator.py` passed |
| WorkOrder creation uses canonical `WorkOrderCreate`/M001 path | `confirm_task_schedule` → single `WorkOrderCreate`; placement/trigger are post-create chains, never replacements |
| Trigger management uses M005 service; fire stays narrow POST | `start_trigger_create`/revoke/rotate/list use management variants; no `TriggerFire` Core variant (guard pins); fire covered by M005 HTTP tests |
| `WorkOrderCoordinator` only release owner; starts reach `JobSubmissionService` | Untouched; `check_work_order_coordinator.py` ok; placement never edits Job dependencies (test) |
| `InputMode` unchanged by composer selection | Toggle tests assert Insert/Normal preserved |
| Session default after startup/new session/tab | `ComposerMode::default() == Session`; toggle only via explicit Tab/`Ctrl+G`; no persistence of Task default |
| Task-model preference separately scoped | Unchanged paths; fallback/remember tests green |
| Prompt editable/unsubmitted until confirm succeeds | Sheet keeps widget text; failure restores exactly once; stale success never wipes newer typing |
| No frontend rollback after late/stale success | Stale completions drop without delete; `tab_close_cancels_ui_only_never_daemon_work` |
| Lane reorder future/unclaimed only, head pinned, CAS revision | Pinned-head pure math + dialog clamp + `expected_revision` on every reorder; conflict refreshes |
| Bearer never principal credential / never authorizes Core | Guard `check_task_trigger_boundaries.py` ok; no `TriggerFire` variant; fire path never resolves principals |
| Plaintext never persisted/exposed to model/tools | Redacted `Debug`, no `Serialize`/SQLite/transcript/history/audit/event; tests assert absence; guard ok |
| Gate All/Any, repeat, delay/not-before, sequence-ready, model/policy, worktree/session unchanged | `capable_external_trigger_builds_gate_and_join` + M007 trajectory green |

## 6. Failure and recovery review

- Duplicate delivery/idempotency: WO create uses per-confirm key; trigger create uses stable `tui-setup-<wo>` / `tui-trigger-<req>-<wo>` keys; duplicate trigger returns metadata with `secret: None` → active + rotate hint, no second bearer. Tests: duplicate-converges, setup-reconciles.
- Cancellation races: `close_dialog` cancels sheet/view/secret requests + bumps view generation; tab switch/reconnect clear bearer and cancel; stale completions drop. Tests: stale create/view/trigger, tab-switch scope, scope-loss clear.
- Daemon restart: trigger/lane truth is durable; TUI refreshes view/metadata on foreground; M005 restart test green, M007 restart convergence green.
- Partial persistence failure: WO success + trigger failure → setup-incomplete + retry (no delete); trigger response unusable → rotate path. Tests: failure-leaves-WO, duplicate, bearer-unusable.
- Stale generation/lease: every trigger/lane/view completion carries route + request/generation; mismatches drop without logging secrets. Tests: stale trigger/list/revoke, other-project drops.
- Contention/resource release: lane CAS first-writer-wins + `queue changed; retry`; concurrent setup converges via stable key + list-first; rotation is explicit revoke-then-create. Tests: queue-concurrent, reorder-conflict, setup-reconciles.
- Malformed/unauthorized input: validation fails closed with actionable messages; daemon `forbidden` → setup-incomplete fail-closed; opaque privacy preserved (no existence oracle beyond existing shapes). Tests: unauthorized-fails-closed, incapable-trigger error.
- Bounded event/artifact: toasts carry IDs/status only, never bearers; curl line exists only in transient dialog render, never in toast/history/audit. Tests assert absence.

## 7. Migration and compatibility review

- Schema migration: none. No new tables/columns; `STORAGE_LAYOUT_VERSION` unchanged. Existing M005 `migrate_v63`/layout 63 retained (guard pins).
- Backward compatibility: old server without WorkOrders → existing M003 compatibility path (sheet/view gate on `supported`); WorkOrder-capable but trigger-incapable → trigger row visibly disabled, confirm fails closed with M005 message. Current server → row enabled, full flow active. Current client vs newer server → additive trigger metadata decodes compatibly (serde defaults).
- Protocol: none. No new `CoreRequest`/`CoreResponse`/`CoreEvent` family beyond the existing M005 management variants already landed; capability discovery reuses `WorkOrderCapabilities.supported`. No version bump per additive rules.
- Configuration: no new config keys. Keymap change is a default-binding migration; user overrides in `KeybindConfig` still win via `build_bindings` overlay.
- Rollback: deleting the C001 frontend restores M003 `Ctrl+G`/disabled-trigger behavior; durable WorkOrders/triggers/lanes remain valid (no migration to roll back).

## 8. Security review

- Authorization: WO/trigger create/revoke use existing project-scoped descriptors; frontend capability checks are hints. Viewer/unauthorized `forbidden` fails closed with setup-incomplete, no bearer, no oracle beyond existing privacy shapes (test).
- Secret handling: `OneTimeBearer` (redacted `Debug`, no `Serialize`, bounded length, `clear` on drop) + `OneTimeTriggerSecret` (transient, route-bound). Never in prompt/history/transcript/notifications/audit/events/DTOs/SQLite/model context (tests + `check_task_trigger_boundaries.py`). Clipboard integration not added (no new dependency). No secret-read API added; rotation is the only replacement path.
- Path validation: `trigger_ref` fixed to `external` (non-path, bounded); lane/work-order/trigger IDs are typed opaque IDs resolved server-side.
- Privilege boundaries: TUI never creates sessions/jobs directly; fire bearer never becomes principal (guard pins `cggtr_` vs `cggt_`, no principal resolution on fire path, no `TriggerFire` Core variant).
- DoS bounds: sheet buffers ≤128 chars; bearer ≤512; trigger IDs ≤128; idempotency keys ≤128; list limits ≤20; view rows ≤200/50; curl/base-URL never guesses `localhost` for remote daemons.
- Redaction/audit: observability records structural IDs/state/action only; bearer/auth header/prompt/curl-token never in logs (guard scans fire/trigger/daemon paths; TUI toasts/labels carry metadata only).

## 9. Documentation and operations

- `architecture/work_orders.md` — C001 UX correction (Tab ownership, queue editor, trigger gate/summary), Task-view trigger management, and new human trigger-setup/secret section (no backend redesign).
- `architecture/tui.md` — root Tab ownership, migrated `SwitchAgent`, modal-local Tab, trigger completions, queue/secret dialog/state.
- `.opencode/skills/tui/SKILL.md` — work-orders command row updated for Tab/queue/trigger/secret scope-loss contract.
- User-facing help/default keybinding tables (`src/tui/input.rs` `default_help_entries`) — Tab composer + `Ctrl+A` agent in both modes; `Ctrl+G` marked alias.
- This closure record + `plans/registry.md` + corrective addendum + implementation-plan status (see §12).

Operator recovery: trigger setup-incomplete → `t` in Task view (reconciles, idempotent retry); lost bearer → `T` to rotate (revoke + create, new bearer only); revoked/expired/exhausted → `t` for a new bearer; queue conflict → `queue changed; retry` + canonical refresh; stale/ambiguous → metadata (`e`) before any new credential; authority loss → fail closed, re-authenticate, refresh.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Queue preview pin is a Task-view-cache hint; a lane whose head started running after the sheet opened shows the pin only after the next prefetch | Insertion marker could initially allow position 0 when the head just pinned; durable CAS + daemon validation still prevent unsafe jumps, and refresh corrects the preview | None for closure; future sheet prefetch may include occurrence states if a cheap lane-head signal lands |
| low | Trigger expiry/max-fire are daemon-managed; the sheet does not expose expiry/max-fire editors | Human triggers are created without expiry/cap; rotation/revoke cover replacement, and defaults match the narrow M005 capability | None for closure; a future milestone may add bounded expiry/max-fire fields if product priority justifies it |
| low | Full base URL is a `${CODEGG_BASE_URL}` template, not auto-detected | External scripts must substitute their reachable daemon base; avoids guessing `localhost` for remote daemons | None for closure; document as intentional |

No critical/high/medium findings. No new backend correctness/security defect was found; no split trigger-backend corrective plan was needed.

## 11. Roadmap disposition

- C001 closed. The UX-fidelity corrective subsystem (`project-work-orders-task-view-ux-corrective-addendum.md` C001) is complete; no second corrective pass is required.
- Predecessor M003/M005/M007 closure records stand unchanged (history not rewritten; this record references them and states exactly what changed).
- No future plan is unblocked by C001 beyond closing the corrective itself: the registry's blocked work (dependency-security M005 updater interface, architecture-convergence M009 operational evidence, runtime-safety C002 Linux fixture) has no C001 dependency, and no registered implementation plan lists C001 as a hard prerequisite. The main Project Work Orders workstream remains closed.

## 12. Registry updates

- `plans/registry.md`: corrective subsystem `Project Work Orders and Task View — UX fidelity corrective` → `closed` (C001 closed); dependency-ready table C001 row → `closed` with closure link `plans/closure/project-work-orders-task-view-corrective/001-status.md` and implementation `1bd77c6d`; execution-order gate (1) notes C001 closed; closure-work control row for the corrective → closed; recently-closed table gains the C001 row. Blocked-work table unchanged (M005 updater, M009, C002 remain as before; nothing unblocked).
- `plans/subsystems/project-work-orders-task-view-ux-corrective-addendum.md`: `Status: ready` → `Status: closed` with C001 exit conditions met.
- `plans/implementation/project-work-orders-task-view-corrective/001-human-task-ux-and-trigger-surface.md`: `Status: ready` → `Status: closed`.

## Appendix — before/after keybinding table (C001)

| Context | Before (M003) | After (C001) | Notes |
|---|---|---|---|
| Root prompt, bare Tab | `SwitchAgent` | `ToggleComposerMode` (Session↔Task) | Exactly one owner; both Insert+Normal |
| Root prompt, `Ctrl+A` | (unbound) | `SwitchAgent` | Portable 0x01; configurable/discoverable |
| Root prompt, `Ctrl+G` | `ToggleComposerMode` (sole route) | `ToggleComposerMode` (alias) | Backward-compatible, identical action |
| Root prompt, Shift+Tab | `TogglePermissionMode` | `TogglePermissionMode` | Unchanged |
| TaskSchedule modal, Tab/Shift+Tab | Field focus (consumed) | Field focus incl. new Queue/ExternalTrigger (consumed) | Never toggles composer underneath |
| TaskView modal, Tab | Consumed (`None`) | Consumed (`None`) | Never leaks to composer/agent |
| TriggerSecret modal, Tab | (new) | Consumed (`None`) | Never leaks |
| TaskView `t`/`T`/`X`/`e` | (unbound) | Setup/rotate/revoke/refresh | Metadata only |
| Scheduling queue, `j/k`/Up/Down | (unbound in sheet; Task view `j/k` navigate, Shift+J/K reorder) | Direct insertion movement when Queue focused; Task view navigation/Shift+J/K retained | Focus-scoped only |

Terminal-encoding audit: `Ctrl+A` (0x01) and `Ctrl+G` (0x07) are distinct from Tab (0x09). `Ctrl+I` (=Tab), `Ctrl+H` (=Backspace), `Ctrl+M` (=Enter), `Ctrl+J` (LineFeed) are never used as the replacement. Leaving `SwitchAgent` unbound was rejected in favor of the safe `Ctrl+A` default; the action remains fully remappable via `KeybindConfig`.

## Appendix — end-to-end Task-with-trigger trace (secret redacted)

Covered by the landed suites (no live provider/network):

1. Root Session → bare Tab → Task → prompt → Enter → sheet prefetch (`WorkOrderCapabilities` + `LaneList` + `Summary` + Task-model pref).
2. Sheet: sequential lane + focused queue insertion (`j/k`) + external trigger on (capable) → confirm → `validate_draft` + `build_work_order_create` (sequence + `external_trigger` gates, All/Any join).
3. `WorkOrderCreate` → durable WorkOrder (daemon-owned) → prompt cleared, history recorded, Task-model persisted best-effort.
4. Placement: fresh `LaneList` + CAS `LaneReorder` to the selected insertion position (conflict → `queue changed; retry` + refresh).
5. `WorkOrderTriggerCreate` (project-authorized, stable idempotency key) → `WorkOrderTrigger{metadata, secret: REDACTED, duplicate: false}` → transient `TriggerSecret` dialog (ID, bearer REDACTED, `POST /api/v1/task-triggers/<id>/fire`, curl with `Authorization: Bearer REDACTED`).
6. External `POST /api/v1/task-triggers/<id>/fire` with `Authorization: Bearer REDACTED` + `Idempotency-Key` → `accepted`/`already_fired` receipt (M005 HTTP tests) → coordinator latches only the external gate → materializes through `JobSubmissionService` once other gates permit (M005 `trigger_fire_reaches_coordinator_exactly_once`, M007 trajectory).

Partial-success/stale/ambiguous/rotation evidence: §2 Finding B rows + §6. Queue/CAS evidence: §2 Finding C rows + M007 lane/restart tests. Authorization/privacy: §2 trigger rows + M005 management matrix + opaque-privacy shapes.
