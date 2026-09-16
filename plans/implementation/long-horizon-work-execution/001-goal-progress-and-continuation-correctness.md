# Long-Horizon Work Execution M001 — Goal Progress and Continuation Correctness

Status: implemented

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`

Primary class: invariant / capability

## 1. Objective

Make autonomous Goal continuation depend on observable progress or a verified live wait, remove the incorrect nonexistent `Blocked` status contract, and bound repeated no-progress recovery before the Goal enters the existing `AwaitingUser` state.

This milestone fixes the current Goal runtime without introducing WorkPlan storage yet.

## 2. Why this milestone is ready

Hard dependencies are already closed:

- Goal storage/runtime/budget/verification exists and is production-owned by `codegg-core`/AgentLoop.
- `RecoveryController` and `ProgressSignal` already provide bounded turn-local progress/replan/stall vocabulary.
- Scheduler jobs, AgentRuns, and managed processes expose host-owned status that can distinguish a real wait from model narration.
- Context-continuity M001-M004 are closed; this plan does not alter their authority.

No unresolved architecture decision remains after ADR-0003.

## 3. Current implementation evidence

At the baseline:

- `crates/codegg-core/src/goal/runtime.rs::should_continue()` checks `GoalStatus` and budget only. An Active Goal with remaining budget always receives another continuation prompt.
- `src/agent/turn_completion.rs::maybe_continue_goal()` caps the outer loop at `MAX_CONTINUATIONS = 32`; this is a final guard, not a useful stagnation policy.
- `build_continuation_prompt()` says the runtime counts consecutive blocked turns and allows a `Blocked` status, but `GoalStatus` has no `Blocked` variant.
- `src/agent/progress_recovery.rs` already defines `ProgressSignal::{None, NewEvidence, StateChanged, ChildAdvanced}`, no-progress/repeat incidents, and graduated `Nudge -> Correct -> RestoreBasePalette -> Replan -> Stall` behavior.
- Goal store uses monotonic revision/CAS for progress and completion transitions.
- Goal completion verifier already distinguishes host evidence from model claims.

## 4. Invariants that must not regress

- Goal budget/status remains authoritative; progress policy cannot revive Paused/Cancelled/BudgetLimited/Complete Goals.
- The model cannot mark a Goal complete solely by claiming progress.
- Existing GoalVerification remains the completion gate.
- Continuation remains bounded by existing turn/tool/token/wall-clock limits plus the outer emergency cap.
- A wait is considered verified only when a canonical live handle/state exists.
- No new `Blocked` Goal status is introduced in this milestone.
- No hidden reasoning is persisted to diagnose progress.
- User steering/cancellation interrupts continuation normally.

## 5. Scope

### In scope

- correct continuation prompt/status language;
- typed long-horizon continuation progress classification;
- host-owned progress fingerprint/input assembled from existing state;
- `Progress | VerifiedWait | NoProgress` continuation decision semantics;
- bounded consecutive no-progress counters/recovery transitions;
- replan instruction before terminal escalation;
- `AwaitingUser` transition after repeated genuine blocker/no-progress state;
- diagnostics/events/tests.

### Explicitly out of scope

- durable WorkPlan/WorkItem storage;
- changing Goal schema except minimal additive fields only if strictly required for restart-safe no-progress count;
- automatic creation of Goals;
- new scheduler/process polling framework;
- compaction changes;
- automatic permission/retry work from the sibling roadmap.

## 6. Required production changes

### Core/domain

Define a small typed continuation assessment, for example:

```rust
enum GoalProgressDisposition {
    Progress { fingerprint: String },
    VerifiedWait { handle: WaitHandleRef, fingerprint: String },
    NoProgress { fingerprint: String, reason: GoalNoProgressReason },
}
```

The exact shape may differ. It must contain only host-observable bounded metadata.

Build the fingerprint from authoritative revisions/statuses available at the continuation boundary. Candidate inputs include Goal revision/progress fields, Todo revision, canonical job/run/test status transitions, child advancement, and workspace mutation/evidence generation. Do not hash free-form model reasoning as proof of progress.

Reuse or adapt `ProgressSignal` and `RecoveryController` semantics where possible. Do not create another general recovery controller.

### Storage and migrations

Prefer deriving consecutive state within the running Goal continuation loop. If restart-safe blocker counting is required for correct behavior across a resumed Goal, add only a bounded additive Goal/runtime field/store record with a monotonic revision. Do not store arbitrary observations or transcript text.

Any migration must preserve existing Goal rows and status strings.

### Protocol and DTOs

Add bounded reason/status projection only if needed for frontend visibility. Existing clients may ignore it. Do not require frontend participation for correctness.

### Runtime and concurrency

Update `maybe_continue_goal()` so each continuation cycle:

1. accounts the just-finished turn;
2. reloads current Goal revision/status;
3. assesses progress/wait/no-progress from canonical state;
4. immediately exits on pause/cancel/replacement/budget;
5. resets no-progress recovery after authoritative progress;
6. for VerifiedWait, waits/polls only the existing canonical handle using the established scheduler/process mechanism; it must not relaunch the operation;
7. for NoProgress, applies a bounded correction/replan sequence;
8. after the configured small consecutive terminal threshold, transitions the same Goal revision to `AwaitingUser` with a concise blocker report.

The existing 32-continuation cap remains an emergency invariant but should not be the ordinary stagnation exit.

### Frontend or operator surface

Expose a concise event/status reason such as `progress`, `verified_wait`, `replan`, `awaiting_user_no_progress`, or `budget_limited`. Avoid dumping command output or plan content.

### Security and authorization

No authority changes. Wait/progress inspection is read-only. A model cannot create a fake WaitHandleRef through prose.

### Documentation and static guards

- correct `architecture/goal.md`;
- remove every production/docs assertion that a `Blocked` Goal status exists unless a later separate ADR adds it;
- document progress/wait/no-progress and AwaitingUser behavior;
- add a targeted source/test guard if useful to prevent reintroducing the nonexistent status wording.

## 7. Ordered work packages

### Work package A — Normalize progress/wait inputs

Intent: define the smallest host-observable continuation assessment contract.

Required changes:

- enumerate authoritative state sources;
- reuse RecoveryController progress vocabulary;
- create deterministic bounded fingerprinting;
- define how live jobs/runs/processes qualify as VerifiedWait.

Acceptance evidence:

- identical state produces identical fingerprint;
- new evidence/state/child completion changes progress;
- model prose alone does not.

### Work package B — Correct continuation state machine

Intent: stop blind budget-only continuation.

Required changes:

- thread assessment through Goal continuation;
- reset on progress;
- bounded no-progress recovery/replan;
- AwaitingUser terminal handoff for unresolved repeated blocker;
- preserve pause/cancel/budget behavior.

Acceptance evidence:

- scripted Goal that makes progress continues;
- stalled Goal replans then stops well before 32 cycles;
- live verified wait does not relaunch work.

### Work package C — Prompt and diagnostics correction

Intent: make model-facing contract match runtime truth.

Required changes:

- remove nonexistent Blocked status instruction;
- tell model to report open questions/blocker evidence through existing Goal progress tools;
- render reason codes/status for TUI/projections where appropriate.

Acceptance evidence:

- docs/prompts and enum agree;
- frontend status is bounded and source-attributable.

## 8. Failure, cancellation, restart, and contention semantics

- Failure to load progress evidence must not be interpreted as Progress. It should conservatively produce a bounded diagnostic and either replan or AwaitingUser according to the evidence criticality.
- A live handle that disappears between assessment and wait is reloaded; terminal success/failure becomes progress/evidence, not a duplicate launch.
- Cancellation immediately stops continuation and cannot be converted into blocker progression.
- Goal replacement/revision mismatch aborts the stale continuation decision.
- Concurrent Goal progress update wins through revision/CAS; stale recovery cannot overwrite it.
- Restart during a continuation relies on durable Goal/job/run state. If no durable no-progress counter is added, the counter may restart conservatively; it must not falsely mark complete.

## 9. Compatibility and migration

- Existing Goal rows remain valid.
- `AwaitingUser` already exists and becomes the canonical unresolved-blocker state.
- Existing `/goal resume` and budget semantics remain unchanged.
- No change to GoalCompletionRequest or verifier authority is required except optional reason integration.

## 10. Required tests

### Focused unit tests

- `should_continue`/new assessment for Progress, VerifiedWait, NoProgress, non-active and each budget axis;
- prompt contains no nonexistent Blocked contract;
- progress fingerprint stability and change cases;
- RecoveryController reset/replan threshold integration.

### Integration tests

- progressing scripted Goal performs multiple continuations and completes through existing verifier;
- narration/no-state-change Goal reaches replan/AwaitingUser before emergency cap;
- live scheduler job produces VerifiedWait and is not duplicated;
- failed job becomes evidence/progress then allows model recovery.

### Restart and recovery tests

- resume Active Goal with live durable job;
- resume after job completed while daemon was down;
- stale Goal revision cannot apply prior no-progress decision.

### Contention and cancellation tests

- user pause/cancel/replace races with continuation assessment;
- steering during replan stops the old path cleanly.

### Security and negative tests

- model-provided text naming a fake job does not produce VerifiedWait;
- hidden reasoning/tool raw output is absent from durable progress fingerprint diagnostics.

### Migration and compatibility tests

Only required if storage changes; legacy Goal rows must load with safe defaults.

## 11. Required verification commands

```bash
cargo test -p codegg-core -- goal
cargo test --test agent_loop_harness -- goal
cargo test --test goal_verification
python3 scripts/check_core_boundary.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

If exact test target names differ at implementation time, use the repository's current focused equivalents and record the deviation in closure evidence. Do not add a new CI lane.

## 12. Documentation updates

- `architecture/goal.md`
- `architecture/agent.md` where autonomous continuation is described
- relevant prompt/agent documentation
- subsystem roadmap milestone status after implementation

## 13. Acceptance criteria

- An Active Goal no longer auto-continues solely because budget remains.
- Genuine host-observed progress resets stagnation recovery.
- A verified live wait is distinguished from no progress and does not duplicate the awaited operation.
- Repeated no-progress state triggers replan and then `AwaitingUser` before the 32-cycle emergency cap.
- No model/document claims a `Blocked` Goal status exists.
- Existing pause/cancel/budget/completion verifier behavior remains correct.

## 14. Stop conditions

Stop and report rather than improvise if:

- correct progress assessment requires a new general workflow store (that belongs to M002);
- a provider-specific hidden-state API would be required;
- implementation would weaken GoalVerification or treat model prose as evidence;
- a new Goal status is necessary rather than using `AwaitingUser`;
- reliable wait classification cannot be obtained from canonical job/run/process state.

## 15. Closure evidence required

- implementation commit(s);
- requirement-to-evidence matrix for Progress/Wait/NoProgress and status transitions;
- focused test outcomes including stalled and wait cases;
- evidence that the incorrect Blocked prompt contract is gone;
- pause/cancel/revision race evidence;
- exact verification commands run;
- residual limitations and recommendation to close or corrective-pass.

## 16. Handoff notes

Preserve the closed context-continuity architecture. Favor reuse of `progress_recovery.rs` and existing Goal/job/run stores over new cross-cutting abstractions. The emergency 32-loop cap should remain even after normal stagnation handling is corrected.
