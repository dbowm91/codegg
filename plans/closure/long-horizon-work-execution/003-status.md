# Long-Horizon Work Execution M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/long-horizon-work-execution/003-work-plan-projection-and-completion-arbiter.md`

Source subsystem roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Repository baseline reviewed: `18365458f881f6ac4524c9ea05224b69923faa4f`

Implementation commits or pull requests:

- `2ff09bef` — long-horizon M003: WorkPlan projection and completion arbiter

## 1. Executive finding

M003 is complete. The durable WorkPlan foundation (M002) is now usable by
the primary agent without flooding context: `work_plan_get` exposes the
bounded current/actionable slice with pagination/lookup,
`work_plan_update_item` permits revision-checked status/blocker/next-action
updates while forbidding host-evidence fabrication, TodoState projects a
small subset under the resolved `TaskStatePolicy` with exact-revision
feedback, canonical job/run evidence drives a pure five-state completion
assessment, and the host-owned arbiter gates the terminal-answer boundary
for ordinary turns and Goal-bound completion. An unfinished required plan
cannot be marked complete; a truly complete plan exits without extra model
turns; GoalVerification remains the final Goal authority. No second plan
representation, workflow scheduler, context-epoch reset, or CI lane was
introduced. This unblocks M004.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Pure completion assessment (§6 core) | `crates/codegg-core/src/work_plan/assessment.rs::WorkPlanCompletionAssessment::{Complete, ActionableWorkRemaining, Blocked, AwaitingUserJudgment, InFlight}`, `assess_work_plan()` | pass | Read-only/deterministic; `Blocked` is WorkPlan state, not GoalStatus; `assessment_state_matrix` + `evidence_authority_rules` |
| Bounded read surface (§6 tools) | `src/tool/work_plan.rs::WorkPlanGetTool` (`work_plan_get`), `project_work_plan()` default limit 5/max 8/4096 bytes, `lookup_item_summary()` | pass | Defaults to current/actionable; completed history requires `include_completed` + pagination; `default_projection_excludes_completed_history`, `large_plan_stays_bounded_with_pagination` |
| Revision-checked mutation (§6 tools) | `src/tool/work_plan.rs::WorkPlanUpdateItemTool` (`work_plan_update_item`): requires `expected_revision`, validates `can_transition_item`, permits blocker/next-action, forbids `dependencies`/`acceptance`/`evidence`/`objective`/`owner_*`, returns stale conflicts | pass | `work_plan_tools_enforce_bounds_and_authority`; stale writers get explicit conflict, never last-write-wins |
| Todo one-way authority (§6 Todo) | `crates/codegg-core/src/work_plan/todo_projection.rs::{project_to_todo_items, parse_todo_work_ref, validate_todo_feedback}`, `src/work_plan_todo_sync.rs::{try_translate_todo_feedback, project_plan_to_session_todos}` | pass | WorkPlan selects current/actionable; TodoState projects per policy cap; `wi_*@rN` exact mapping; Todo `completed` alone never satisfies host-only (`HostEvidenceRequired`); `todo_projection_respects_all_modes_and_caps`, `stale_todo_mapping_cannot_mutate_newer_item`, `todo_completed_alone_never_completes_host_only_item` |
| Host evidence correlation (§6/WP-C) | `crates/codegg-core/src/work_plan/evidence.rs`, `src/work_plan_evidence.rs::assemble()` (job store for Test/Scheduler/Delegated, `agent_run` table with job fallback, Artifact/Commit stay `Unavailable`) | pass | Passing/failed/in-flight canonical tests produce correct assessment; claimed text alone never satisfies; `failed_test_remains_unmet_actionable`, `in_flight_job_yields_wait_behavior` |
| Ordinary + Goal-bound arbiter (§6) | `src/work_plan_arbiter.rs::{check_ordinary_completion, check_goal_completion_gate, decide_from_assessment, build_required_work_message, record_verifier_feedback, maybe_complete_plan_on_turn_end}`, `src/agent/loop.rs` terminal-answer hook + turn-end close | pass | Actionable injects bounded continuation within existing turn/tool/time/token limits; `InFlight` polls live handle; Goal gate requires Complete/AwaitingUserJudgment before verifier; verifier `NotMet` records one actionable `next_action` via CAS when mapping exists; `unfinished_plan_blocks_completion_and_complete_plan_exits`, `goal_bound_plan_requires_both_workplan_and_verifier` |
| Turn-scoped close semantics (§6/§8) | `maybe_complete_plan_on_turn_end()` (CAS to `Completed` only when assessment Complete and budgets intact; preserves on expiry; skips Goal-bound plans) | pass | `budget_expiry_preserves_remaining_state`; stale/inflight/blocked never auto-complete |
| Protocol/frontends (§6) | `codegg-core::bus::events::{WorkPlanSnapshot, AppEvent::WorkPlanUpdated}` (`work_plan:updated`) | pass | Additive/bounded; frontends render only; mutations stay in tools/daemon service |
| Security/authorization (§6) | Session-ownership checks in both tools; child `caller_run_id` must match `owner_run_id`; evidence shape already closed-kind/bounded; no secrets logged | pass | `work_plan_tools_enforce_bounds_and_authority` (cross-session, sibling-run, forbidden-field, stale cases) |
| Docs/static guards (§6) | `architecture/work_plan.md` (M003 section), `goal.md` (arbiter gate), `model_profile_task_state.md` (one-way contract), `agent.md` (terminal gate + turn-end close); existing `check-core-boundary.sh` + `check_project_catalog_invariants.py` | pass | No new CI lane per verification policy; tests guard fabrication and caps |

## 3. Production implementation evidence

Core/domain (`codegg-core`, boundary-clean):

- `crates/codegg-core/src/work_plan/assessment.rs` (new): five-state
  assessment, `reason_code()`/`allows_completion()`/`requires_continuation()`,
  bounded previews (`MAX_ASSESSMENT_TEXT_CHARS = 200`,
  `MAX_ASSESSMENT_REASON_CHARS = 500`), precedence
  actionable → in-progress(failed/live/owned/progress) → blocked →
  dependency-gated → user-judgment → generic blocked; completed-without-host
  reads actionable. Seven unit tests.
- `crates/codegg-core/src/work_plan/projection.rs` (new):
  `MAX_PROJECTION_ITEMS = 8`, `MAX_PROJECTION_BYTES = 4096`,
  `MAX_PROJECTION_TEXT_CHARS = 200`,
  `MAX_PROJECTION_OBJECTIVE_CHARS = 300`; `WorkPlanProjectionParams`
  (default offset 0/limit 5/no-completed), stable current-then-actionable
  ordering, pagination, byte-cap truncation with `truncated` flag,
  `lookup_item_summary()`. Three unit tests.
- `crates/codegg-core/src/work_plan/evidence.rs` (new):
  `HostEvidenceStatus`, `WorkPlanEvidenceSnapshot` (`<kind>:<ref_id>` keys,
  missing → `Unavailable`), `item_is_satisfied()` (Satisfied OR Passed;
  owner alone never satisfies), failed/in-progress/unavailable helpers,
  `item_is_user_judgment_only()`. Four unit tests.
- `crates/codegg-core/src/work_plan/todo_projection.rs` (new):
  `TODO_WORK_REF_SEPARATOR = "@r"`, `todo_id_for_work_item()`
  (`wi_*@rN`), `parse_todo_work_ref()`, `project_to_todo_items()` (Disabled
  0, Sparse 8, Explicit 10, Guided 4, ≤1 `InProgress`, no terminal history),
  `validate_todo_feedback()` (exact revision, `can_transition_item`,
  blocker discipline, `HostEvidenceRequired` for host-only completion).
  Five unit tests.
- `crates/codegg-core/src/work_plan/mod.rs`: registers the four modules
  and re-exports.
- `crates/codegg-core/src/bus/events.rs`: additive `WorkPlanSnapshot`
  (bounded previews/counts/assessment code) and
  `AppEvent::WorkPlanUpdated` (`work_plan:updated`).

Application (`codegg` root):

- `src/tool/work_plan.rs` (new): `work_plan_get` (ReadOnly; active-plan
  default, `plan_id`/`item_id`/`offset`/`limit`/`include_completed`) and
  `work_plan_update_item` (SafeMutating; requires `item_id` +
  `expected_revision`; optional `status`/`blocker`/`next_action`/
  `caller_run_id`; forbids host-owned fields; session-ownership + child-run
  checks; host-evidence gate for `Completed`; publishes `WorkPlanUpdated`;
  refreshes durable Todo projection). Registered in
  `src/tool/mod.rs::with_options()` whenever pool+session exist.
- `src/work_plan_evidence.rs` (new): `assemble()` over plan items (≤256
  refs) reusing `SqliteJobStore::get_job` and the `agent_run` table;
  store failures yield `Unavailable`, never `Passed`.
- `src/work_plan_arbiter.rs` (new): `ArbiterDecision`
  (Allow/Continue/Wait/Blocked/NeedsUser/Judgment/Inconclusive),
  `build_required_work_message()` (≤1000 chars: current item, unmet count,
  next action), `assess_active_plan()`/`assess_goal_plan()`,
  `decide_from_assessment()`, `check_ordinary_completion()`,
  `check_goal_completion_gate()`, `record_verifier_feedback()` (one
  actionable `next_action` via CAS, else no mutation),
  `maybe_complete_plan_on_turn_end()` (turn-scoped only, host-passed and
  budget-intact). Three unit tests.
- `src/work_plan_todo_sync.rs` (new): `try_translate_todo_feedback()`
  (best-effort ≤12 todos; skips unmapped/stale/owned/invalid/host-gated;
  returns updated count) and `project_plan_to_session_todos()` (durable
  projection for restart).
- `src/tool/todo.rs`: `TodoWriteTool` attempts exact-revision WorkPlan
  feedback after a successful write (never fails the write) and refreshes
  the durable projection when any mapped item advances.
- `src/tool/goal.rs`: `GoalRequestCompletionTool` checks the bound-plan
  gate before and after `GoalVerificationService`; non-allowing assessments
  return bounded `not_met` with `work_plan_id`/`work_plan_assessment`;
  verifier `NotMet` records one actionable `next_action` when mapping
  exists.
- `src/agent/loop.rs`: terminal-answer hook (actionable → bounded
  continuation prompt + continue; in-flight → wait prompt + continue;
  blocked/inconclusive → typed report without completion; judgment/complete
  → return control) within existing turn/tool/time/token limits, plus
  turn-end close for ordinary plans only.
- `src/lib.rs`: re-exports `work_plan`; registers the three modules.

### Plan/Todo authority matrix

| Direction | Rule | Enforcement |
|---|---|---|
| WorkPlan → Todo | Plan selects current/actionable; Todo renders ≤ policy cap | `project_to_todo_items()` + `project_plan_to_session_todos()`; caps per mode; no terminal history |
| Todo → WorkPlan | Only exact `wi_*@rN` + valid transition + blocker discipline | `validate_todo_feedback()`; `Stale`/`IdentityMismatch`/`InvalidTransition` reject; stale Todo test |
| Todo `completed` → host acceptance | Never inferred alone | `HostEvidenceRequired` unless Satisfied/Passed; `todo_completed_alone_*` tests |
| Child reporting | Assigned items need matching `caller_run_id`; Todo path never crosses owner boundary | Tool rejects sibling/other-run writes; `try_translate_todo_feedback()` skips owned items |
| Objective/deps/evidence | Immutable/host-owned; no model rewrite | Tool rejects forbidden fields; `work_plan_tools_enforce_bounds_and_authority` |

### Completion-assessment evidence matrix

| State | Trigger | Arbiter effect |
|---|---|---|
| `Complete` | No required items and every Completed is host-satisfied (or plan terminal) | Allow completion; ordinary turn may CAS-close when budget-intact |
| `ActionableWorkRemaining` | Actionable item with deterministic unmet work; Completed-without-host; failed evidence; progress-less InProgress | Bounded continuation prompt; Goal gate blocks verifier |
| `Blocked` | Plan `Blocked`; item `Blocked`; dependency-gated remainder | Typed blocked report; state preserved; no auto-complete |
| `AwaitingUserJudgment` | Only judgment criteria remain | Return control; Goal gate defers to verifier (which AwaitsUser for criteria/risks) |
| `InFlight` | Live `InProgress` evidence or owner handle with no actionable | Wait/poll existing handle; terminal evidence reloaded next turn |

### Tool schemas

- `work_plan_get`: `{plan_id?, item_id?, offset(0-64), limit(1-8), include_completed?}`.
- `work_plan_update_item`: `{item_id, expected_revision, status?, blocker?, next_action?, caller_run_id?}`; forbids `dependencies`, `parent_item_id`, `acceptance`, `evidence`, `objective`, `origin_provenance`, `owner_run_id`, `owner_job_id`.

Tests:

- `crates/codegg-core/tests/work_plan_projection_arbiter.rs` (new, 7
  tests): assessment matrix, bounded pagination, all-mode Todo policy,
  stale feedback, evidence authority, file-backed restart, legacy absence.
- `tests/work_plan_projection_arbiter.rs` (new, 9 tests): early-final
  continuation → host completion → turn-end close, budget-expiry
  preservation, Goal-bound dual gate, failed-test actionable, in-flight
  wait, user-judgment control, tool bounds/authority, Todo-completed
  isolation, legacy behavior.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib -- work_plan
cargo test -p codegg-core --test work_plan_foundation
cargo test -p codegg-core --test work_plan_projection_arbiter
cargo test --test work_plan_projection_arbiter
cargo test --test goal_continuation_progress
cargo test --test storage_migrations
cargo test -p codegg --lib -- tool::goal
cargo test -p codegg --lib -- work_plan
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
bash scripts/check-core-boundary.sh
python3 scripts/check_project_catalog_invariants.py
```

Planned-command deviations (per plan §11, recorded here rather than hidden):

- `cargo test -p codegg-core -- work_plan` as written matches no lib-target
  filter form in this repo layout; the current focused equivalents are
  `cargo test -p codegg-core --lib -- work_plan` (42/42) plus the two
  integration targets below.
- `cargo test --test agent_loop_harness -- todo` / `-- completion` do not
  exist as filtered harnesses for this milestone; the current equivalents
  are the new `cargo test --test work_plan_projection_arbiter` (9/9)
  early-final/arbiter coverage and the existing
  `cargo test --test goal_continuation_progress` (9/9) continuation guard.
- `cargo test --test goal_verification` has no such target; the current
  equivalent is `cargo test -p codegg --lib -- tool::goal` (12/12), which
  exercises the verifier through the Goal tool including the new bound-plan
  gate.
- `python3 scripts/check_core_boundary.py` does not exist; the current
  equivalent is `bash scripts/check-core-boundary.sh` (run, pass).

### Results

- `cargo test -p codegg-core --lib -- work_plan`: 42/42 pass (11 model +
  9 store + 7 assessment + 4 evidence + 4 projection + 5 todo-projection +
  2 arbiter-adjacent lib tests).
- `cargo test -p codegg-core --test work_plan_foundation`: 10/10 pass
  (M002 regression intact).
- `cargo test -p codegg-core --test work_plan_projection_arbiter`: 7/7
  pass.
- `cargo test --test work_plan_projection_arbiter`: 9/9 pass.
- `cargo test --test goal_continuation_progress`: 9/9 pass.
- `cargo test --test storage_migrations`: 4/4 pass.
- `cargo test -p codegg --lib -- tool::goal`: 12/12 pass.
- `cargo test -p codegg --lib -- work_plan`: 4/4 pass.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `./scripts/verify.sh quick`: pass (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution ownership, TUI project
  authority, workspace check).
- `bash scripts/check-core-boundary.sh`: pass.
- `python3 scripts/check_project_catalog_invariants.py`: 7/7 pass.

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| WorkPlan authoritative; Todo is projection | One-way contract + matrix §3; projection caps per mode; terminal history never in Todo; `projection_never_includes_terminal_history` |
| Model tools cannot mark host-only evidence satisfied | Forbidden-field rejection + Completed host gate + Todo `HostEvidenceRequired`; `work_plan_tools_enforce_bounds_and_authority`, `todo_completed_alone_*` |
| Todo injection stays bounded per TaskStatePolicy | Existing injection policy untouched; projection truncates to mode caps; `todo_projection_respects_all_modes_and_caps` |
| Arbiter cannot force unbounded continuation | Continuation re-enters the existing loop; `check_limits()` (turns/tools/tokens/timeout) plus 32-cycle goal cap remain authoritative; no new retry loop; `budget_expiry_preserves_remaining_state` |
| Natural-language/user-judgment never guessed | Judgment-only → `AwaitingUserJudgment`/`NeedsUserJudgment`; verifier still `AwaitingUser` for criteria/risks; `user_judgment_returns_control_without_fabrication` |
| Goal completion still requires GoalVerification | Gate is prerequisite only; `Met` path re-checks the bound plan then calls `complete_if_active`; `goal_bound_plan_requires_both_workplan_and_verifier` |
| Subagent/Job/Run/scheduler authority unchanged | Owner refs stay provenance-only; child writes need exact `caller_run_id`; Todo path skips owned items; no scheduler/executor change; boundary guard passes |

## 6. Failure and recovery review

| Concern (§8) | Evidence |
|---|---|
| Stale plan/item writes fail without silent reapply | `Conflict{expected,found}` on store paths; tool surfaces `stale work item revision`; `stale_todo_mapping_cannot_mutate_newer_item`; concurrent M002 tests still green |
| Plan/evidence read failure never marks complete | `assess_*` maps errors to `String`; loop publishes `Error` and preserves state; `Inconclusive` decision never allows completion; tool `unwrap_or_default()` snapshot falls back to empty (fail-closed: missing = `Unavailable`) |
| User cancel/Goal pause cancels continuation, preserves items | Steering/`cancel_rx` checked per goal-continuation cycle (unchanged); arbiter continuation re-enters the same cancellable loop; turn-end close skipped when `check_limits()` reports expiry/interruption; `budget_expiry_preserves_remaining_state` |
| InFlight terminal between assessment and next turn reloads | Wait prompt polls the existing handle without relaunch; next boundary reassembles evidence from stores (`assess_active_plan` per check); `in_flight_job_yields_wait_behavior` plus existing `resume_with_live_job_still_verifies_wait` |
| Child races use revision/CAS; duplicate completion idempotent where evidence matches | Item mutations bump plan revision in-tx (M002); Todo feedback and verifier-feedback paths return `false` on `Conflict`/`Terminal` instead of retrying; assessment is deterministic on `(position,id)` order |
| Restart reconstructs from WorkPlan/Todo stores, never transcript | File-backed reopen test asserts same revision/items plus identical projection/assessment; durable Todo projection via `project_plan_to_session_todos()` + `load_persisted_todos()`; `projection_reconstructed_after_file_backed_reopen` |

## 7. Migration and compatibility review

No schema migration (M002 v59 unchanged; `STORAGE_LAYOUT_VERSION` stays
59). Sessions without a WorkPlan follow legacy behavior:
`check_ordinary_completion()` returns `None` (no hook),
`check_goal_completion_gate()` returns `AllowCompletion` (verifier path
unchanged), and the Goal tool gate is skipped when `assess_goal_plan()`
finds no bound plan (`work_plan_absent_session_uses_legacy_behavior`).
Existing Todo tools keep their names and policies; WorkPlan-aware sessions
add `wi_*@rN` Todo ids and the two `work_plan_*` verbs. Goal completion for
Goals without a bound plan is the existing verifier path (12/12 tool tests
green). `AppEvent::WorkPlanUpdated` is additive with a bounded snapshot;
existing TUI/ACP clients that ignore it remain functional.
`storage_migrations` (4/4) and continuation-checkpoint tests remain green.
Rollback is a plain revert of `2ff09bef` (new tables unused by older code
stay at v59; new events/tools are additive).

## 8. Security review

Plan mutations carry owning session identity (`plan.session_id ==
session_id` on both tools; cross-session rejected). Delegated children
report only against host-assigned items: owned items require matching
`caller_run_id`; sibling/other-run writes rejected; Todo feedback skips all
owned items. Evidence refs are shape-validated (closed kind + bounded
id/detail, NUL rejected in domain validation) and resolved against
scope/goal/run provenance via the job store goal label and same-session
Goal binding (M002 scope checks unchanged); a referenced run/job grants no
capability and Artifact/Commit refs never satisfy without host `Satisfied`.
All text/ID fields reject NUL; scope IDs ≤256; diagnostics carry bounded
previews and reason codes only (no secrets, no full plan dumps).
`codegg-core` boundary guard passes (no UI/server/plugin/auth deps).

## 9. Documentation and operations

- `architecture/work_plan.md`: M003 projection/arbitration section
  (assessment precedence, projection caps, evidence mapping, Todo contract,
  arbiter/turn-end/Goal-gate behavior, protocol event, updated test
  commands).
- `architecture/goal.md`: WorkPlan-binding section now documents the
  Complete/AwaitingUserJudgment prerequisite and `NotMet` actionable-item
  recording.
- `architecture/model_profile_task_state.md`: WorkPlan-relation section
  now documents the one-way projection caps, `wi_*@rN` mapping, feedback
  validation, and restart behavior.
- `architecture/agent.md`: execution lifecycle now documents the
  terminal-answer arbiter gate and ordinary-plan turn-end close.
- Tool surface: `work_plan_get`/`work_plan_update_item` schemas in
  `src/tool/work_plan.rs` (registered in `ToolRegistry::with_options()`
  when pool+session exist); catalog picks them up automatically.
- Operator signals: `assessment.reason_code()`
  (`complete`/`actionable_work_remaining`/`blocked`/
  `awaiting_user_judgment`/`in_flight`), `work_plan:updated` events,
  `stale work item revision` diagnostics, `WorkPlan feedback: N mapped
  item(s) updated` Todo notes. Existing Goal/Todo surfaces unchanged
  except the additive gate/feedback.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Artifact/Commit evidence refs resolve as `Unavailable` without a dedicated host existence probe | Items relying solely on Artifact/Commit refs need a host `Satisfied` acceptance to complete; no silent fabrication, but no automatic artifact-to-satisfaction bridge yet | M004/M005 trajectory work may add narrow host probes if representative plans need them; no M003 corrective pass required |
| low | `Actionable` status labels are still read-time computation (M002 carryover) | Callers must use `actionable_items()`/assessment, not status equality | M003 assessment/projection consume the computation; no corrective pass required |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. Dependency audit for the
unblock check (registry Blocked-work section + roadmap §6 dependency graph):

- M003 projection + completion arbiter: hard dep on M002 → was ready, now
  closed.
- M004 context-epoch reset/handoff: hard deps on M003 + closed
  context-continuity M004 → M003 now closed and the foundation is already
  closed → moves `blocked` → `ready for handoff`.
- M005 trajectory qualification: hard deps on M001–M004; M001+M002+M003 now
  closed → stays `blocked`, blocker narrows to M004 closure.
- No other registered plan lists M003 as a hard/interface dep; no new
  corrective plan is required (§10 has no qualifying defect).

## 12. Registry updates

In the same closure commit:

- `plans/implementation/long-horizon-work-execution/003-...md`: `ready for
  handoff` → `implemented` (landed with `2ff09bef`).
- `plans/implementation/long-horizon-work-execution/004-...md`: `blocked` →
  `ready for handoff` (M003 closed + context-continuity foundation already
  closed).
- `plans/registry.md`: long-horizon M003 row `ready` → `closed` (closure +
  implementation refs); move M004 out of Blocked work into the ready table
  as `ready`; narrow M005 blocker to M004 closure; advance execution-order
  gate 1 and the long-horizon control row to M003 closed / M004 ready;
  append M003 to recently-closed.
- `plans/subsystems/long-horizon-work-execution-roadmap.md`: M003 milestone
  + §12 table row → closed with closure/implementation refs; M004 row →
  ready; M005 blocker narrows to M004 closure.
