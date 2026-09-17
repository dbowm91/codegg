# Long-Horizon Work Execution M005 — Long-Horizon Trajectory and Recovery Qualification

Status: ready for handoff

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`

Primary class: invariant / closure

Hard dependencies: M001-M004 closure.

## 1. Objective

Qualify the completed long-horizon architecture under deterministic extended trajectories: large WorkPlans, repeated compaction, optional fresh context epochs, daemon restart, subagent/test jobs, failures/waits, user steering, no-progress recovery, and premature-final attempts. Close the workstream only if CodeGG preserves authoritative work state and completes or stops correctly without duplicate execution.

This is a verification/corrective milestone, not a new feature phase.

## 2. Why this milestone is blocked

It exists to verify the integrated behavior of M001-M004. Beginning before those contracts close would produce brittle tests against moving interfaces.

## 3. Current implementation evidence

The repository already contains useful deterministic harness components:

- scripted/mock provider tests in the agent-loop harness;
- forced context compaction tests from the closed context-continuity workstream;
- SQLite restart/migration fixtures;
- scheduler Job/Attempt and AgentRun test seams;
- Goal verification fixtures;
- managed-process/test-runner fake or bounded execution seams;
- static ownership guards and proportional `verify.sh quick` verification.

M005 should compose these rather than add external-model CI or a second testing framework.

## 4. Invariants that must not regress

- Tests use host-controlled deterministic providers/fixtures by default; no public API key is required for routine verification.
- Qualification cannot weaken bounds merely to make the test pass.
- No duplicate side effect is accepted as normal recovery behavior.
- Long-plan completion is evaluated from WorkPlan/host evidence, not final prose.
- Prepared/uninstalled context checkpoints do not become resume authority.
- Fresh epoch/compaction does not change permissions, model selection, workspace, Goal budget, or WorkPlan ownership.
- Closure must record failures/unrun evidence honestly.

## 5. Scope

### In scope

- one or more deterministic long-trajectory scenario fixtures;
- at least eight context transitions (compactions and/or selected epochs) in representative scenario;
- daemon/storage reopen boundaries;
- Goal-bound and ordinary turn-scoped WorkPlans;
- subagent/test/job evidence and verified waits;
- user steering after prior compaction;
- intentional repeated no-progress and replan/AwaitingUser;
- premature-final answer attempts;
- stale revision/cancellation/restart races;
- bounded performance/resource assertions;
- corrective fixes only where required to satisfy the documented architecture.

### Explicitly out of scope

- benchmark leaderboard claims;
- permanent cloud-model/nightly matrix;
- broad refactors unrelated to failures found by the scenarios;
- new general chaos framework;
- reopening closed context-continuity work without a concrete regression.

## 6. Required production changes

No production changes are required if M001-M004 pass qualification. Any defect discovered must be fixed in its canonical owner and documented as a corrective finding in the later closure record. If the fix materially exceeds this roadmap, stop and create a corrective implementation plan rather than hiding scope expansion in M005.

### Scenario harness contract

Build reusable scripted steps that can express:

```text
user objective
-> WorkPlan create/update
-> provider tool calls/final attempts
-> host tool/job/test outcomes
-> forced compaction
-> optional epoch reset
-> daemon/store reopen
-> steering/update
-> completion assessment
```

Assertions inspect canonical stores/events plus provider-visible bounded context. Avoid matching incidental prompt prose unless testing a stable contract.

## 7. Ordered work packages

### Work package A — Scenario fixtures

Create a deterministic large plan with multiple phases/dependencies, a delegated child, passing/failing test evidence, and a user-steering change after an early phase.

Acceptance evidence: fixture can run without network/external provider and exposes canonical state checkpoints.

### Work package B — Context-transition trajectory

Force at least eight transitions across one logical task, including repeated compaction and at least one fresh epoch where policy permits. At each transition assert objective, current phase/item, remaining required work, next action, steering, and evidence identity.

### Work package C — Failure/restart trajectory

Inject daemon/storage reopen at safe and awkward boundaries: in-progress item with live/terminal job, prepared compaction checkpoint, stale WorkPlan update, and Goal continuation.

### Work package D — Completion/stall trajectory

Have scripted provider attempt final answer with unfinished required items; assert arbiter continuation. Separately create no-progress turns and assert replan then AwaitingUser/blocked report before emergency continuation cap.

### Work package E — Closure reconciliation

Run focused/broad proportional verification, update roadmap/registry/architecture status, and create `plans/closure/long-horizon-work-execution/005-status.md` with full evidence matrix.

## 8. Failure, cancellation, restart, and contention semantics

The scenarios must explicitly demonstrate:

- no side effect/job is relaunched solely because provider context was reset;
- cancellation leaves unfinished items unfinished/cancelled rather than completed;
- restart uses current WorkPlan/Goal/job state and ignores prepared checkpoint candidates;
- stale plan/checkpoint revisions cannot overwrite newer steering/progress;
- failed/inconclusive completion evidence does not become success;
- missing recovery artifact produces bounded degradation, not loss of objective/plan authority.

## 9. Compatibility and migration

Run at least one scenario with a legacy/no-WorkPlan session and one old continuation-checkpoint fixture if retained by current tests. New features must not make legacy sessions unreadable.

## 10. Required tests

### Focused unit tests

Only add unit regressions for concrete defects discovered during qualification.

### Integration tests

- ordinary large plan full trajectory;
- Goal-bound large plan full trajectory;
- premature final -> continuation;
- verified wait -> same handle -> completion;
- no-progress -> replan -> AwaitingUser;
- passing/failed host test evidence.

### Restart and recovery tests

- restart after plan mutation;
- restart during live job;
- restart after job completion before model sees result;
- restart around prepared/installed compaction/epoch boundary.

### Contention and cancellation tests

- user steering vs stale checkpoint/plan update;
- cancel vs completion assessment;
- child update vs parent current-item transition.

### Security and negative tests

- model text cannot forge test/job/evidence completion;
- no hidden reasoning/credential content appears in WorkPlan/checkpoint diagnostics;
- context epoch preserves execution-policy identity.

### Migration and compatibility tests

- pre-WorkPlan DB/session path;
- prior continuation checkpoint version if still supported.

## 11. Required verification commands

```bash
cargo test -p codegg-core -- work_plan
cargo test -p codegg-core -- goal
cargo test --test agent_loop_harness
cargo test --test goal_verification
python3 scripts/check_core_boundary.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Do not add an all-platform or live-provider matrix solely for this milestone. Record any host-specific skipped evidence.

## 12. Documentation updates

- finalize `architecture/work_plan.md` and affected Goal/context/agent docs;
- update `plans/subsystems/long-horizon-work-execution-roadmap.md` milestone statuses;
- update `plans/registry.md`;
- create closure record.

## 13. Acceptance criteria

- Representative large plan retains authoritative objective/work state across at least eight context transitions.
- Later user steering never reverts after compaction/epoch reset.
- Work completed before a reset is not redundantly re-executed because the model forgot it.
- Premature final answers do not close unfinished required plans.
- Complete plans terminate without extra continuation.
- Verified waits do not duplicate jobs/processes.
- No-progress trajectories replan/stop within designed bounds.
- Restart does not convert ambiguous in-progress work into replay or success.
- No new second compaction/workflow/history owner exists.

## 14. Stop conditions

Stop and open a corrective plan if qualification reveals an ownership flaw requiring redesign of WorkPlan, Goal, context rollover, scheduler/AgentRun, or evidence semantics rather than a bounded bug fix.

## 15. Closure evidence required

- scenario definitions and deterministic seeds/scripts;
- per-transition state/evidence table for the representative trajectory;
- requirement-to-test matrix;
- restart/cancellation/contention outcomes;
- exact verification commands and outputs/status;
- list of defects found/fixed and why earlier tests missed them;
- residual limits/host-specific evidence;
- recommendation: closed, conditionally closed, or corrective pass required.

## 16. Handoff notes

Keep this milestone closure-oriented. The purpose is not to invent another harness; extend the existing scripted-provider/context tests with enough scenario composition to prove the integrated contracts.
