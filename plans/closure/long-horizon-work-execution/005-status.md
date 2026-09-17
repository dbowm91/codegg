# Long-Horizon Work Execution M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/long-horizon-work-execution/005-long-horizon-trajectory-and-recovery-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Repository baseline reviewed: `18365458f881f6ac4524c9ea05224b69923faa4f`

Implementation commits or pull requests:

- `35d8a955` — long-horizon M005: trajectory and recovery
  qualification (deterministic harness only; no production delta)

## 1. Executive finding

M005 is complete. The M001–M004 architecture passed integrated
qualification with no production defect found, so per plan §6 no production
change was made. One new deterministic harness,
`tests/long_horizon_trajectory_qualification.rs` (22 tests, 27/27 with the
5 shared `common::secret_scan` helper tests), drives a representative
9-item/3-phase large plan across eight context transitions — seven
compactions plus one policy-gated fresh-epoch reset at a verified phase
boundary — with user steering injected after an early phase, a delegated
child with canonical subagent evidence, live-then-completing test jobs, and
a premature-final probe. Focused scenarios additionally prove Goal-bound
trajectories, verified waits on the same handle, no-progress
replan-then-AwaitingUser below the emergency cap, passing/failed/forged
host evidence, file-backed daemon/storage reopen at safe and awkward
boundaries, steering/stale-revision contention, cancellation semantics,
child-vs-parent races, security negatives, legacy migration paths, bounded
resource assertions, and the no-second-owner static guard. Completed work
kept exactly one attempt per item across all resets, the canonical job
count never grew due to a reset, and complete plans terminate without
extra continuation. This closes M005 and therefore the long-horizon work
execution workstream (M001–M005 all closed).

## 2. Requirement-to-evidence matrix

| Requirement (plan §10) | Evidence | Result | Notes |
|---|---|---|---|
| Ordinary large plan full trajectory | `ordinary_large_plan_full_trajectory` | pass | 4 items, 3 phases, delegated child, passing test + subagent evidence, steering, premature-final probe, turn-end close exactly once |
| Goal-bound large plan full trajectory | `goal_bound_large_plan_full_trajectory` | pass | Bound plan gates Goal completion; Goal tool rejects early final with `work_plan_id`; verifier `Met` only after host completion; Goal reaches `Complete` |
| Premature final → continuation | `premature_final_requires_continuation_then_terminates` | pass | `ContinueWithPrompt` names the current item and stays within the arbiter bound; `AllowCompletion` only after host completion |
| Verified wait → same handle → completion | `verified_wait_same_handle_then_completion` | pass | `WaitForHandle` names the identical job id; job count stays 1 across completion; no relaunch |
| No-progress → replan → AwaitingUser | `no_progress_replans_then_awaits_user` | pass | nudge → replan → awaiting-user over 3 identical boundaries; `MAX_…_BEFORE_AWAITING_USER == 3`; `should_continue` false after; stale escalation rejected |
| Passing/failed host test evidence | `passing_and_failed_host_test_evidence` | pass | Passed → satisfiable; Failed → stays actionable; dangling ref → `Unavailable`, forged completion never `Complete` |
| Restart after plan mutation | `restart_after_plan_mutation` | pass | File-backed reopen preserves phase/revisions; pre-restart stale plan/item writes → `Conflict` |
| Restart during live job | `restart_during_live_job` | pass | Item still `InProgress` with identical attempts; WorkPlan wait and Goal `VerifiedWait` reassemble on the same handle; job count 1 |
| Restart after job completion before model sees result | `restart_after_job_completion_before_model_sees_result` | pass | No replay/success synthesis on reload; host completes from canonical evidence; `AllowCompletion`; no duplicate job |
| Restart around prepared/installed boundary | `restart_around_prepared_installed_boundary` | pass | Prepared never resume authority across reopen; installed usable with surviving provenance; newer plan revision supersedes stale provenance |
| Steering vs stale checkpoint/plan update | `steering_vs_stale_plan_update` | pass | Pre-steering provenance fails revalidation; source revisions report `active work plan revision changed`; stale item write → `Conflict`, state untouched |
| Cancel vs completion assessment | `cancel_vs_completion_assessment` | pass | Unfinished items stay unfinished (never completed); terminal transition fails closed; turn-end close fails closed, state preserved cancelled |
| Child update vs parent transition | `child_update_vs_parent_transition` | pass | Matching `caller_run_id` reports; sibling run rejected (`different run`); stale replay → `Conflict`; bounded read intact |
| Model text cannot forge completion | `model_text_cannot_forge_completion` | pass | Host-owned fields rejected; Todo-completed alone translates 0 items; prose-only job id yields no executions and no wait handle |
| No hidden content in diagnostics | `diagnostics_carry_no_hidden_content` | pass | Fingerprint excludes secrets/reasoning prose; checkpoint diagnostics exclude bodies; no reasoning/credential keys in payload; provenance ≤4096 B; arbiter message bounded |
| Epoch preserves execution policy | `context_epoch_preserves_execution_policy` | pass | Supported/unsupported profile split; single frame, valid pairs, no tool history, no reasoning; steering/handles/current item visible; default policy `disabled`; lineage IDs-only; `context_epoch:started` emitted |
| Pre-WorkPlan DB/session path | `legacy_session_without_workplan_uses_legacy_behavior` | pass | No plan → legacy `None` + gate allow; v58 database gains empty tables, goals intact, layout version tracks |
| Prior checkpoint version | `prior_checkpoint_version_remains_readable` | pass | Pre-M004 body installs, validates `Usable`, provenance `None`, projection renders legacy objective |
| Representative 8-transition trajectory | `eight_transition_trajectory_with_epoch_reset` | pass | Per-transition table §3; phase-boundary fresh epoch validated; attempts stay 1; job count stays 2; `AllowCompletion` + single turn-end close |
| Bounded performance/resources | `projections_and_diagnostics_stay_bounded` | pass | 12-item plan: default ≤5, wide ≤8 items and ≤4096 B; provenance ≤4096 B; handoff ≤12 KiB; assessment pure, no model call |
| Missing recovery artifact | `missing_recovery_artifact_degrades_boundedly` | pass | Corrupted handle degrades to summary-only; verified+degraded == total; checkpoint still installs |
| No second compaction/workflow owner | `no_second_compaction_or_workflow_owner` | pass | Static guard over `epoch.rs` + WorkPlan assessment/store sources |

## 3. Production implementation evidence

No production files changed. Per plan §6, M005 is a
verification/corrective milestone: defects found would have been fixed in
their canonical owners (M001–M004, compaction/rollover, scheduler/jobs).
Qualification found no production defect, so the tree delta is exactly:

- `tests/long_horizon_trajectory_qualification.rs` (new, 22 tests):
  shared deterministic fixtures (session seeding, file-backed pools,
  Test/Subagent job insertion with live or terminal attempts, host-side
  completion through `Satisfied` acceptance or `Passed` evidence only)
  plus the 22 scenarios in the matrix above.
- `architecture/work_plan.md`: short M005 qualification section (docs only).
- Planning control points: this closure record, roadmap milestone status,
  registry status, implementation-plan status (see §12).

### Representative trajectory per-transition table

Objective `migrate checkout flow without breaking discounts`, 9 items
(`port discount validation`, `update checkout tests` [TestJob], `delegated
checkout audit` [DelegatedRun + `run-child-1`], `fix discount test with
strict parser`, `migrate to snapshot tests`, `remove new network calls`,
`ship migration`, `verify rollout`, `close out docs`), dependency-chained
across three phases:

| Epoch | Host evolution | Phase | Remaining | Steering visible | Evidence identity |
|---|---|---|---|---|---|
| 0 | plan created; premature-final probe continues | phase one | 9 | no | 2 live jobs, no relaunch |
| 1 | host-completes discount validation (`Satisfied`) | phase one | 8 | no | unchanged handles |
| 2 | user steering (message + phase/next-action) + test job completes + checkout tests completed via `Passed` evidence | phase one (steered) | 7 | yes | TestJob → `Passed` |
| 3 | subagent job completes + audit completed via `Passed` delegated evidence | phase one (steered) | 6 | yes | DelegatedRun → `Passed` |
| 4 | phase-two pointer + strict-parser fix completed; **fresh epoch** on `phase_boundary` trigger (supported profile) | phase two | 5 | yes | both `Passed`, stable ids |
| 5 | snapshot-test migration completed | phase two | 4 | yes | stable |
| 6 | network-call removal completed | phase two | 3 | yes | stable |
| 7 | ship + verify + docs completed | phase two | 0 | yes | stable |

Each epoch asserts: objective preserved in the snapshot; checkpoint
prepare → read-back digest → single-frame/valid-pair verification →
install; installed provenance equals the current plan id/revision and
revalidates clean; remaining counts follow 9,8,7,6,5,4,3,0; the next
actionable item carries a non-empty next action; steering visibility flips
exactly at epoch 2; evidence lookups resolve to the same canonical job
ids. Closure: `AllowCompletion`, one turn-end close, every item
`Completed` with `attempts == 1`, job count exactly 2.

### Proof no duplicate side effect hides in recovery

- Trajectory: job count 2 before, during, and after all eight transitions
  and the epoch reset; completed items keep `attempts == 1`.
- Verified wait: job count 1 before and after the live → terminal
  transition; the wait names the identical handle.
- No-progress: stagnation escalates at 3 boundaries, far below the 32-turn
  emergency guard; no continuation is consumed by spinning.
- Restart: in-progress items reload with identical status/attempts and no
  automatic transition; prepared candidates are never authority.

## 4. Verification executed

### Commands run (local; no CI lane added per verification policy)

```bash
cargo test --test long_horizon_trajectory_qualification -- --test-threads=1
cargo test -p codegg-core --lib -- work_plan
cargo test -p codegg-core --lib -- goal
cargo test -p codegg-core --test work_plan_foundation --test work_plan_projection_arbiter --test continuation_checkpoint
cargo test --test agent_loop_harness
cargo test --test goal_continuation_progress
cargo test --test work_plan_projection_arbiter
cargo test --test context_epoch_handoff
cargo test --test context_continuity_m004
cargo test --test storage_migrations
bash scripts/check-core-boundary.sh
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Planned-command deviations (per plan §11, recorded here rather than hidden):

- `cargo test -p codegg-core -- work_plan` / `-- goal` as written match no
  lib-target filter form in this layout; the focused equivalents are
  `cargo test -p codegg-core --lib -- work_plan` (58/58) and
  `cargo test -p codegg-core --lib -- goal` (60/60).
- `cargo test --test goal_verification` names no existing test target; the
  current equivalents are `cargo test --test goal_continuation_progress`
  (9/9) and the Goal-bound trajectory in the new harness.
- `python3 scripts/check_core_boundary.py` does not exist; the current
  equivalent is `bash scripts/check-core-boundary.sh` (run, pass).

### Results (local)

- `long_horizon_trajectory_qualification`: 27/27 pass (22 qualification
  scenarios + 5 shared `common::secret_scan` helper tests).
- `codegg-core --lib -- work_plan`: 58/58 pass.
- `codegg-core --lib -- goal`: 60/60 pass.
- `work_plan_foundation`: 10/10; `work_plan_projection_arbiter` (core):
  7/7; `continuation_checkpoint`: 11/11 pass.
- `agent_loop_harness`: 52/52 pass.
- `goal_continuation_progress`: 9/9 pass.
- `work_plan_projection_arbiter` (root): 9/9 pass.
- `context_epoch_handoff`: 23/23 pass.
- `context_continuity_m004`: 15/15 pass.
- `storage_migrations`: 4/4 pass.
- `bash scripts/check-core-boundary.sh`: pass.
- `python3 scripts/check_sandbox_contract.py`: pass.
- `python3 scripts/check_execution_ownership.py`: pass.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `./scripts/verify.sh quick`: pass (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution ownership, TUI project
  authority, workspace check).

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Deterministic providers/fixtures only; no API key | All scenarios use in-memory/file SQLite, `compact_context` with `provider: None`, and host-assembled evidence; no network calls |
| Bounds never weakened to pass | New tests assert existing bounds (projection ≤8/4096 B, provenance ≤4096 B, arbiter ≤1000 chars, handoff ≤12 KiB); no bound constant changed |
| No duplicate side effect accepted | Job-count and per-item `attempts` assertions across trajectory, waits, and restarts (§3) |
| Completion from WorkPlan/host evidence, not prose | Premature-final continuation, forged-evidence rejection, Todo-alone rejection, prose-job rejection |
| Prepared/uninstalled checkpoints never resume authority | Prepared rows ignored across file-backed reopen; only installed rows validate `Usable` |
| Epoch/compaction changes no authority | Fresh-epoch builder returns messages only; unsupported profiles fall back; default policy disabled; Goal/budget/permission state untouched |
| Honest failure/unrun evidence | §4 deviations recorded; no live-provider matrix per verification policy; residual limits in §10 |

## 6. Failure and recovery review

| Concern (§8) | Evidence |
|---|---|
| No relaunch solely because context reset | Trajectory + wait job-count/attempts assertions |
| Cancellation leaves work unfinished/cancelled, not completed | `cancel_vs_completion_assessment`: leftover item never `Completed`; terminal transitions and turn-end close fail closed |
| Restart uses current WorkPlan/Goal/job state; ignores prepared candidates | File-backed reopen tests; Goal `VerifiedWait` reassembles on the same handle |
| Stale revisions cannot overwrite newer steering/progress | `steering_vs_stale_plan_update`, restart stale-write rejection, child stale-replay rejection, rollover source-revision drift |
| Failed/inconclusive evidence never becomes success | Failed stays actionable; dangling stays actionable; `Unavailable` never satisfies |
| Missing recovery artifact degrades boundedly | `missing_recovery_artifact_degrades_boundedly`: degrade to summary-only, checkpoint still installs |

## 7. Migration and compatibility review

No schema migration (no production delta; `STORAGE_LAYOUT_VERSION`
unchanged). Legacy sessions without a plan keep legacy completion behavior;
a simulated v58 database gains empty WorkPlan tables with goals intact;
pre-M004 checkpoint bodies install, validate, and render with `None`
provenance while the default epoch policy stays disabled. Existing model
profiles without epoch support remain on normal compaction. Rollback is a
plain revert of this closure commit (test + docs + planning files only).

## 8. Security review

Model tools reject host-owned acceptance/evidence/status writes loudly;
Todo completion alone never completes host-gated items; prose-named jobs
never become wait handles; completed-without-signal items stay actionable.
Fingerprints, checkpoint/payload diagnostics, epoch lineage, and arbiter
messages carry IDs/digests/counts/reasons only — asserted free of secrets,
hidden-reasoning markers, and credential keys. `codegg-core` boundary,
sandbox-contract, and execution-ownership guards pass. No new tool,
protocol field, or store was added.

## 9. Documentation and operations

- `architecture/work_plan.md`: M005 qualification section (harness map,
  representative trajectory summary, test commands).
- Goal/context/agent docs: unchanged — no production delta to document;
  M001–M004 sections remain authoritative.
- Operator signals: unchanged (`decision_diagnostic()`,
  `ContextEpochLineage::bounded_line()`, `RolloverDiagnostics`,
  arbiter/assessment reason codes).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Qualification is deterministic/scripted only; no live-provider or multi-host matrix | Integrated behavior under real model prose diversity is not measured here | None required by the verification policy; future product work may add opt-in live harnesses under its own plan |
| low | Artifact/Commit evidence refs still resolve as `Unavailable` without a dedicated host existence probe (M003/M004 carryover) | Items relying solely on those refs need host `Satisfied` acceptance; no silent fabrication | None required for closure; a future plan may add narrow host probes if representative work needs them |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and the workstream closes with it (M005 is terminal:
M001+M002+M003+M004+M005 all closed). Dependency audit for the unblock
check (registry Blocked-work section + roadmap §6 dependency graph):

- M005 trajectory qualification: hard deps on M001–M004, all closed
  before qualification → now closed.
- No other registered plan lists M005 (or the long-horizon workstream) as
  a hard/interface dependency → **nothing newly unblocked, nothing newly
  blocked**; no corrective plan required (§10 has no qualifying defect).

## 12. Registry updates

In the same closure commit:

- `plans/implementation/long-horizon-work-execution/005-...md`: `ready for
  handoff` → `implemented` (landed with `35d8a955`; harness-only,
  no production delta).
- `plans/registry.md`: long-horizon M005 row `ready` → `closed` (closure +
  implementation refs); advance execution-order gate 1 and the
  long-horizon control row to M005 closed / workstream closed; append M005
  to recently-closed; subsystem row `active` → `closed`.
- `plans/subsystems/long-horizon-work-execution-roadmap.md`: `Status:
  active` → `closed`; M005 milestone + §12 table row → closed with
  closure refs.
