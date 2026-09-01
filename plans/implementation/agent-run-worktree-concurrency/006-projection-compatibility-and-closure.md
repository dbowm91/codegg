# Agent Run, Async Delegation, and Worktree Concurrency Milestone 006 — Projection, Compatibility, and Closure

Status: implemented

Repository baseline: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`

Source roadmap:

- `plans/subsystems/agent-run-worktree-concurrency-roadmap.md#m006--projection-compatibility-simplification-and-strict-closure`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#18-worktree-native-concurrency`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`
- `plans/002-long-term-roadmap.md#phase-10--worktree-native-concurrency`

Applicable ADRs:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

Primary class: capability/polish/invariant

Hard blockers: M001-M005 must close.

## 1. Objective

Finish the workstream by projecting authoritative durable run/worktree state through existing session/native frontend contracts, reconciling the model-facing compatibility surface, removing redundant daemon subagent admission/polling paths where they are no longer needed, and gathering strict closure evidence without expanding CodeGG’s verification apparatus.

This milestone is the only one allowed to claim the combined async-delegation/worktree capability closed.

## 2. Why this milestone becomes ready after M001-M005

The predecessor milestones establish all authoritative execution contracts:

- M001: durable task/run identity and scheduler ownership;
- M002: mailbox/journal/status/wait/cancel/notification;
- M003: durable worktree service/leases/restart recovery;
- M004: automatic mutation isolation, child commits, structured results, explicit integration;
- M005: run groups/join policies/background scheduler handles.

The existing session projection subsystem already provides version/capability negotiation, bounded snapshots/events, replay/cursors, visibility classes, deterministic reducers, resync, and TUI/native transport integration. This milestone should extend that system additively rather than invent a second frontend state model.

## 3. Current implementation evidence

Before editing, confirm the post-M005 repository state and compare it to the original baseline:

- authoritative run/worktree/mailbox/group stores exist and have restart semantics;
- legacy `SubAgentTask`, `TaskStore`, `SubAgentRequest`, `SubAgentPool`, `Subagent*` events, numeric task IDs, and `task get` polling may still exist as compatibility adapters;
- session projection still carries older bounded subagent summaries and separate job/run concepts;
- TUI/sidebar may show worktree facts but not durable run ownership/result commit/group state;
- scheduler/execution ownership static guards may still classify old pool send sites as compatibility or definition paths;
- architecture docs may describe both old and new authority paths.

Do not assume a compatibility type is dead merely because a new service exists; prove production reachability before deleting it.

## 4. Invariants that must not regress

- Authoritative run/worktree execution state remains in domain stores/services; projection is derived.
- Daemon production has one scheduler admission/resource authority for child/background execution.
- Frontend reconnect/replay cannot mutate execution or create duplicate controls.
- Projection payloads remain bounded, redacted, versioned, and visibility-classified.
- Hidden reasoning, credentials, full mailboxes, unbounded diffs/logs, and internal authority bodies are not projected.
- Compatibility removal must not break standalone mode or supported older clients without an explicit migration window.
- Worktree cleanup/ownership and child commit/integration semantics from M003/M004 cannot be weakened for simpler UI.
- Routine CI remains deliberately minimal.

## 5. Scope

### In scope

- additive projection DTOs/events for durable agent task/run tree, worktree ID/branch/base/result commit/health summary, group state/join policy, structured terminal result summary, attention-required state, and control/cancellation status;
- adapters from authoritative run journal/store/worktree/group transitions into projection events;
- snapshot/replay/resync tests across reconnect and daemon restart;
- TUI/native frontend rendering/navigation sufficient to inspect parent/child runs, statuses, worktree/branch, validation/result summary, and failed/orphaned attention state;
- ACP compatibility where the current ACP/session projection surface automatically consumes the new fields/events; avoid ACP-specific redesign unless required;
- model-facing task-tool compatibility cleanup: retain stable aliases where needed, prefer durable run IDs, reduce repeated polling guidance now that push/wait exists;
- prove and remove redundant daemon subagent machine-capacity admission/semaphore paths after scheduler ownership is established;
- reduce legacy `TaskStore`/numeric ID/event adapters to the smallest documented compatibility surface, deleting dead dual-write or duplicate-state code;
- architecture/docs/static-guard reconciliation;
- final production-shaped failure/restart/contention/security review and closure record.

### Explicitly out of scope

- team roles/project chat/presence;
- remote execution or distributed agent trees;
- arbitrary rewind/history editing;
- new workflow language;
- UI polish unrelated to run/worktree observability;
- new CI lanes, fuzz farms, benchmark/coverage/binary-size gates, release automation, or scheduled workflows.

## 6. Required production changes

### Projection contract

Extend the existing projection protocol additively. Prefer a normalized bounded summary such as:

```text
AgentRunSummary
  run_id
  task_id
  parent_run_id
  agent
  status
  depth
  worktree_id?
  branch?
  base_commit?
  result_commit?
  validation_summary?
  group_id?
  attention_required
  terminal_summary?
```

Keep large result/diff/test data behind existing artifact/run handles. Add events for run upsert/progress/terminal/control-attention and worktree/group changes only as required for deterministic reducer state.

If old `SubagentStarted/Progress/Completed/Failed` projection events remain for compatibility, derive them from the same authoritative transitions and document removal criteria. Do not maintain a second mutable subagent tree.

### TUI/frontend

Provide compact inspectability, not a workflow IDE rewrite. At minimum:

- run tree/child list with status;
- selected child detail or existing subagent panel updated to durable run identity;
- worktree/branch/result commit and validation summary;
- clear queued/running/waiting/failed/cancelled/completed/attention states;
- indication when a failed/dirty worktree is retained and how to inspect/clean it;
- group progress summary where M005 exists.

Existing keyboard navigation may be reused; avoid broad new keymaps unless necessary.

### Compatibility convergence

Inventory all production uses of:

- `SubAgentPool` direct sends;
- pool semaphore/admission registry;
- `TaskStore` writes/reads;
- numeric task ID generation/hash aliases;
- legacy subagent event production;
- standalone compatibility constructors.

For each, classify as:

1. canonical new path;
2. necessary standalone/older-client compatibility;
3. test fixture;
4. dead/redundant and removable.

Delete category 4. Narrow category 2 with explicit comments/guards. Daemon production must not perform scheduler admission and then fail on a redundant machine-capacity semaphore in the old pool.

### Static ownership guards

Update existing `check_scheduler_bypass.py`, `check_execution_ownership.py`, or machine-readable execution manifest only where ownership has genuinely moved. Prefer deleting obsolete exemptions over adding new scripts.

Add a narrow guard/test only if a known high-risk regression cannot be expressed through existing ownership checks—for example, direct daemon `SubAgentPool::send` bypassing `AgentRunService`/scheduler.

### Documentation

Reconcile:

- `architecture/agent.md`;
- `architecture/scheduler.md`;
- `architecture/worktree.md`;
- `architecture/git.md`;
- `architecture/projection.md`;
- task/tool and TUI docs;
- planning roadmap/registry/closure record.

Document the final simple mental model:

```text
spawn -> durable AgentRun -> scheduler
                       -> mailbox/journal
                       -> optional owned worktree
                       -> structured result
                       -> explicit integration
```

## 7. Ordered work packages

### A — Authority inventory before deletion

Map post-M005 production call graph and create a table of legacy paths with keep/delete classification.

Acceptance evidence:

- every direct pool send/task-store write/event producer is accounted for;
- no deletion is based only on naming/search without runtime ownership analysis.

### B — Projection DTO/events/reducer

Add bounded durable run/worktree/group summaries and deterministic reducer transitions.

Acceptance evidence:

- snapshot + replay reconstructs equivalent state;
- unknown/additive compatibility behavior remains correct;
- secret/hidden-reasoning disclosure tests remain green.

### C — TUI/native consumer

Adapt existing subagent/run/worktree surfaces to authoritative IDs/status/results.

Acceptance evidence:

- user can inspect concurrent child statuses and worktree/result commit without reading logs;
- reconnect/resync preserves tree state.

### D — Compatibility simplification

Remove redundant daemon pool admission/dual-state/polling code proven unused; retain narrow standalone/legacy adapters.

Acceptance evidence:

- scheduler is sole daemon machine-resource admission owner;
- existing first-level task compatibility fixtures remain green;
- standalone path remains explicitly marked outside daemon guarantees.

### E — Failure/restart/contention closure review

Run production-shaped scenarios spanning all milestones and fix only defects within this roadmap.

Required scenarios:

- multiple concurrent read-only and mutating children;
- overlapping edits isolated in separate worktrees;
- message/interrupt/cancel races;
- daemon restart with active runs/groups and dirty worktrees;
- child commit + validation + explicit integration success/conflict;
- detached/background completion after parent turn ends;
- frontend disconnect/reconnect and projection replay.

### F — Final docs/registry/closure

Create `plans/closure/agent-run-worktree-concurrency/006-status.md` with exact evidence. Mark milestones/roadmap/registry consistently only after acceptance criteria are met.

## 8. Failure, cancellation, restart, and contention semantics

Closure must demonstrate rather than redesign the semantics established earlier:

- queued/running cancellation is downward and first-terminal-wins;
- control delivery is persist-before-signal and restart-safe;
- stable journal replay never repeats completed non-idempotent effects;
- worktree reconciliation is conservative and preserves dirty/conflicted state;
- child commits persist across restart and are never recreated blindly;
- integration conflicts remain explicit typed Git state;
- group joins/detached/background jobs remain durable and bounded;
- projection lag/resync never mutates authority or duplicates control delivery;
- scheduler fairness/resources remain global across separate worktrees.

Any violation is a correctness defect, not documentation-only closure work.

## 9. Compatibility and migration

- Keep compatibility aliases/events only with a named removal criterion and no duplicated authority.
- Older persisted legacy tasks remain readable as legacy history; do not fabricate run/worktree provenance.
- Older clients may continue receiving existing fields/events during the negotiated compatibility window.
- New clients should prefer typed task/run/worktree IDs and structured result handles.
- Standalone mode may keep local adapters but must not be described as daemon-durable/restart-safe unless it actually is.

## 10. Required tests

### Focused unit tests

- projection DTO bounds/visibility;
- reducer run/worktree/group transitions;
- legacy adapter mapping;
- compatibility removal guards.

### Integration tests

- session projection from spawn through terminal result/integration;
- TUI state from snapshot + incremental replay;
- multiple concurrent children and groups;
- old task get/event compatibility where retained.

### Restart and recovery tests

- active child/group/worktree across daemon restart;
- terminal child before parent consumption;
- dirty orphan worktree attention state;
- projection reconnect/resync after restart.

### Contention and cancellation tests

- multiple projects/roots contending globally;
- cancel root/group/child while queued/running;
- no duplicate pool admission failure after scheduler admission.

### Security and negative tests

- projection redaction/disclosure;
- unrelated run/worktree control denial;
- child path/Git authority regression;
- unmanaged worktree cleanup refusal;
- stale compatibility IDs cannot target unrelated durable runs.

## 11. Required verification commands

Use focused subsystem tests first. Final closure should remain minimal and evidence-driven, for example:

```bash
cargo test -p codegg-protocol
cargo test --lib agent
cargo test --lib scheduler
cargo test --test session_projection_consumer
cargo test --test scheduler_restart_recovery
cargo test --test scheduler_contention
cargo test --test scheduler_cancellation
cargo test --test worktree
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
./scripts/verify.sh quick
```

Adjust to the then-current repository commands and run only relevant focused tests plus one documented quick broad pass. Do not add new CI jobs, matrices, scanners, coverage/benchmark/size gates, dependency bots, workflow dispatch, or release automation.

## 12. Documentation updates

- all architecture docs named in section 6;
- task/delegation model-facing contract;
- TUI/user-facing run/worktree inspection docs;
- subsystem roadmap milestone table;
- `plans/registry.md`;
- closure record.

## 13. Acceptance criteria

1. Authoritative durable run/worktree/group state is projected additively through the canonical session projection system.
2. Frontend reconnect/replay reconstructs the same child tree/worktree/result state.
3. TUI/native clients can inspect child status, worktree/branch, result commit/validation, and attention-required state.
4. Daemon production has one scheduler admission/resource authority; redundant pool machine-capacity admission is removed or provably non-production.
5. Legacy task/pool/event adapters are reduced to a documented compatibility boundary with no dual authority.
6. Concurrent mutating children remain worktree-isolated and child commit/integration policies remain intact.
7. Parent/child control, cancellation, restart, groups, and background handles remain deterministic under production-shaped closure tests.
8. Dirty/conflicted worktrees survive failure/restart until safe cleanup.
9. Projection/security tests show no hidden reasoning/secret/unbounded output disclosure.
10. No critical/high/medium unresolved finding remains in this roadmap’s correctness, security, recovery, contention, leak, or compatibility scope.
11. Existing minimal verification posture remains unchanged except for focused tests/guard updates required by moved ownership.
12. Roadmap, implementation plans, architecture docs, registry, and closure record agree.

## 14. Stop conditions

Stop and require a corrective plan if:

- any predecessor milestone is only conditionally closed on a correctness issue relevant here;
- closing requires hiding a known restart/cancellation/worktree leak defect;
- deleting compatibility code breaks supported standalone/older-client behavior without a migration decision;
- projection would need to become an execution authority;
- verification scope begins expanding into unrelated CI/release hardening;
- a high/medium finding remains after the bounded corrective pass.

## 15. Closure evidence required

The closure record must include:

- implementation and reviewed-head commits for M001-M006;
- milestone-by-milestone closure references;
- final architecture ownership map;
- production-path proof of one scheduler admission authority;
- concurrent child/worktree/commit/integration evidence;
- mailbox/cancel/restart/group/background-handle evidence;
- projection reconnect/resync evidence;
- legacy compatibility inventory and deletions retained/removed;
- exact focused tests/guards/quick verification outcomes;
- unresolved findings by severity;
- final recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

This is a closure/simplification milestone, not an invitation to add more orchestration features. Prefer deleting adapters and reconciling one authority over adding abstractions. Keep verification proportionate to the project: focused adversarial tests for the concurrency/restart boundaries plus the existing quick broad check are sufficient if green.
