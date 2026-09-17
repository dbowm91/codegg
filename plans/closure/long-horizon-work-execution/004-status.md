# Long-Horizon Work Execution M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/long-horizon-work-execution/004-context-epoch-reset-and-handoff-integration.md`

Source subsystem roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Repository baseline reviewed: `18365458f881f6ac4524c9ea05224b69923faa4f`

Implementation commits or pull requests:

- `e3bfc565` — long-horizon M004: context-epoch reset and handoff integration

## 1. Executive finding

M004 is complete. Continuation checkpoints now carry bounded WorkPlan
identity/revision provenance without embedding the full plan; rollover
revalidates WorkPlan ID/revision alongside goal/plan/todo/parent before any
epoch handoff activates; a deterministic, profile-aware epoch policy
(default disabled/conservative) selects fresh provider-visible epochs only
at safe triggers (explicit operator, verified phase boundary,
repeated-compaction threshold, or bounded model-profile recovery); fresh
reconstruction rebuilds from canonical host state through the existing
compaction/rollover owners with exactly one continuation block, preserved
steering/handles, and valid tool contracts; restart loads only installed
rows with WorkPlan-aware validation; the bounded `context_epoch:started`
event explains each reset without payload contents; and normal compaction
remains the canonical fallback/default path. No second transcript store,
compaction engine, workflow scheduler, or history rewrite was introduced.
This unblocks M005.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WorkPlan ID/revision in checkpoint (§6 core) | `codegg-core::work_plan::checkpoint::{WorkPlanCheckpointProvenance, build_provenance, validate_provenance, provenance_from_body, revalidate_against_current}`; `src/context/continuation.rs::ContinuationSnapshot.work_plan` + `WorkPlanNextAction` precedence; `to_payload_body` additive `work_plan` key | pass | Bounded (≤5 actionable + ≤5 blocked summaries, ≤200 chars each, ≤4096-byte provenance); full plan stays in `work_plan` store reachable via tools/handles |
| Checkpoint schema/version (§6 core) | Envelope stays v1 (`CONTINUATION_CHECKPOINT_SCHEMA_VERSION = 1`); `work_plan` is an additive optional body key; `provenance_from_body` returns `None` for legacy bodies; `validate_installed_for_restart` fails closed only on present-but-malformed `work_plan` | pass | Prior checkpoints remain readable; old bodies render safely (`old_checkpoint_without_work_plan_renders_safely`) |
| Source-revision revalidation (§6 transactionality) | `src/context/rollover.rs::RolloverSourceRevisions` gains `work_plan_id/revision` with `capture_with_work_plan`, `is_stale_against`, `stale_reason` (`active work plan changed/revision changed`), `into_with_work_plan_overrides`; `AgentLoop::capture_rollover_revisions` + `load_work_plan_revision` + G-block revalidation; explicit `revalidate_against_current` defense-in-depth in `try_start_fresh_epoch` | pass | Stale revisions abort/rebuild, never install (`checkpoint_rejects_stale_work_plan_install`, `work_plan_todo_goal_races_abort_candidate`) |
| Epoch policy (§6 policy) | `codegg-core::work_plan::epoch_policy::{ContextEpochPolicy, ContextEpochInputs, ContextEpochTrigger, ContextEpochKeepReason, ContextEpochDecision, decide_epoch, epoch_supported_for_profile}`; default `enabled=false`; precedence disabled → unsupported → explicit → phase → threshold → profile-recovery → specific keep | pass | Deterministic (same state → same decision); no unconditional per-N-turn loop; `epoch_policy_decision_matrix` |
| Model-profile integration (§6 policy) | `epoch_supported_for_profile` opts in only `LongContextPlanner`/`FrontierReasoning`/`FrontierExecutor`; all other profiles (default/fast/local/tool-fragile) stay on normal compaction; unsupported form returns bounded `unsupported profile … use normal compaction` | pass | `unsupported_profiles_remain_on_normal_compaction`, `old_model_profile_config_behaves_unchanged` |
| Fresh reconstruction (§6 runtime) | `src/context/epoch.rs::{FreshEpochInputs, build_handoff_text, build_fresh_epoch_messages, validate_fresh_epoch_messages, ContextEpochLineage, build_epoch_started_event}` as consumer of canonical owners; order system → objective → Goal → WorkPlan → Todos → continuation → handles → steering → checkpoint lineage; marker-led single handoff block; no tool history | pass | One continuation frame, valid pairs, steering visible, WorkItem immediately continuable (`fresh_context_single_frame_and_tool_contracts`, `phase_boundary_to_fresh_epoch_continues_next_work_item`) |
| Safe triggers (§5/§6) | `try_start_fresh_epoch` gates on `decide_epoch`; triggers covered: phase boundary, repeated threshold, explicit operator, model-profile recovery (≥1 prior rollover); cancellation/steering aborts | pass | `repeated_compactions_trigger_only_for_supported_profile`, `steering_during_preparation_prevents_stale_activation` |
| Diagnostics/events/metrics (§6 protocol/WP-D) | `codegg-core::bus::events::AppEvent::ContextEpochStarted` (`context_epoch:started`) with reason/trigger/checkpoint/plan-revision/compaction-count/profile; `ContextEpochLineage::bounded_line`, `decision_diagnostic`, `RolloverDiagnostics`; TUI toast + inactive summary | pass | IDs/revisions/counts/reasons only; no payload/user/secret content |
| Restart/diagnostics (§6 WP-D) | `validate_installed_for_restart` validates `work_plan` shape; `render_installed_projection` renders legacy bodies unchanged and appends bounded `WorkPlan:` footer + next-action fallback; `latest_installed` remains sole resume authority; prepared rows never resume | pass | `restart_after_fresh_epoch_preserves_lineage`, `restart_before_candidate_activation_ignores_prepared` |
| Security/authorization (§6) | Epoch preserves execution-policy/model-selection authority (returns messages only, never mutates policy); no approval bypass/replay; no hidden reasoning; no cross-session handles; bounded previews | pass | `epoch_preserves_authority_and_hides_reasoning` |
| Docs/static guards (§6) | `architecture/context-compaction-ownership.md` (consumer/path section), `compaction.md` (fresh-epoch section + test commands), `work_plan.md` (M004 section), `goal.md` (epoch-handoff relation), `model_profile_task_state.md` (profile compatibility); `no_second_compaction_engine_or_history_store` static guard | pass | No new CI lane per verification policy |

## 3. Production implementation evidence

Core/domain (`codegg-core`, boundary-clean):

- `crates/codegg-core/src/work_plan/checkpoint.rs` (new): `MAX_CHECKPOINT_WORK_ITEMS = 5`,
  `MAX_CHECKPOINT_ITEM_TEXT_CHARS = 200`, `MAX_CHECKPOINT_PHASE_CHARS = 256`;
  `WorkPlanCheckpointItem`/`WorkPlanCheckpointProvenance`; `build_provenance()`
  (current-item-first, then actionable `(position,id)`, then blocked; counts
  describe full plan); `validate_provenance()` (wp_/wi_ prefixes, NUL/bounds,
  status vocabulary); `provenance_from_body()` (missing/null → legacy `None`,
  malformed → `Err`); `insert_provenance_into_body()`; `revalidate_against_current()`
  (legacy `None` never rejects; captured-without-current is stale). Five unit tests.
- `crates/codegg-core/src/work_plan/epoch_policy.rs` (new):
  `ContextEpochTrigger` (4 triggers), `ContextEpochKeepReason` (6 keep reasons),
  `ContextEpochDecision::{keep,start,reason_code}`, `ContextEpochPolicy`
  (default disabled; `enabled_for_handoff(threshold)` opt-in),
  `ContextEpochInputs`, `decide_epoch()` (precedence above),
  `epoch_supported_for_profile()` (long-horizon 3 only),
  `decision_diagnostic()`. Nine unit tests.
- `crates/codegg-core/src/work_plan/mod.rs`: registers/exports the two modules.
- `crates/codegg-core/src/bus/events.rs`: additive `AppEvent::ContextEpochStarted`
  (session/reason/trigger/checkpoint-id/seq/plan-id/rev/compaction-count/profile)
  with `event_type() = "context_epoch:started"`.

Application (`codegg` root):

- `src/context/continuation.rs`: `ContinuationSnapshot.work_plan` (optional),
  `ContinuationAssemblyInput.active_work_plan`, `CurrentTaskSource::WorkPlanNextAction`
  with precedence Goal → WorkPlan → Todo → previous; frame merges WorkPlan
  blocked markers advisory-only; footer renders `WorkPlan: id rev status`;
  payload body gains additive `work_plan` (absent for legacy).
- `src/context/rollover.rs`: `RolloverSourceRevisions` gains
  `work_plan_id/revision` + `capture_with_work_plan`/`into_with_work_plan_overrides`
  + stale reasons; `validate_installed_for_restart` validates `work_plan` shape;
  `work_plan_provenance_of()` helper; `render_installed_projection` renders
  legacy bodies unchanged plus bounded `WorkPlan:` footer and actionable
  next-action fallback on empty semantic steps.
- `src/context/epoch.rs` (new): consumer/path docs; bounds
  (`MAX_EPOCH_STEERING_MESSAGES = 5`, `MAX_EPOCH_STEERING_CHARS = 2000`,
  `MAX_EPOCH_RECOVERY_HANDLES = 16`, `MAX_EPOCH_HANDOFF_CHARS = 12 KiB`);
  `FreshEpochInputs`/`FreshEpochGoal` (host-owned only);
  `ContextEpochLineage::bounded_line()`; `build_handoff_text()` (marker-led,
  fixed order, no stale frames/reasoning); `build_fresh_epoch_messages()`
  (own `System` handoff starting with the versioned marker; unsupported
  profiles return bounded fallback error); `validate_fresh_epoch_messages()`
  (canonical pair + single-frame checks); `build_epoch_started_event()`.
  Seven unit tests. Re-exports the core policy for one import path.
- `src/context/mod.rs`: exports `epoch`.
- `src/agent/context_runtime.rs`: `load_work_plan_revision()`,
  `load_active_work_plan_for_snapshot()` (legacy `None` degradation);
  `capture_rollover_revisions` captures WorkPlan; baseline + stale-rebuild
  snapshots carry WorkPlan provenance; G-block uses
  `into_with_work_plan_overrides`; new `try_start_fresh_epoch()` (policy →
  authoritative load → reconstruction → canonical validation → `prepare_candidate`
  → cancellation/revalidation (coarse + explicit WorkPlan) → `finish_prepared_install`
  → lineage + `ContextEpochStarted` publish). Durable history never deleted;
  default production path unchanged (method is explicit-trigger; normal
  compaction remains default).
- `src/tui/runtime/app_events.rs`: `ContextEpochStarted` toast
  (`Fresh context epoch (trigger/reason) checkpoint …`) + inactive
  `StatusUpdate` kind + detail line. Other frontends ignoring the event
  remain functional.
- `src/context/compaction.rs`, `tests/context_continuity_m004.rs`: updated
  `ContinuationAssemblyInput` literals with `active_work_plan: None` (legacy
  coverage intact).

### Fresh-context before/after fixture (representative)

Before (pre-epoch provider history): `System("system prompt")`,
`User("migrate checkout flow …")`, filler assistant/user turns, possibly a
stale CodeGG frame from an older checkpoint.

After (`build_fresh_epoch_messages` with supported profile):

```text
System("system: canonical instructions")
System("[codegg continuation state v1]
- Objective: migrate checkout flow
- Goal: goal-1 rev 1 phase phase one next: run checkout tests
- WorkPlan: wp_… rev 3 status active phase phase one item wi_…
- next wi_…: port discount validation (next: do step 1)
…
- Todos: port discount validation | …
- Continuation state:
  [prior installed projection text]
- Recovery handles: ctx://tool/s/0/c0
- User steering: keep discounts backward compatible
- Checkpoint: ckpt-1 seq 1")
User("keep discounts backward compatible")   // only when not already tail-visible
```

Exactly one `count_continuation_frames`, valid pairs, no tool history, no
reasoning, steering visible, current WorkItem (`wi_*` + next action) present.

### Policy decision matrix (evidence)

| Policy | Inputs (rollovers, phase, explicit, replan, supported) | Decision |
|---|---|---|
| default (disabled) | (99, true, true, true, true) | Keep `disabled` |
| enabled t=3 | (99, *, *, *, false) | Keep `unsupported_profile` |
| enabled t=3 | (3, false, false, false, true) | Start `repeated_compaction` (deterministic) |
| enabled t=3 | (0, true, false, false, true) | Start `phase_boundary` |
| enabled t=3 | (0, true, true, false, true) | Start `explicit_operator` |
| enabled t=3 | (2, false, false, false, true) | Keep `insufficient_rollovers` |
| enabled t=3 | (0, false, false, true, true) | Keep (no prior rollover) |
| enabled t=3 | (1, false, false, true, true) | Start `model_profile_policy` |
| enabled t=None | (0, false, false, false, true) | Keep `no_trigger` |

### Proof the compaction path remains canonical

- `src/context/compaction.rs` untouched as owner (only test-literal update);
  `src/context/rollover.rs` extends the existing sequencing rather than
  forking it; `src/context/epoch.rs` contains no `compact_context`,
  `needs_context_compaction`, `context_tokens`, `CREATE TABLE`, transcript
  `DELETE/UPDATE`, and reuses `validate_message_invariants`,
  `count_continuation_frames`, `decide_epoch` (static guard
  `no_second_compaction_engine_or_history_store`).
- Default policy disabled: `old_checkpoint_without_work_plan_renders_safely`
  asserts legacy render + `disabled` keep; `verify.sh quick` + existing
  `context_continuity_m004` (15/15) green.
- No storage migration: `STORAGE_LAYOUT_VERSION` unchanged; catalog guard 7/7.

## 4. Verification executed

### Commands run (local; no CI lane added per verification policy)

```bash
cargo test -p codegg-core --lib -- work_plan
cargo test -p codegg --lib -- context::epoch context::continuation context::rollover
cargo test -p codegg --lib -- work_plan
cargo test -p codegg-core --lib -- continuation
cargo test -p codegg-core --test continuation_checkpoint
cargo test -p codegg-core --test work_plan_foundation
cargo test -p codegg-core --test work_plan_projection_arbiter
cargo test --test context_continuity_m004
cargo test --test context_epoch_handoff
cargo test --test work_plan_projection_arbiter
cargo test --test goal_continuation_progress
cargo test --test storage_migrations
cargo test --test agent_loop_harness -- compaction
cargo test --test agent_loop_harness -- work_plan
bash scripts/check-core-boundary.sh
python3 scripts/check_project_catalog_invariants.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Planned-command deviations (per plan §11, recorded here rather than hidden):

- `cargo test -p codegg-core -- continuation` as written matches no lib-target
  filter form in this layout; the current focused equivalents are
  `cargo test -p codegg-core --lib -- continuation` (15/15) plus
  `cargo test -p codegg-core --test continuation_checkpoint` (11/11).
- `cargo test --test agent_loop_harness -- compaction` / `-- work_plan` match
  zero tests in the current harness (0/0, pass with no filter hits); the
  current equivalents are `cargo test --test context_continuity_m004` (15/15),
  `cargo test --test context_epoch_handoff` (23/23), and
  `cargo test --test work_plan_projection_arbiter` (9/9).
- `python3 scripts/check_core_boundary.py` does not exist; the current
  equivalent is `bash scripts/check-core-boundary.sh` (run, pass).

### Results (local)

- `cargo test -p codegg-core --lib -- work_plan`: 58/58 pass (11 model +
  9 store + 7 assessment + 4 evidence + 4 projection + 5 todo-projection +
  5 checkpoint + 9 epoch-policy + 4 surrounding lib tests).
- `cargo test -p codegg --lib -- context::epoch context::continuation context::rollover`: 19/19 pass.
- `cargo test -p codegg --lib -- work_plan`: 4/4 pass.
- `cargo test -p codegg-core --lib -- continuation`: 15/15 pass.
- `cargo test -p codegg-core --test continuation_checkpoint`: 11/11 pass.
- `cargo test -p codegg-core --test work_plan_foundation`: 10/10 pass (M002 regression intact).
- `cargo test -p codegg-core --test work_plan_projection_arbiter`: 7/7 pass.
- `cargo test --test context_continuity_m004`: 15/15 pass (M004-continuity regression intact).
- `cargo test --test context_epoch_handoff`: 23/23 pass (20 epoch-handoff + 3 shared secret-scan guards).
- `cargo test --test work_plan_projection_arbiter`: 9/9 pass (M003 regression intact).
- `cargo test --test goal_continuation_progress`: 9/9 pass.
- `cargo test --test storage_migrations`: 4/4 pass.
- `cargo test --test agent_loop_harness -- compaction / -- work_plan`: 0/0 each (no matching harness filters; documented above).
- `bash scripts/check-core-boundary.sh`: pass.
- `python3 scripts/check_project_catalog_invariants.py`: 7/7 pass.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `./scripts/verify.sh quick`: pass (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution ownership, TUI project
  authority, workspace check).

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| No new compaction/history/checkpoint owner | `epoch.rs` header declares consumer/path; static guard `no_second_compaction_engine_or_history_store`; boundary guard passes |
| Fresh epoch does not delete/mutate durable history | Reconstruction returns in-memory `Vec<Message>` only; persistence is `prepare_candidate` + `finish_prepared_install` (same ordering as rollover); no `DELETE/UPDATE session` in epoch module (guarded) |
| Reset does not reset workspace/Git/jobs/runs/budgets/selection/permissions/sandbox/steering | Method takes/returns messages + lineage only; goal/budget/permission/sandbox untouched (`epoch_preserves_authority_and_hides_reasoning`); steering explicitly preserved (`recent_steering_preserved_after_old_checkpoint`) |
| WorkPlan/Goal/Todo revisions captured + revalidated | `capture_with_work_plan` + `into_with_work_plan_overrides` + explicit `revalidate_against_current` in `try_start_fresh_epoch`; stale aborts (`checkpoint_rejects_stale_work_plan_install`, race tests) |
| At most one continuation/handoff block visible | Marker-led handoff + `validate_fresh_epoch_messages` + `count_continuation_frames == 1` in unit + integration tests |
| Prompt provenance canonical; summaries cannot replace instructions | Inputs are host-owned only (system/goal/plan/todo/checkpoint/steering/handles); semantic enrichment never overwrites host facts (M002 tests green); no model paraphrase path |
| Tool pairing/chronology valid in either path | Fresh epochs carry no tool history (valid by construction) + canonical `validate_message_invariants`; integration asserts it |
| Epoch optional/profile-aware, never unconditional loop | Default disabled; supported 3 profiles only; threshold requires enablement + support; no per-N-turn auto-fire (`repeated_compactions_trigger_only_for_supported_profile`) |

## 6. Failure and recovery review

| Concern (§8) | Evidence |
|---|---|
| Preparation/reconstruction failure leaves history unchanged | `build_fresh_epoch_messages` errors before any store write; `prepare_candidate` errors defer with history intact (same pattern as rollover); unit `empty_instructions_and_objective_rejected`, `unsupported_profile_uses_normal_compaction` |
| Incompatible profile form uses normal compaction | `build_fresh_epoch_messages` returns bounded `unsupported profile …` and policy keeps `unsupported_profile`; integration `unsupported_profile_falls_back_to_normal_compaction` |
| Revision drift aborts/rebuilds | Coarse `is_stale_against` + explicit WorkPlan `revalidate_against_current` in `try_start_fresh_epoch`; `checkpoint_rejects_stale_work_plan_install`, `work_plan_todo_goal_races_abort_candidate` |
| Cancellation/steering during preparation prevents stale activation | `cancel_rx` checked before build, after prepare, before install (mirrors rollover points); todo-revision steering aborts (`steering_during_preparation_prevents_stale_activation`) |
| Restart loads only installed/authoritative state | `latest_installed` sole authority; prepared ignored (`restart_before_candidate_activation_ignores_prepared`); installed with WorkPlan validates + projects (`restart_after_fresh_epoch_preserves_lineage`) |
| Missing optional handles degrade | Recovery handles are exact-or-absent; missing degrades to summaries/diagnostics per continuity rules (M004-continuity tests green; epoch carries handles bounded, never bodies) |

## 7. Migration and compatibility review

No schema migration (continuation envelope stays v1; `STORAGE_LAYOUT_VERSION`
unchanged; `storage_migrations` 4/4; catalog invariants 7/7). Sessions without
a WorkPlan follow legacy behavior: snapshot `work_plan = None`, payload has no
`work_plan` key, `provenance_from_body` → `None`, rollover revisions carry
`(None, None)`, `render_installed_projection` renders exactly as before, and
`decide_epoch` with the default policy keeps `disabled`. Existing model
profiles with no epoch field inherit conservative behavior
(`old_model_profile_config_behaves_unchanged`). `AppEvent::ContextEpochStarted`
is additive; TUI explicitly handles it while older clients ignoring it remain
functional. Rollback is a plain revert of `e3bfc565` (new tables absent; new
events/tools additive; old checkpoints unaffected).

## 8. Security review

Plan mutations carry owning session identity (M002/M003 checks unchanged).
Evidence refs remain shape-validated and resolved against same-session
provenance; a referenced run/job grants no capability. Epoch reconstruction
inputs exclude hidden reasoning, credentials, and secret-bearing tool args;
payload validation rejects forbidden content classes (M001 store tests green).
Diagnostics/events carry bounded previews, IDs, digests, counts, and reason
codes only — verified by `diagnostic_helpers_never_embed_payload_body`,
`lineage_line_never_carries_body`, and the secret-scan harness bundled in
`context_epoch_handoff` (3/3). `codegg-core` boundary guard passes (no
UI/server/plugin/auth deps). WorkPlan/Goal/Todo stores are consumed by
reference; owner/run refs stay provenance-only.

## 9. Documentation and operations

- `architecture/context-compaction-ownership.md`: M004 consumer/path section
  (policy/reconstruction/validation/persistence/history/lineage/default).
- `architecture/compaction.md`: fresh-epoch consumer section + `context_epoch_handoff`
  test commands.
- `architecture/work_plan.md`: M004 checkpoint/policy/reconstruction section.
- `architecture/goal.md`: epoch-handoff relation (Goal authority preserved,
  newer-goal precedence, drift aborts).
- `architecture/model_profile_task_state.md`: fresh-epoch profile compatibility
  (3 supported, rest on normal compaction).
- Tool/event surface: `context_epoch:started` in `codegg-core::bus::events`
  with TUI toast/summary; catalog picks up no new model tool (epoch is a host
  policy path, not a model-invoked forget tool).
- Operator signals: `decision_diagnostic()`, `ContextEpochLineage::bounded_line()`,
  `RolloverDiagnostics::bounded_line()`, `stale work plan revision` / `active work
  plan changed` diagnostics, `unsupported profile` fallback reason.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `try_start_fresh_epoch` has no automatic production call site yet; it is an explicit host/operator path exercised by unit + integration tests | Production turns keep normal compaction by default (intended conservative posture); fresh epochs require explicit host wiring or a future policy hook | M005 trajectory work may wire a bounded explicit trigger if representative plans need it; no M004 corrective pass required |
| low | Artifact/Commit evidence refs still resolve as `Unavailable` without a dedicated host existence probe (M003 carryover) | Items relying solely on those refs need host `Satisfied` acceptance; no silent fabrication | M005 may add narrow host probes if representative plans need them; no M004 corrective pass required |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. Dependency audit for the
unblock check (registry Blocked-work section + roadmap §6 dependency graph):

- M004 context-epoch reset/handoff: hard deps on M003 + closed
  context-continuity M004 → both were closed before implementation → now
  closed.
- M005 trajectory qualification: hard deps on M001–M004; M001+M002+M003 were
  closed and M004 now closes → all hard deps satisfied → moves `blocked` →
  `ready for handoff`.
- No other registered plan lists M004 as a hard/interface dep; no new
  corrective plan is required (§10 has no qualifying defect).

## 12. Registry updates

In the same closure commit:

- `plans/implementation/long-horizon-work-execution/004-...md`: `ready for
  handoff` → `implemented` (landed with `e3bfc565`).
- `plans/implementation/long-horizon-work-execution/005-...md`: `blocked` →
  `ready for handoff` (M001–M004 now closed).
- `plans/registry.md`: long-horizon M004 row `ready` → `closed` (closure +
  implementation refs); move M005 out of Blocked work into the ready table
  as `ready`; advance execution-order gate 1 and the long-horizon control row
  to M004 closed / M005 ready; append M004 to recently-closed.
- `plans/subsystems/long-horizon-work-execution-roadmap.md`: M004 milestone
  + §12 table row → closed with closure/implementation refs; M005 row →
  ready with blocker cleared.
