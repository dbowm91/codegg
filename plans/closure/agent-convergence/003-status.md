# Agent Convergence M003 — Closure Status

Status: closed

## 1. Scope and decision

M003 is strictly closed. CodeGG now supports bounded owner-directed repair
and replan cycles on top of the M002 produce/verify path. Continuations remain
ordinary scheduler-owned durable runs, use explicit immutable Git provenance,
and retain explicit parent integration and host goal-verification boundaries.

## 2. Implementation revisions and dependency review

- Planning activation: `11cbad8e` (`plans: activate convergence repair milestone`).
- Implementation: `33ec0376ea0e63e16ec977c8414fd4ebb577de1a` (`feat: add bounded convergence repair and replan`).
- Reviewed hard dependency: M002 implementation
  `28008ddd434ac2be9b57620cd84ce18b6064959a`, with accepted closure
  `plans/closure/agent-convergence/002-status.md`.
- The implementation plan moved `active -> closing` when production landed and
  is moved `closing -> implemented` with this accepted closure record.

## 3. Repair and replan state-machine evidence

- `repair` and `replan` are legal only from `AwaitingDecision`, after the
  durable revision/CAS decision is accepted. The store advances exactly one
  cycle; stale or duplicate decisions cannot create a competing continuation.
- Repair requires a terminal completed run, a successful clean
  `AgentRunResult`, matching run/result worktree identity, matching repository
  identity, a result commit, and a resolvable full commit object.
- Repair prompts include the original objective/criteria, bounded verifier
  findings, prior structured summary/paths, and the exact result commit. The
  continuation is submitted with an idempotency key and a host-owned
  `base_commit` payload.
- Replan supports only `original` and `last_clean_result`. The latter uses the
  same clean-result validation as repair; arbitrary paths, refs, and model
  supplied values are rejected before submission.
- Every continuation enters a new producer cycle and receives a fresh
  independent read-only verifier after the producer reaches a clean successful
  result. No parent checkout is mutated and no child worktree is reused.

## 4. Repair/replan base and result commit matrix

| Continuation | Base selected by host | Result recorded by host | Parent integration |
|---|---|---|---|
| Initial producer | owning checkout at scheduler preparation | structured result `base_commit -> result_commit` | none |
| Repair | prior cycle clean `result_commit` | new cycle result commit in a new worktree | explicit typed integration only |
| Replan/original | recorded cycle-0 source base | new cycle result commit in a new worktree | explicit typed integration only |
| Replan/last_clean_result | current cycle's validated clean producer result | new cycle result commit in a new worktree | explicit typed integration only |

`egggit::resolve_commit` accepts only a full 40/64-character object id and
requires Git to resolve to the same id. The scheduler passes that value to
`CreateWorktreeRequest.base_commit`; worktree allocation remains owned by the
existing `WorktreeService`.

## 5. Worktree isolation and lease evidence

The continuation path creates a new durable `AgentRun` through `TaskTool` and
the existing `JobSubmissionService`. The `SubagentJobExecutor` carries the
host-selected base into `WorktreeService::create`, which allocates a distinct
managed worktree and lease for mutation-capable children. The prior run must be
terminal before repair eligibility is accepted, and its worktree/result
provenance must match. No file copying or active lease transfer was added.

## 6. Cycle, budget, and no-progress bounds

- Default `max_cycles` is 2; the core/store hard maximum is 4.
- Producer width is host-limited to 3 and the implemented strategy is the
  single-producer strategy. No generic DAG or parallel mutating composition was
  introduced.
- Existing root/session/project scheduler, tool, and token budgets remain the
  authority for each child run. The convergence coordinator adds a bounded
  creation-time wall-clock check, capped by the 24-hour host maximum.
- A continuation is exhausted when the cycle budget or deadline is reached.
  Consecutive equivalent result-commit/verdict fingerprints also exhaust
  rather than permit a blind third retry.
- Exhausted and terminal records cannot create another child.

## 7. Producer-group disposition

No producer-group strategy was enabled in M003. The model-facing schema
accepts only `strategy = single`; the width bound is represented and checked,
but alternatives and combined mutating subtasks remain deferred until an
explicit safe composition/integration plan exists. This preserves the
existing run-group authority and avoids implying that independent commits
compose automatically.

## 8. Model-profile and configuration evidence

- `OrchestrationTier` is a coarse profile field with conservative default
  `SoloPreferred`; unknown models and existing vendor adapters are not
  promoted from marketing claims.
- `ModelProfileResolver` and declarative adapter inheritance support explicit
  user/project overrides. Config-layer model profiles are merged by key.
- `[orchestration]` defaults to `auto_convergence = false`,
  `default_max_cycles = 2`, and `max_producers_per_cycle = 1`. Host bounds
  clamp cycles to 1–4, producers to 1–3, and an optional wall-clock setting to
  at most 24 hours.
- When enabled, only a root model resolved as `ConvergenceCapable` receives
  optional prompt guidance. Explicit convergence invocation remains available
  regardless of this guidance setting.

## 9. Projection and authority evidence

The additive convergence projection now carries remaining cycles, selected
terminal run/result commit, and finding count, while preserving bounded run
handles and verdict summaries. The TUI sidebar renders the remaining budget
and verifier finding count. A semantic pass remains advisory; selected results
are an explicit `AgentRunIntegrationService` handoff, and deterministic
`GoalVerificationService` remains the only goal-completion authority.

## 10. Restart, cancellation, and idempotency evidence

Decision persistence precedes continuation submission. Reconciliation sees a
persisted `Repairing`/`Replanning` state and an empty or populated next-cycle
producer reference; the same convergence/phase/ordinal idempotency key makes
submission repeat-safe. CAS prevents a second owner decision from creating a
competing cycle. Existing cancellation routes only the convergence's active
producer/verifier run ids through run control; deadline and terminal transitions
remain durable. Existing M002 event-wakeup/reconciliation behavior is retained.

## 11. Verification and findings

Successful focused verification:

- `cargo fmt --all -- --check`
- `git diff --check`
- `cargo test -p codegg-core agent_convergence --locked` — 9 passed
- `cargo test -p codegg-core worktree --locked` — 12 passed
- `cargo test -p codegg-core model_profile --locked` — 17 passed
- `cargo test -p codegg-config paths::tests::test_merge_configs_merges_model_profiles_and_orchestration --locked` — passed
- `cargo test --test agent_convergence --locked` — passed
- `cargo clippy -p codegg --lib --locked -- -D warnings` — passed
- `scripts/verify.sh quick` — passed, including workspace all-target checks
- Relevant execution, projection, scheduler, identity, provider, and boundary
  guards passed.

Two unrelated pre-existing static guard findings remain outside M003 scope:
`check_project_catalog_invariants.py` expects the repository's older storage
layout value 48 while the code is already at 49, and
`check_tool_broker_boundary.py` reports the existing direct structured call in
`src/tool/review.rs:216`. No critical, high, or medium M003 finding remains.

## 12. Roadmap, registry, and final disposition

The agent-convergence roadmap is closed at M003. The implementation plan is
marked `implemented`, and this record is the accepted closure evidence.

The closure audit searched `plans/registry.md`, subsystem roadmaps, and
implementation-plan dependency declarations for downstream references to
agent-convergence M003. No future plan is blocked on M003: the related
agent-run/worktree M004 is already closed, and memory-to-skill work is
independent and remains sequenced on its own predecessors. Therefore no
future plan status was changed or unblocked by this closure.

No ADR, CI lane, provider compatibility promise, token-accounting authority,
or implicit Git integration was added. Final subsystem disposition: closed;
no corrective pass required.
