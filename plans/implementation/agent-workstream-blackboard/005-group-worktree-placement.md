# Agent Workstream Blackboard M005 — Explicit Group-Owned Worktree Placement and Serialized Shared Mutation

Status: blocked — M001 and M004 strict closure AND ADR-0032 explicit acceptance

Planning baseline: de49884f3fdc6d528620f2a2645694b9b19fc087 (2026-10-15)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M005
Hard decision gate: plans/adrs/ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md is PROPOSED, not accepted. ADR-0002 remains controlling until accepted supersession of its per-concurrent-mutating-child rule.
Canonical: plans/000-long-term-specification.md §§9, 16–17, 27, 29; plans/001-terminology-and-domain-model.md §§5, 7–8.
Primary class: capability / invariant. No work may enable shared mutation before the decision and safety gates close.

## Objective

Allow explicit, authorized workspace placement for task spawn and spawn_many without equating agent concurrency with filesystem concurrency. Preserve current default per-mutating-child isolated worktree, read-only inherited checkout, and daemon-owned lease safety. Introduce an opt-in serialized shared checkout for a bounded group of mutation-capable children only where every write path can be governed; no concurrent unreserved writes.

## Existing production boundary

SubAgentPool::requires_isolation in src/agent/worker.rs identifies write-capable tool surfaces; src/scheduler/executors.rs allocates WorktreeService::CreateWorktreeRequest and replaces child workspace_root/allowed_paths with owned checkout. WorktreeService currently creates an owner_run_id lease with one active owner. src/tool/task.rs spawn_many permits at most 16 children and has no placement choice. WorkOrders have a distinct WorkspacePolicy corrected in M001. Nested allocation uses canonical common Git root and explicitly selected parent/base state.

## Work packages

A. Consolidate placement resolution. Define one core domain policy (avoid duplicating WorkOrder WorkspacePolicy and task-spawn policy semantics): Auto, IsolatedNew, InheritReadOnly, and SerializedShared (group or exact worktree ID). Separate selected placement from effective mutation capability and sandbox/approval ceiling. Auto remains existing M004-era isolated default for write-capable delegated runs. Group placement is an explicit field on spawn_many and inherited by nested children only within authorized ceilings; single spawn may opt in if supported. Reject unsupported placement with typed diagnostic, never silently share or weaken capabilities.

B. Separate checkout ownership from active execution access. Worktree owner may be a group/session/work order under bounded reservation, but individual execution permits remain run-owned/generation-fenced. Do not turn a worktree lease into a bearer token. Establish typed scope for ProjectId/RepositoryId/WorkspaceId/WorktreeId/group/run; align persistence, restart reconciliation and safe cleanup. A group terminal does not force-remove dirty or conflicted checkout. External manually created worktrees are never silently claimed.

C. Implement defensible serialized mutation. Shared group members may analyze/read concurrently. All filesystem-mutating actions, Bash/terminal/process execution that can mutate, Git index/branch/commit, LSP edits, tools/plugins, Tool Programs and Python execution require a common checkout-scoped writer reservation held for the effective mutation lifetime. Disallow escape paths or unsupported backends; for arbitrary shells whose write extent cannot be proved safe, reserve the entire process lifetime or fail closed. Enforce this on daemon/tool execution boundary, not on a prompt hint or regex command classifier. Ensure approval waits do not hold incompatible locks indefinitely; define ordering vs scheduler/Git repository locks to avoid deadlock. Claim/path intents from M003 are advisory and cannot replace this writer reservation.

D. Git changes/integration. One group checkout has one coherent Git index/branch; commits must be serialized and attributed to active run or designated group owner. Explicit parent integration remains authorized and typed; no child push/rebase/merge by default. Qualify nested parent checkout, same-repository base/HEAD, successive child-result integration on an advanced target, conflicts/recovery and cancelled writers. If typed integration cannot prove a moving parent safe, reject with actionable recovery, not silent rewrites.

E. Negative platform and remote behavior. Check Linux/macOS/Windows effective backends independently; do not treat a scheduler key as OS filesystem containment. Non-Git mutable work cannot claim Git worktree isolation. Cross-node/shared directory and network filesystem modes are out of scope. Deny SerializedShared if any active tool path bypasses the common guard or is outside the effective sandbox.

## Acceptance tests

- Two write-capable members use one group checkout with exclusive writer operations; two isolated members still receive distinct managed worktrees; read-only inherited member cannot write.
- Two contending writers: precisely one runs, second waits or receives typed conflict; cancellation, restart, process kill, permission wait, exception and stale lease never admit two writers.
- Tools outside ordinary file edit paths (shell, terminal, Git, plugin, Python, LSP, Tool Program, native process) cannot bypass the effective guard; adversarial renamed files, symlink escapes, overlapping path claims and foreign-process edits treated conservatively.
- Separate repository/workspace groups do not accidentally serialize one another; another principal/group cannot acquire an active group lease merely by knowing IDs.
- Descendant inherits only narrowed authority and placement ceilings, never arbitrary group membership or Git push/force.
- Dirty/conflicted cleanup retained; branch and untracked data never destroyed for apparent success.
- Existing SubAgentExecutor auto-isolation and M001 WorkOrder behavior regressions remain green; exact hosted platform evidence where claimed.

## Verification / stop conditions

Run focused worktree, scheduler contention/restart, task/run control, session authorization, agent integration, Git security and process execution tests, scripts/verify.sh quick, Clippy, source guards for scheduler/exec and sandbox, and at least one production-shaped multi-run shared-mutation trajectory under supported native containment. Mark unsupported platforms explicitly; no 'supported' labels based only on mocked reservations.

Stop this milestone if accepted ADR is missing, if any mutation surface can evade the reservation, if the implementation requires a second scheduler, or if arbitrary shell requires weakening a sandbox. In that case retain Auto isolated, report a corrective or rejected feasibility record; M006 and shared build policy remain blocked rather than pretending the feature works.

## Closure

plans/closure/agent-workstream-blackboard/005-status.md must include accepted-ADR identity and approval, pre/post architecture, mutex/lease state machine, negative bypass and race matrix, native containment evidence by platform, nested/advancing integration results, performance cost, exact commits/commands, residual risks and unblock audit.
