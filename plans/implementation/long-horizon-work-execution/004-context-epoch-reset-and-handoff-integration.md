# Long-Horizon Work Execution M004 — Context-Epoch Reset and Structured Handoff Integration

Status: blocked

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/long-horizon-work-execution-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0003-long-horizon-work-state-and-context-epochs.md`

Primary class: infrastructure / capability

Hard dependencies: M003 closure and closed context-continuity M004.

## 1. Objective

Integrate durable WorkPlan identity/revision into the existing continuation-checkpoint handoff and add an optional, model-profile/policy-aware **fresh provider context epoch** path that reconstructs context from authoritative host state without rewriting session history or creating a second compaction engine.

## 2. Why this milestone is blocked

M003 must first provide stable WorkPlan projection/completion semantics. The closed context-continuity workstream already supplies the checkpoint/rollover primitives this milestone must reuse.

## 3. Current implementation evidence

- `src/context/compaction.rs` is the canonical reduction owner.
- `src/context/rollover.rs` owns transactional checkpoint prepare/verify/install and restart validation.
- `ContinuationCheckpointStore` provides durable installed lineage and compaction event markers.
- current continuation snapshot already captures Goal/Todo/plan-path/digest and bounded recovery references.
- turn start can inject exactly one installed continuation block with newer Goal precedence.
- model profiles already describe context window, prompt profile, late-system behavior, tool reliability and task-state policy, providing a natural place for epoch-reset compatibility/defaults.

## 4. Invariants that must not regress

- No new compaction/history/checkpoint owner.
- Fresh epoch does not delete or mutate durable session message history.
- A context reset does not reset workspace, Git, jobs, AgentRuns, budgets, model selection, permissions, sandbox, or user steering state.
- WorkPlan/Goal/Todo revisions are captured and revalidated before an epoch handoff becomes active.
- At most one current CodeGG continuation/handoff block is visible in the new provider context.
- System/runtime prompt provenance remains canonical; model-generated summaries cannot replace immutable instructions.
- Tool call/result pairing and provider message chronology are valid in either normal compaction or fresh-epoch path.
- Epoch reset is optional/profile-aware, never an unconditional per-N-turn loop.

## 5. Scope

### In scope

- additive WorkPlan ID/revision fields/projection in continuation snapshot/checkpoint;
- source-revision revalidation at rollover/epoch install;
- `ContextEpochPolicy` or equivalent resolved policy;
- fresh provider request-context reconstruction from canonical state;
- safe triggers: verified phase boundary, repeated-compaction threshold, explicit operator/host trigger, or model-profile policy;
- diagnostics/events/metrics;
- regression tests across restart and repeated compaction.

### Explicitly out of scope

- changing token accounting/compaction algorithms unnecessarily;
- clearing persisted message history;
- provider-private context-storage APIs as a requirement;
- automatic epoch reset for all models;
- a separate memory service;
- WorkPlan semantics already owned by M002/M003.

## 6. Required production changes

### Core/domain

Extend the continuation payload with bounded WorkPlan provenance, for example:

```text
work_plan:
  id
  revision
  status
  current_phase/current_item
  bounded actionable/blocked summary
  source digest
```

Do not embed the entire plan. Exact detail remains available through WorkPlan tools/recovery handles.

If checkpoint schema versioning is needed, add an additive version with backward rendering for prior installed checkpoints.

### Context policy

Define a resolved epoch-reset policy separate from the compaction trigger. Candidate inputs:

- model profile family/prompt profile/context characteristics;
- number of successful rollovers since last fresh epoch;
- verified WorkPlan phase completion;
- repeated no-progress/replan signal when a clean context is an allowed recovery action;
- explicit command/config override.

Default should be conservative/disabled unless evidence/model profile supports reset. A policy decision must be deterministic from host state; the model does not invoke an unrestricted “forget context” tool.

### Runtime reconstruction

At a safe turn boundary, create a new provider-visible message sequence from:

1. canonical compiled system/developer/runtime instructions;
2. immutable session/user objective provenance;
3. latest Goal projection if active;
4. current WorkPlan projection/revision;
5. bounded Todo projection;
6. installed continuation semantic/deterministic state and exact recovery handles;
7. bounded latest user steering/control spine;
8. any provider-required compatibility messages.

Do not copy historical stale compaction frames or hidden reasoning.

### Transactionality

Reuse rollover source capture/revalidation. An epoch candidate must not activate if Goal/WorkPlan/Todo revisions changed after preparation. Abort/rebuild rather than install stale next action.

Durable session history remains unchanged; epoch identity may be a bounded diagnostic/continuation lineage field/event, not a second transcript table.

### Protocol/frontends

Expose optional event/diagnostic `context_epoch_started` with reason, checkpoint/workplan revision, prior compaction count, and provider/model profile identifier. Do not expose sensitive payload contents.

### Security and authorization

Epoch reset preserves the exact execution-policy snapshot/model-selection authority for the next turn according to their own owners. It cannot bypass an approval or replay a denied action.

### Documentation/static guards

Update context-compaction ownership docs to describe this as a consumer/path of the existing owner. Add a guard/test preventing introduction of a second context history/compaction engine if useful.

## 7. Ordered work packages

### Work package A — Checkpoint WorkPlan provenance

Add ID/revision/projection, bounded serialization, migration/version compatibility, and source revalidation.

Acceptance evidence: checkpoint restores the exact active WorkPlan revision and rejects stale install.

### Work package B — Epoch policy

Implement typed decision/reason codes and model-profile/config integration. Keep disabled/conservative default unless existing profile evidence warrants otherwise.

Acceptance evidence: same state gives same decision; unsupported profiles remain on normal compaction.

### Work package C — Fresh-context reconstruction

Build provider-visible sequence through existing prompt/context-plan owners. Verify one continuation block, later user steering, tool contracts, and Goal/WorkPlan current state.

Acceptance evidence: provider receives no stale duplicate compacted frame and can continue current WorkItem immediately.

### Work package D — Restart/diagnostics

Persist/derive enough lineage to resume after daemon restart and explain why a fresh epoch occurred.

## 8. Failure, cancellation, restart, and contention semantics

- Candidate preparation/reconstruction failure leaves current provider history/context strategy unchanged; it does not destroy existing state.
- If provider/model cannot accept the reconstructed control-message form, use normal compaction for that profile and emit a bounded reason.
- Revision drift before activation aborts/rebuilds candidate.
- User cancellation/steering during preparation prevents stale activation.
- Restart loads only installed checkpoint/authoritative WorkPlan; no prepared epoch candidate becomes authority.
- Missing optional artifact handles degrade to summaries/diagnostics as existing context-continuity rules specify.

## 9. Compatibility and migration

- Prior continuation checkpoints remain readable.
- Existing compaction behavior remains default-compatible when epoch reset is disabled/not selected.
- Existing model profiles with no epoch field inherit conservative behavior.
- No session/history API breaking change.

## 10. Required tests

### Focused unit tests

- epoch policy decision/reason matrix;
- WorkPlan checkpoint projection bounds;
- revision revalidation;
- fresh-context message construction and single-frame invariant.

### Integration tests

- phase boundary -> fresh epoch -> continue next WorkItem;
- repeated compactions trigger policy only for configured/profile-supported model;
- recent user steering after old checkpoint is preserved;
- normal compaction remains functional when reset disabled.

### Restart and recovery tests

- restart before candidate activation;
- restart after fresh epoch event/checkpoint install;
- newer WorkPlan revision overrides stale checkpoint next action.

### Contention and cancellation tests

- WorkPlan/Todo/Goal update races candidate install;
- steering/cancel during epoch preparation.

### Security and negative tests

- permission/sandbox/model preference unchanged by reset;
- hidden reasoning absent;
- no cross-session recovery handle exposure.

### Migration and compatibility tests

- old checkpoint version renders safely;
- old model profile/config behaves unchanged.

## 11. Required verification commands

```bash
cargo test -p codegg-core -- continuation
cargo test --test agent_loop_harness -- compaction
cargo test --test agent_loop_harness -- work_plan
python3 scripts/check_core_boundary.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Use current focused equivalents where necessary.

## 12. Documentation updates

- `architecture/context-compaction-ownership.md`
- `architecture/compaction.md`
- `architecture/work_plan.md`
- `architecture/goal.md`
- model-profile documentation/config schema.

## 13. Acceptance criteria

- continuation checkpoint carries/revalidates WorkPlan identity/revision without embedding full plan;
- a supported policy can start a clean provider-visible epoch from authoritative host state;
- objective, current item, next action, Goal/Todo state, user steering and recovery handles survive;
- no durable history/workspace/authority state is reset;
- stale revisions cannot activate;
- normal compaction remains the canonical fallback/default path.

## 14. Stop conditions

Stop if implementation requires a second transcript store, provider-specific hidden memory as the canonical path, destructive history rewrite, model-initiated authority change, or reopening the closed compaction owner.

## 15. Closure evidence required

- implementation commits;
- checkpoint schema/version evidence;
- policy decision matrix;
- fresh-context before/after projection fixture;
- restart/revision-race tests;
- proof existing compaction path remains canonical;
- exact verification commands and residual limitations.

## 16. Handoff notes

The value of this milestone is clean reconstruction, not frequent resets. Prefer phase boundaries and measured/profile-supported triggers. Do not introduce a magic numeric reset cadence as the only policy.
