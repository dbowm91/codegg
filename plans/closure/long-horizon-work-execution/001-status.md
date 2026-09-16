# Long-Horizon Work Execution M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/long-horizon-work-execution/001-goal-progress-and-continuation-correctness.md`

Source subsystem roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Repository baseline reviewed: `18365458f881f6ac4524c9ea05224b69923faa4f`

Implementation commits or pull requests:

- `5d79bd9d` — long-horizon M001: goal progress and continuation correctness

## 1. Executive finding

M001 is complete. An Active Goal no longer auto-continues on budget alone:
every continuation cycle reloads the current Goal revision, checks
budget/terminal status first, then classifies host-observed state as
`Progress`, `VerifiedWait`, or `NoProgress`. Genuine todo/execution progress
resets stagnation; a live goal-owned test/delegated run yields a verified
wait that polls the existing handle without relaunching it; repeated
no-progress state issues a nudge then an explicit replan instruction and, on
the third consecutive no-progress cycle, transitions the same Goal revision
to the existing `AwaitingUser` state with a concise bounded blocker report —
well before the 32-cycle emergency cap, which remains as an invariant only.
The incorrect nonexistent `Blocked` goal-status contract is removed from the
continuation prompt (which now explicitly denies it and directs blockers to
`AwaitingUser`), and a unit-test guard prevents its reintroduction. No
WorkPlan storage, schema migration, scheduler polling framework, compaction
change, or new CI lane was introduced. This unblocks M002.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Correct continuation prompt/status language (§5) | `crates/codegg-core/src/goal/runtime.rs::build_continuation_prompt` rewritten blocker bullet; `continuation_prompt_has_no_blocked_status_contract` test | pass | Old "count consecutive blocked turns before allowing a `Blocked` status" removed; prompt now denies `Blocked` and directs `open_questions` → `AwaitingUser` |
| Typed long-horizon continuation progress classification (§5/§6) | `crates/codegg-core/src/goal/progress.rs::GoalProgressDisposition::{Progress, VerifiedWait, NoProgress}`, `GoalNoProgressReason::{NoStateChange, BlockerReported, EvidenceLoadFailed}`, `WaitHandleRef::{TestJob, DelegatedRun}` | pass | Host-observable bounded metadata only; reuses `ProgressSignal` (`NewEvidence`/`StateChanged`/`ChildAdvanced`) and graduated `Nudge→Replan→Stall` vocabulary without a second general recovery controller |
| Host-owned fingerprint from existing state (§6) | `goal_continuation_fingerprint()` over goal status + per-status todo counts + open-question count + sorted `(id, source, status)` executions; `src/goal_continuation.rs::assemble_continuation_evidence` from `Goal` + in-memory `TodoState` + goal-labelled durable jobs | pass | Free-form reasoning/summary/output never hashed; prose-only revision bumps leave the fingerprint unchanged (tested) |
| `Progress` resets stagnation recovery (§6) | `maybe_continue_goal` sets `consecutive_no_progress = 0` on `Progress` and `VerifiedWait`; `progressing_goal_continues_and_completes_through_verifier` | pass | Todo completion and execution transitions classify as progress |
| `VerifiedWait` polls without relaunch (§6) | `build_verified_wait_prompt` ("Do not start a duplicate"); loop queues wait prompt and drains, never creates jobs; `live_job_produces_verified_wait_without_duplication` (job count stays 1) | pass | Handle accepted only from store records carrying the goal provenance label |
| Bounded no-progress correction/replan (§6) | `stagnation_step()` (1→nudge, 2→replan, ≥3→escalate); `build_replan_prompt`; `stalled_goal_replans_then_awaits_user_before_emergency_cap` (3 < 32) | pass | Replan instruction precedes terminal escalation |
| `AwaitingUser` terminal handoff (§6) | `update_status_if_revision(..., AwaitingUser)` + `GoalUpdated` publish + `build_awaiting_user_blocker_report`; stale CAS returns `None` → abort | pass | Same-revision CAS; concurrent progress/pause/cancel/replacement wins |
| Pause/cancel/budget preserved (§4) | `should_continue(&goal)` gate first each cycle; non-active never continues (`should_continue_never_revives_non_active_status`, all budget axes); steering/`cancel_rx` checked per cycle | pass | Progress policy cannot revive non-Active goals |
| Verifier remains completion gate (§4) | No change to `GoalVerificationService`; `progressing_goal_continues_and_completes_through_verifier` completes via `Met` + `complete_if_active` | pass | Model prose still not authority |
| Bounded reason/status projection (§6) | `reason_code()` (`progress`, `verified_wait`, `no_progress`, `blocker_reported`, `evidence_load_failed`) + `stagnation_step_str()` (`nudge`, `replan`, `awaiting_user_no_progress`) in tracing; terminal state via existing `GoalUpdated`; no new DTO | pass | Existing clients ignore nothing new; no command output dumped |
| No new `Blocked` status (§4) | `GoalStatus` unchanged (7 variants); grep shows only denials + `TodoStatus::Blocked` (separate) | pass | `update_status_if_revision` to `AwaitingUser` only |
| No hidden reasoning persisted (§4) | Fingerprint/report/evidence carry counts/statuses/ids only; `fingerprint_and_diagnostics_exclude_hidden_reasoning` | pass | Open questions counted, never quoted into fingerprint |
| Restart-safe without migration (§6) | No schema change; counter is run-local and restarts conservatively; `resume_with_live_job_still_verifies_wait` | pass | Never falsely marks complete |
| Docs + guard (§6) | `architecture/goal.md` updated (progress section, lifecycle, invariants, no-`Blocked` statement); prompt guard test | pass | `architecture/agent.md` needs no change (no goal-continuation section owns the contract; `goal.md` is authoritative) |

## 3. Production implementation evidence

Core/domain (`codegg-core`, boundary-clean):

- `crates/codegg-core/src/goal/progress.rs` (new): `GoalNoProgressReason`,
  `WaitHandleRef`, `GoalProgressDisposition` + `reason_code()`,
  `GoalContinuationEvidence` (revision carried for CAS, excluded from hash),
  `goal_continuation_fingerprint()`, `assess_goal_continuation()`,
  `disposition_for_evidence_failure()`, `live_wait_handle()`,
  `GoalStagnationStep` + `stagnation_step()` + `stagnation_step_str()`,
  `MAX_CONSECUTIVE_NOPROGRESS_BEFORE_AWAITING_USER = 3`. Ten unit tests.
- `crates/codegg-core/src/goal/runtime.rs`: continuation prompt blocker
  bullet corrected (denies `Blocked`, directs `open_questions` →
  `AwaitingUser`, notes narration ≠ progress); new
  `build_verified_wait_prompt()`, `build_replan_prompt()`,
  `build_awaiting_user_blocker_report()` (all bounded, no output dumps);
  `should_continue()` unchanged as the budget/status gate; five new tests
  (no-`Blocked` guard, wait/replan/report rendering, all budget axes,
  non-active never revives).
- `crates/codegg-core/src/goal/mod.rs`: registers `progress`.

Application/runtime (root):

- `src/goal_continuation.rs` (new): read-only
  `assemble_continuation_evidence()` from `Goal` + cloned `TodoState`
  (per-status counts, no content text) + `SqliteJobStore` `Test`/`Subagent`
  records filtered by session, `created_at >= goal.created_at`, and the
  host-written `goal_id` label; maps to `Passed`/`Failed`/`InProgress`.
  Two unit tests (counts without content, fake prose never a handle).
- `src/agent/turn_completion.rs::maybe_continue_goal()`: per-cycle
  steering/cancel check → reload goal → replacement abort (initial id) →
  `should_continue` budget/terminal gate (wrap-up preserved) → assemble
  evidence (todo lock never held across store query) → assess →
  `Progress` (reset, continue prompt) / `VerifiedWait` (reset, wait prompt,
  no relaunch) / `NoProgress` (increment, nudge prompt → replan prompt →
  same-revision `AwaitingUser` CAS + `GoalUpdated` + bounded report, else
  abort on stale). Emergency `MAX_CONTINUATIONS = 32` retained.
- `src/lib.rs`: registers `goal_continuation`.
- `tests/goal_continuation_progress.rs` (new): nine deterministic
  store-backed integration tests (see §4).

Storage/protocol: no migration, no DTO change. `AwaitingUser` reuses the
existing status string and `GoalUpdated` event. Workspace file-mutation
generation is intentionally not fingerprinted in M001 (residual, see §10).

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib -- goal
cargo test -p codegg --lib -- goal_continuation
cargo test --test goal_continuation_progress -- --test-threads=1
cargo test --test agent_loop_harness -- goal -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
./scripts/check-core-boundary.sh
```

Planned-command deviations (per plan §11, recorded here rather than hidden):

- `python3 scripts/check_core_boundary.py` does not exist; the repository's
  current focused equivalent is `./scripts/check-core-boundary.sh` (run,
  pass).
- `cargo test --test goal_verification` names no existing test target; the
  current focused equivalents are `cargo test -p codegg --lib --
  goal_continuation` (assembler unit tests) and `cargo test --test
  goal_continuation_progress` (store-backed continuation qualification).
- `cargo test -p codegg-core -- goal` was run as `cargo test -p
  codegg-core --lib -- goal` (same coverage, narrower target).
- `cargo test --test agent_loop_harness -- goal` matches zero tests
  (harness has no goal-named cases); it passes vacuously and the real loop
  driver is covered by the deterministic assessment + CAS tests above. No
  new CI lane was added.

### Results

- `cargo test -p codegg-core --lib -- goal`: 58/58 pass (runtime incl. all
  budget axes + non-active matrix + no-`Blocked` guard; progress incl.
  fingerprint stability/change, prose-only non-progress, wait, terminal
  transition, blocker reason, unknown-source rejection, stagnation
  thresholds; store + verification suites unchanged).
- `cargo test -p codegg --lib -- goal_continuation`: 2/2 pass (counts
  without content text; fake job prose never a handle).
- `cargo test --test goal_continuation_progress`: 9/9 pass
  (progress-then-`Met`-completion; stall nudge→replan→`AwaitingUser` with
  3 < 32; live job `VerifiedWait` with job count 1; failed-job transition
  is progress and verifier `NotMet`; restart resume still waits; stale
  revision CAS aborts then current revision succeeds; pause/cancel/replace
  stop continuation; fake prose never waits; fingerprint/diagnostics exclude
  hidden reasoning).
- `cargo test --test agent_loop_harness -- goal`: 0 passed, 52 filtered,
  ok (no goal-named harness cases; see deviation note).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `./scripts/verify.sh quick`: pass (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution ownership, TUI project
  authority, workspace check).
- `./scripts/check-core-boundary.sh`: pass (`progress.rs` uses only
  `serde`/`sha2`/sibling goal types).

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Budget/status authoritative; progress never revives Paused/Cancelled/BudgetLimited/Complete | `should_continue` gate first each cycle; `should_continue_never_revives_non_active_status`; `stalled_...` asserts `AwaitingUser` does not continue |
| Model cannot mark complete by claiming progress | Verifier untouched; `failed_job_...` shows failure → `NotMet`; completion still needs `Met` + `complete_if_active` |
| Existing verification remains the completion gate | Same `GoalVerificationService` path; `progressing_...` passes through it |
| Continuation bounded by turn/tool/token/wall-clock + 32-cap | All budget axes tested; 32-cap retained and logged; stagnation exits at 3 |
| Verified wait only on canonical live handle/state | `WaitHandleRef::bounded` from labelled store records only; unknown sources rejected; fake-prose tests |
| No new `Blocked` status | `GoalStatus` unchanged; prompt denies it; guard test; grep shows denials only |
| No hidden reasoning persisted | Fingerprint/report/evidence exclude prose/output; dedicated test with `hunter2` sentinel |
| Steering/cancellation interrupts normally | Per-cycle `steering`/`cancel_rx` check; returns before any `AwaitingUser` transition |

## 6. Failure and recovery review

| Concern (§8) | Evidence |
|---|---|
| Evidence load failure ≠ progress | `disposition_for_evidence_failure` → `NoProgress(EvidenceLoadFailed)`; routed through replan/`AwaitingUser` like other stagnation; covered in diagnostics test |
| Live handle disappears between assess and wait | Each cycle reloads goal + jobs; a terminal outcome reclassifies as `Progress` next boundary (`failed_job_...`); nothing is relaunched host-side |
| Cancellation stops immediately | Checked before load/assess and before escalation; never converted to blocker progression |
| Replacement/revision mismatch aborts stale decision | Initial-id comparison aborts on replacement (`pause_cancel_replace_stop_continuation` covers replace→paused predecessor); `AwaitingUser` uses `update_status_if_revision`, stale returns `None` → abort (`stale_revision_...`) |
| Concurrent progress wins via revision/CAS | Same test: stale revision fails, current succeeds; progress bump via `update_progress` wins |
| Restart relies on durable state; counter restarts conservatively | No durable counter added (deliberate); `resume_with_live_job_still_verifies_wait` reassembles from durable goal/job stores; never marks complete |
| Duplicate delivery/idempotency | Wait path creates no jobs (`live_job_...` asserts count 1); completion still single-CAS |
| Malformed/unauthorized input | Empty/overlong job ids rejected in `WaitHandleRef::bounded`; unknown sources rejected; overlong fields truncated to fixed bounds |

## 7. Migration and compatibility review

No schema migration. Goal rows, status strings, `GoalUpdated`/`GoalCompleted`
shapes, `GoalCompletionRequest`/verifier authority, `/goal resume` and
budget semantics are unchanged. `AwaitingUser` already existed and is now the
canonical unresolved-blocker state. Frontends that ignore the new tracing
reason codes (`progress`, `verified_wait`, `replan`,
`awaiting_user_no_progress`, `budget_limited`) remain functional; the only
user-visible transition uses the existing `GoalUpdated` event. Rollback is a
plain revert of `5d79bd9d` with no data backfill.

## 8. Security review

Wait/progress inspection is read-only. A model cannot manufacture a
`WaitHandleRef` through prose: the assembler accepts only durable job records
whose host-written `goal_id` label equals the goal under continuation
(`fake_job_prose_never_produces_verified_wait`). Hidden reasoning, tool
arguments/output, and file contents never enter the fingerprint, prompts, or
stored diagnostics (`fingerprint_and_diagnostics_exclude_hidden_reasoning`;
all prompts bounded to 128–240 chars of structured metadata). No authority,
approval, sandbox, credential, or provider-selection change. No secrets
logged. `codegg-core` boundary guard passes.

## 9. Documentation and operations

- `architecture/goal.md`: module map (`progress.rs`, `goal_continuation.rs`,
  loop driver), corrected lifecycle (budget gate + progress/wait/no-progress,
  3-cycle `AwaitingUser` escalation, 32-cap as emergency only), new
  continuation-progress type section with reason codes, invariants
  (no-`Blocked`, wait-no-relaunch, load-failure, cancel/replace/CAS,
  run-local counter), and the explicit `GoalStatus` closed set.
- `architecture/agent.md`: no edit — it holds no goal-continuation contract
  section; `goal.md` is the authoritative owner and is linked from it.
- Operator signals: tracing reason codes per cycle plus the existing
  `GoalUpdated` event on `AwaitingUser` escalation with a bounded report
  (`goal … entered awaiting_user after N …; fingerprint …; open_questions
  …`).
- Static guard: `continuation_prompt_has_no_blocked_status_contract` fails
  closed if the old "allowing a `Blocked`" offer or "consecutive blocked
  turns before" wording returns, while requiring the explicit denial to stay.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Workspace file-mutation generation is not part of the M001 fingerprint (todos + goal-owned jobs only) | A turn that mutates files without touching todos/jobs and without producing a goal-labelled test run reads as no-progress at the continuation boundary | M002/M003 may add a bounded workspace-generation input when `WorkPlan` evidence wiring lands; no M001 corrective pass required |
| low | `VerifiedWait` covers goal-labelled `Test`/`Subagent` handles only, not managed processes or generic scheduler jobs | Waits on other canonical handles fall back to replan/`AwaitingUser` rather than a typed wait | Revisit if a later milestone registers those handles as goal-provenance owners; out of M001 scope by plan §5 |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. Dependency audit for the
unblock check (registry Blocked-work section + roadmap §6 dependency graph):

- M002 durable WorkPlan foundation: sole closure-sequence gate was M001
  closure (interface dep on ADR-0003 + closed session/storage already
  satisfied). All hard gates now closed → move to `ready` in the same
  closure commit (plan status `blocked` → `ready for handoff`).
- M003 projection + completion arbiter: hard dep on M002 → stays `blocked`.
- M004 context-epoch reset/handoff: hard deps on M003 + closed
  context-continuity → stays `blocked`.
- M005 trajectory qualification: hard deps on M001–M004; M001 now closed →
  stays `blocked`, blocker narrows to M002–M004 closure.
- No other registered plan lists M001 as a hard/interface dep; no new
  corrective plan is required (§10 has no qualifying defect).

## 12. Registry updates

In the same closure commit:

- `plans/implementation/long-horizon-work-execution/001-...md`: `implemented`
  (already landed with `5d79bd9d`).
- `plans/implementation/long-horizon-work-execution/002-...md`: `blocked` →
  `ready for handoff`.
- `plans/registry.md`: long-horizon M001 row `ready` → `closed` (closure +
  implementation refs); add M002 `ready` row; move M002 out of Blocked work;
  narrow M005 blocker to M002–M004; advance execution-order gate 1 and the
  long-horizon control row to M001 closed / M002 ready; append M001 to
  recently-closed.
- `plans/subsystems/long-horizon-work-execution-roadmap.md`: M001 milestone
  + §12 table row → closed with closure/implementation refs; M002 row →
  ready; M005 blocker narrows to M002–M004 closure.
