# Agent Runtime Correctness — Host-Owned Goal Verification Addendum

Status: closing — M012 implementation landed; closure evidence is being recorded

Repository baseline reviewed: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md`

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `architecture/goal.md`
- `architecture/jobs.md`
- `architecture/agent.md`

Applicable ADRs:

- None required for the scoped work. Existing daemon, scheduler, tool-authority, and runtime-asset decisions remain authoritative.

## 1. Purpose and ownership boundary

This addendum closes one remaining goal-runtime correctness gap without reopening the previously closed agent-runtime simplification work: the model that performed a goal can currently request completion and cause the goal to transition directly to `Complete` from model-supplied evidence.

The goal state machine, not the working model and not a plugin, must own the terminal completion decision.

This work owns:

- a host-owned goal-completion proposal and verification contract;
- deterministic derivation of available completion evidence from CodeGG-owned state;
- a typed verification verdict that either permits completion or returns bounded unmet criteria/evidence gaps;
- integration with the existing autonomous-goal continuation path when verification says more work remains;
- durable/auditable provenance sufficient to relate scheduler jobs and delegated runs to the active goal without creating a second scheduler authority.

It consumes but does not redefine:

- the existing goal store/status model and continuation budget logic;
- scheduler `JobRecord`/attempt authority;
- durable delegated `AgentRunRecord` ownership;
- tool execution/result records, Git/workspace state, todos, snapshots, and other existing evidence sources;
- plugin hooks and custom tools as optional evidence producers only.

## 2. Invariants

- A model request to finish a goal is a proposal, never the authoritative terminal transition.
- `GoalStatus::Complete` is reached only after a host-owned verifier accepts the proposal.
- Model-provided `files_changed`, `tests_run`, evidence prose, or remaining-risk prose are claims; CodeGG-owned records remain authoritative when corresponding records exist.
- A verifier cannot convert a failed/absent required deterministic check into success.
- Goal verification must not execute mutating tools or broaden tool authority.
- Plugins may contribute bounded evidence but must not become required for the correctness of core goal completion.
- Uninstalling, disabling, timing out, or crashing a plugin cannot silently make completion easier.
- Verification failure must not consume unbounded continuation turns or create a second autonomy loop.
- Existing goal budgets, cancellation, pause, and user-steering precedence remain intact.

## 3. Explicit non-goals

This addendum does not:

- introduce a general workflow engine;
- create a second agent scheduler or background execution service;
- require an LLM verifier in the first implementation;
- add plugin-specific goal-completion authority;
- redesign goal budgeting, todo semantics, run groups, worktrees, or the job state machine;
- require every natural-language completion criterion to become mechanically provable;
- add new CI lanes, coverage gates, or heavyweight verification infrastructure.

A later read-only semantic verifier may be considered only after deterministic host verification is closed and only for criteria that cannot be decided from structured evidence.

## 4. Current-state evidence

At the reviewed baseline:

- `src/tool/goal.rs` accepts model-supplied completion evidence, requires either tests or an explicit remaining-risk justification, and then directly calls `GoalStore::update_status(..., GoalStatus::Complete)`.
- `GoalCompleted` is currently described as a goal marked complete by the model.
- goal runtime already has terminal states, budgets, continuation decisions, checkpoints, and TUI/projection updates.
- scheduler jobs already carry session/turn identity, parent lineage, and durable labels; delegated runs carry scheduler job identity and durable run provenance.
- this means the missing boundary is certification, not another execution framework.

## 5. Target architecture

The canonical flow becomes:

```text
working agent
  -> goal_request_completion
  -> GoalCompletionProposal
  -> GoalVerificationService
       -> derive CodeGG-owned evidence
       -> evaluate deterministic gates and criteria
       -> GoalVerificationVerdict
            Met
            NotMet { unmet_criteria, evidence_gaps, next_action }
            AwaitingUser { reason }
  -> goal state transition / existing continuation path
```

The proposal may retain bounded model commentary for explanation, but verification must derive authoritative facts where available. Examples include:

- actual scheduler test/lint/build job outcomes associated with the goal/session/turn lineage;
- durable delegated-run terminal states;
- current todos/goal checkpoints;
- Git/workspace changed-file state;
- snapshot or tool-result evidence already retained by CodeGG.

For initial provenance, prefer a host-written goal identifier in existing bounded job/run metadata when that can be done without creating competing identity authority. If implementation evidence proves labels cannot safely support required audit/restart semantics, stop and promote the relation to an explicit typed field rather than parsing display IDs or model text.

## 6. Milestone

### M012 — Host-owned goal completion verification

Status: implemented

Plan:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/012-host-owned-goal-completion-verification.md`

Class: invariant

Dependencies:

- hard: existing closed goal accounting/autonomy/runtime correctness milestones;
- interface: existing scheduler/job, delegated-run, todo, Git/workspace, and tool-result stores.

Exit conditions:

- the model cannot directly set an active goal to `Complete`;
- deterministic evidence is derived from CodeGG-owned state where such state exists;
- failed or missing required checks produce `NotMet` rather than model-overridable success;
- a `NotMet` verdict returns bounded next-action information through the existing goal continuation path;
- cancellation, pause, budget limits, and user steering remain authoritative;
- plugin absence/failure does not weaken completion requirements;
- focused tests and the repository quick verification path pass.

Closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/012-status.md`

## 7. Security, restart, contention, and compatibility

Verification is read-only with respect to tools/workspace state until the goal store applies the resulting state transition. Concurrent completion proposals must be serialized or compare-and-set against the current goal revision/status so a stale proposal cannot complete a goal after pause/cancel/replacement.

Daemon restart must reconstruct the goal and its evidence from durable stores rather than relying on an in-memory verifier cache. Evidence that is inherently ephemeral must be treated as unavailable after restart unless already persisted by its owning subsystem.

Existing clients may continue receiving `GoalCompleted`; its semantics change from model certification to host-accepted completion. Avoid unnecessary protocol expansion unless a bounded verification summary is required for operator visibility.

## 8. Verification posture

Keep verification proportional:

- focused `goal` core/store/runtime tests;
- focused model-facing goal tool tests;
- integration tests for real job/run evidence correlation and stale/concurrent completion requests;
- one `scripts/verify.sh quick` pass after the milestone is coherent.

Do not add new hosted CI lanes or mandatory full-workspace verification solely for this addendum.

## 9. Deferred work

- optional read-only semantic/LLM criterion adjudication;
- plugin-contributed domain-specific verification checks beyond a small generic evidence seam;
- richer user-facing criterion visualization;
- autonomous goal scheduling across sessions.
