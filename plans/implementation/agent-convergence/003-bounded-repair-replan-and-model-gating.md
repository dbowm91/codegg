# Agent Convergence M003 — Bounded Repair, Replan, and Model Gating

Status: ready

Repository baseline: `1bee32578566cc6cdf4025002af781309d8f29f4`

Source subsystem roadmap:

- `plans/subsystems/agent-convergence-roadmap.md`

Hard dependency:

- M002 `plans/implementation/agent-convergence/002-independent-verifier-and-owner-decision.md` must be strictly closed.

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#18-remote-projects-and-execution-targets` only insofar as existing execution context must not be made path-global;
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Applicable decisions and dependencies:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md` remains authoritative for worktree isolation, child result commits, and explicit parent integration.
- M001/M002 convergence contracts remain authoritative.
- no new ADR is required if this milestone uses existing worktree/base-commit and model-profile configuration seams without changing their ownership. Stop if implementation requires implicit integration, scheduler authority changes, or a new external compatibility contract.

Primary class: capability / reliability / polish

## 1. Objective

Extend the proven single-cycle convergence path into a hard-bounded repair/replan mechanism that can safely continue from explicit Git result provenance, optionally use a small bounded producer group, and expose automatic convergence only under conservative model/configuration policy.

The target behavior is:

```text
cycle 1 producer(s)
      -> independent verifier
      -> Revise
      -> owner decision: repair
      -> new repair AgentRun based on explicit prior result_commit
      -> new managed worktree
      -> independent verifier for cycle 2
      -> Pass
      -> owner accept
      -> optional explicit existing Git integration
      -> existing host goal completion path, if requested
```

The host must be able to stop this process predictably. “Keep trying until the verifier likes it” is not an acceptable runtime contract.

## 2. Explicit non-goals

M003 does not:

- add arbitrary workflow graphs or user-authored state machines;
- allow unlimited repair cycles;
- transfer a live mutable worktree between simultaneously active agents;
- automatically merge/cherry-pick/rebase a producer/repair commit into the parent;
- treat semantic verifier pass as a deterministic test result;
- add majority-vote debate or fleets of verifiers;
- enable Agent Team mode for every model or task by default;
- train/evaluate foundation models or automatically rewrite model profiles from telemetry;
- make vendor-provided Agent Team protocols canonical;
- add cross-daemon repair execution beyond whatever current `AgentRun`/execution-target support already provides;
- resurrect legacy team inbox/outbox code.

## 3. Re-inspection required before implementation

Before editing, re-read:

- M001/M002 closure records and final APIs;
- `src/agent/run_integration.rs` and `crates/codegg-core/src/run_result.rs` for base/result commit validation;
- `crates/codegg-core/src/worktree_service.rs` and `architecture/worktree.md` for lease generation, explicit base commit, retained dirty/conflicted state, and safe creation;
- `src/tool/task.rs` and scheduler-owned `AgentRun` submission payload for whether a host-selected child base commit can already be supplied;
- `crates/codegg-core/src/model_profile/types.rs`, `resolve.rs`, adapter TOMLs, and config schema for the smallest orchestration capability seam;
- session projection/TUI convergence state from M002;
- goal-verification boundary and ordinary parent integration flow.

Do not assume a worktree can be safely “reused” merely because the previous child is terminal. The current lease/service contract is authoritative.

## 4. Repair semantics

### 4.1 Preferred continuation rule

Repair must start from a durable clean producer result commit, not from a path or uncommitted child workspace.

Eligible prior result:

```text
AgentRunResult.status == Succeeded
result_commit != None
repository_state == Clean
same RepositoryId as convergence owner
recorded base/result commit can be resolved
prior worktree/run is terminal
```

The exact accepted repository-state set may include a narrowly documented `Dirty` case only if existing result collection proves the result commit fully captures the intended mutation and the dirty residue is irrelevant. Default to requiring clean state.

Repair construction:

1. read prior cycle's authoritative `AgentRunResult`;
2. verify repository identity and result commit again immediately before submission;
3. create a new repair child request whose host-owned base is that result commit;
4. submit through the ordinary scheduler-owned `AgentRun` path;
5. allocate a new managed worktree using that explicit commit as base;
6. give the repair agent the verifier's bounded findings/repair requests plus original criteria;
7. record the new producer/repair run in the next cycle;
8. use a fresh independent verifier after repair completes.

Do not perform an intermediate parent integration merely to establish the next repair base. The result commit is an immutable Git object and can seed another isolated worktree directly.

### 4.2 Ineligible prior results

Return a typed `CannotRepairFromResult`/`NeedsAttention` decision when:

- no result commit exists;
- repository is conflicted or unknown;
- commit cannot be resolved;
- repository identity differs;
- prior result/worktree provenance is incomplete;
- current policy forbids the required child mutation;
- convergence has exhausted cycle/token/wall-clock budgets.

The engine may offer `replan`/`escalate`; it must not copy files out of the old worktree ad hoc or silently use the parent's current HEAD.

### 4.3 Repair agent prompt/context

The repair agent receives:

- original bounded objective/criteria;
- verifier `Revise` findings and repair requests;
- exact base commit identity;
- normal project/runtime assets for its own pinned turn;
- optionally the prior producer summary and changed-path list from structured result.

It does not need the previous producer's hidden reasoning. The Git base already carries the code artifact.

## 5. Replan semantics

`replan` starts a new producer cycle rather than a repair of the prior result.

The owner must choose/host must record the source base from a small safe set:

```text
original convergence base
last clean accepted producer result commit
explicit owner-selected commit already validated in the same repository
```

In the first M003 implementation, prefer only `original` and `last_clean_result`. Do not accept arbitrary model-generated refs unless they pass the existing typed Git object/ref validation and ownership policy.

A replan prompt receives previous verifier findings as lessons but is told to reconsider the approach rather than make a narrow patch.

## 6. Cycle and budget enforcement

### 6.1 Hard limits

Host code must enforce:

```text
default max_cycles = 2
hard max_cycles <= 4
max producer runs per cycle <= 3
```

The exact default producer width may remain 1. The upper bound may never exceed the existing run-group hard limit and should be substantially lower by default.

A model message, verifier repair request, custom agent definition, or project skill cannot raise hard limits.

### 6.2 Inherited budgets

Each producer/verifier/repair run still consumes normal root/session/project scheduler and token/tool budgets. The convergence service also tracks a bounded aggregate budget envelope derived from the owner at creation time.

At minimum track/limit:

- cycles consumed;
- total child runs created by the convergence;
- aggregate wall-clock deadline;
- optional aggregate model-token allowance if the current run-budget service can enforce it without duplicating token accounting.

Do not create a second independent token accounting source if existing run/root usage can answer the question. Prefer a host cap/check over duplicated counters.

### 6.3 Stop conditions

Automatically transition to `Exhausted` rather than spawn more work when any hard convergence bound is reached. Return the last verifier verdict and explicit guidance to the owner.

Repeated equivalent verifier findings across cycles should feed the existing progress/no-progress classification or a small convergence fingerprint check. Two cycles with materially identical result commit/diff and verdict findings should favor `Exhausted`/`Escalate` rather than a third blind retry.

Do not create another generalized doom-loop detector; reuse existing fingerprints/normalization where practical.

## 7. Small bounded producer groups

M003 may extend one producer to a bounded group only for use cases where alternatives or independent parallel subtasks are useful.

Use existing `spawn_many`/`AgentRunGroupService`; convergence only records the group and defines how results feed verification.

Supported initial producer strategies should be narrow, for example:

```text
single                       # existing M002 behavior
all_independent_subtasks      # verifier checks the combined declared result set
best_candidate                # several complete candidates, verifier compares them
```

Do not implement arbitrary dependency graphs between producers.

For `best_candidate`:

- each candidate is isolated in its own worktree if mutating;
- all results are bounded `AgentRunResult` values;
- verifier is told these are alternatives and must select/reject using explicit criteria;
- convergence records the selected run/result commit in the typed verdict/decision state;
- non-selected branches remain ordinary retained child results and are not automatically integrated or deleted beyond current safe cleanup policy.

For `all_independent_subtasks`, do not pretend separately based child commits compose automatically. Unless current integration service has a safe explicit multi-result composition, keep initial parallel implementation to read-only/research or alternative-candidate modes. If combined code integration is needed, register a separate integration-planning milestone rather than inventing implicit merge order in M003.

## 8. Model-profile and configuration gating

### 8.1 Why a gate is needed

MiniMax's M2.7 report explicitly treats stable role boundaries, adversarial reasoning, protocol adherence, and behavioral differentiation as native model capabilities. Their Agent Team product also warns that multi-agent execution adds handoff, sharing, aggregation, verifier, retry, time, and token costs.

CodeGG should therefore distinguish “can execute a child task” from “is a good default leader/verifier in a multi-cycle convergence protocol.”

### 8.2 Smallest profile extension

Add one coarse field rather than many booleans, for example:

```rust
enum OrchestrationTier {
    SoloPreferred,
    DelegationCapable,
    ConvergenceCapable,
}
```

Requirements:

- represented in `ModelProfileConfig` and `ResolvedModelProfile` or an equivalent existing policy object;
- custom/project config can explicitly override it;
- default/unknown model profile is conservative (`SoloPreferred` or `DelegationCapable`, but not automatic `ConvergenceCapable`);
- existing models retain current behavior when the field is absent;
- vendor adapter files are not upgraded to `ConvergenceCapable` solely from marketing claims;
- resolution/merge tests cover absent/default/override behavior.

If current architecture has a more appropriate execution-policy location that avoids adding the field to every profile, use it, but retain one coarse concept.

### 8.3 Automatic invocation policy

Automatic convergence remains opt-in in M003.

Add a configuration shape equivalent to:

```jsonc
{
  "orchestration": {
    "auto_convergence": false,
    "default_max_cycles": 2,
    "max_producers_per_cycle": 1
  }
}
```

Exact location/names should follow config conventions. Hard host maxima cannot be raised through config.

When `auto_convergence = false`, explicit user/model invocation of the convergence action remains available subject to ordinary permissions/policy.

When enabled, only a root/owner using an effectively `ConvergenceCapable` model should receive prompt guidance encouraging formal convergence for long/high-risk/ambiguous delivery. Do not build an LLM router or separate complexity classifier in this milestone. The model may choose the existing bounded action; the host enforces costs and limits.

A future evaluation can assign built-in profile defaults based on measured CodeGG harness performance.

## 9. Owner decision extensions

Enable M003 decisions:

```text
repair
replan
```

Decision processing requirements:

- legal only from `AwaitingDecision` with a compatible verifier verdict;
- revision/CAS checked;
- validates remaining cycle/budget before accepting;
- persists the decision before submitting new work or uses an explicit transaction/outbox pattern so restart cannot forget which transition was accepted;
- if submission fails after the decision is persisted, convergence remains recoverable with a state such as `Repairing` plus no accepted next run, and reconciliation can retry submission idempotently from the same invocation key;
- a second owner decision cannot create a competing repair cycle.

`accept` from a `Revise` verdict may be allowed only as an explicit owner override if product policy wants it, but the projection must clearly record `accepted_with_findings`. It still does not bypass deterministic host goal checks. A conservative first implementation may require `stop/escalate/repair/replan` for `Revise` and reserve `accept` for `Pass`.

Document whichever rule is implemented and test it.

## 10. Final result selection and integration handoff

When convergence reaches `Completed`, persist/reference one selected terminal producer/repair run as the convergence result.

Expose:

```text
selected_run_id
selected_result_commit
last_verifier_verdict
cycle_count
accepted_with_findings flag if supported
```

The result remains a handoff. Parent integration must call the existing explicit `AgentRunIntegrationService`/typed Git operation. Revalidate parent base/cleanliness at integration time exactly as ordinary child integration does.

Convergence must not cache a “safe to merge forever” bit. Repository state may change after semantic verification.

## 11. Projection, diagnostics, and operator controls

Expand M002 projection with:

- current and hard max cycles;
- remaining cycle budget;
- producer strategy and count;
- selected result run when terminal;
- `repairing`/`replanning` state;
- last verdict class/finding count;
- exhaustion/no-progress reason.

The owner can continue to `message`/`interrupt` currently active child runs through their run IDs. A convenience convergence steering action may route to the active producer/verifier only if it preserves exact run-control authorization and makes the target explicit in the result. Do not create ambiguous broadcast semantics in this milestone.

A `/convergence` or TUI detail view may show the cycle timeline; keep ordinary projection bounded.

## 12. Expected production-code touch set

Expected areas:

- convergence coordinator/core types from M001/M002;
- `src/tool/task.rs` for repair/replan decisions and optional producer strategy fields;
- scheduler/delegation request construction only to add an existing-service-compatible explicit child base commit if not already present;
- `crates/codegg-core/src/worktree_service.rs` / worktree creation only if the current explicit-base API is not exposed at the durable run preparation seam;
- `crates/codegg-config/src/schema.rs` for bounded orchestration config;
- `crates/codegg-core/src/model_profile/{types,resolve,adapter}.rs` and adapter docs/tests for the coarse tier;
- session projection/TUI convergence detail rendering;
- architecture docs: `agent.md`, `worktree.md`, `model-adapters.md`, `config.md`, `goal.md` as applicable.

Do not edit unrelated provider adapters merely to assign speculative tiers.

## 13. Required tests

### Repair provenance

- clean producer result commit can seed one repair child in a distinct managed worktree;
- repair child base equals exact recorded prior result commit;
- missing commit, conflicted/unknown state, wrong repository, or unresolvable commit is rejected before child submission;
- repair never mutates/reuses the prior child worktree concurrently;
- restart after accepted repair decision but before/after child submission creates at most one repair run.

### Replan

- original-base replan produces a new child from the recorded original base;
- last-clean-result replan uses only a validated same-repository commit;
- model-supplied arbitrary path/ref cannot bypass typed Git validation.

### Bounds/no-progress

- default two-cycle behavior;
- hard max <=4 even if config/model requests more;
- max producer count bounded <=3 and existing run-group hard bound;
- token/wall-clock/root budgets still apply;
- repeated equivalent result/verdict fingerprint exhausts/escalates rather than looping;
- exhausted convergence cannot create another child.

### Producer groups

If implemented:

- alternative candidates use distinct worktrees and are all attributable;
- verifier packet identifies alternatives correctly;
- one selected result is persisted;
- non-selected candidate does not auto-merge/delete;
- unsupported combined-mutating-subtask composition fails closed rather than guessing merge order.

### Model/config gating

- absent profile/config preserves existing behavior;
- unknown model is not automatically convergence-capable;
- explicit config override resolves deterministically;
- `auto_convergence=false` remains default;
- enabling auto guidance cannot raise host cycle/fan-out limits;
- MiniMax and other built-ins retain only evidence-backed tier defaults.

### Goal/integration boundary

- semantic pass/owner accept still requires explicit Git integration;
- parent HEAD movement after verification causes ordinary integration revalidation failure;
- semantic pass cannot override failed host goal evidence.

## 14. Verification commands

Required after implementation:

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core agent_convergence --locked
cargo test -p codegg-core worktree --locked
cargo test -p codegg-core model_profile --locked
cargo test --test agent_convergence --locked
```

Run the focused integration-service and configuration tests touched by the implementation using their existing exact targets.

Then:

```bash
scripts/verify.sh quick
```

No new CI lane, live-provider Agent Team test, or benchmark threshold is required for closure. If model-quality telemetry is gathered, record it as informational evidence rather than making nondeterministic model output a correctness gate.

## 15. Acceptance criteria

M003 may close only when:

1. `repair` and `replan` are legal, bounded, revision-checked owner decisions rather than free-form model continuation.
2. Repair can continue only from an explicit validated same-repository result commit.
3. Every repair/replan child is a new scheduler-owned durable run and mutating child uses a new managed worktree.
4. Dirty/conflicted/missing-result state cannot be silently copied into a later cycle.
5. A fresh independent verifier evaluates each completed repair/replan cycle.
6. Hard cycle limit is four or lower; default is two unless documented evidence justifies a lower value.
7. Producer width is tightly bounded and does not become a generic DAG.
8. Repeated equivalent failures/verdicts stop/escalate rather than loop indefinitely.
9. Automatic convergence is opt-in and uses one conservative model/profile policy seam.
10. Unknown/vendor models are not automatically promoted to convergence-capable from marketing claims alone.
11. Selected final result remains an explicit Git integration handoff; no automatic merge occurs.
12. Existing host goal verification remains final completion authority.
13. Projection/restart/cancellation remain durable and bounded across multiple cycles.
14. Architecture/config/model-profile documentation is current.
15. Focused tests and `scripts/verify.sh quick` pass.

## 16. Stop conditions

Stop and register a follow-up/ADR if:

- repair requires transferring an active worktree lease between concurrent owners;
- correct repair requires implicit parent-branch integration;
- combined code-producing parallel subtasks require a new merge/workflow engine;
- orchestration capability becomes a public provider compatibility promise rather than a local policy hint;
- budget enforcement requires a new token-accounting authority competing with existing run/root accounting;
- model-specific automatic team behavior requires a new router/classifier subsystem.

## 17. Closure evidence required

Create `plans/closure/agent-convergence/003-status.md` containing:

- exact implementation revision and M002 dependency revision;
- repair/replan base/result commit matrix;
- worktree isolation/lease evidence;
- cycle/budget/no-progress bounds;
- producer-group disposition if included;
- model-profile/config default and override evidence;
- explicit integration and goal-authority regressions;
- restart/cancellation/idempotency evidence across multiple cycles;
- focused and quick verification output;
- unresolved findings and final subsystem disposition.

If all three milestones close, mark `plans/subsystems/agent-convergence-roadmap.md` closed and update the active registry. Do not rewrite the historical agent-run/worktree or goal-verification closures.
