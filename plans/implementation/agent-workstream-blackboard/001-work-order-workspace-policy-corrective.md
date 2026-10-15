# Agent Workstream Blackboard M001 — WorkOrder Workspace-Policy Authority Corrective

Status: ready — safety corrective; no production change landed by this plan

Planning baseline: de49884f3fdc6d528620f2a2645694b9b19fc087 (2026-10-15)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md#milestones--deliverables, M001
Applicable accepted ADRs: ADR-0002, ADR-0004, ADR-0005; proposed ADR-0032 is NOT required to fix existing WorkOrders.
Canonical: plans/000-long-term-specification.md §§9, 16, 17, 29; plans/001-terminology-and-domain-model.md §§5, 7–8; plans/003-planning-process.md §7.
Primary class: invariant / corrective safety.
Historical source and closures (do not rewrite): plans/implementation/project-work-orders-task-view/002-release-coordinator-session-materialization.md; plans/closure/project-work-orders-task-view/002-status.md and /007-status.md. Reverify exact historical path names before linking final closure.

## Objective and discovered defects

Restore truthful mutation classification for WorkOrders independent of requested checkout placement. Current src/core/work_order_coordinator.rs: is_mutation = !matches!(workspace_policy, Some(WorkspacePolicy::Shared)); that makes a selected Shared policy force an apparently read-only path although the submitted build agent remains capable of writing. The resolver in crates/codegg-core/src/work_order/coordinator.rs maps (Shared, nonmutating) to ShareReadOnly. A separate Serialized path sets the literal scheduler key exclusive:workspace-mutation, apparently serializing unrelated workspaces. These were outside the emphasis of the original M002/M007 materialization and trajectory qualification; write a precise reproducer before making changes.

## Non-goals / invariants

Do not change WorkOrder identity/gates/repeats, add a blackboard table, permit concurrent shared mutation, grant new tools, bypass the daemon scheduler, alter already closed evidence, or change AutoIsolated defaults. Serialized means a verifiable exclusive scope, not a UI label. Non-Git mutation without an isolation backend must use explicitly authorized serialized execution or fail with NeedsAttention. Preserve cross-principal authorization and child sandbox ceilings.

## Implementation packages

A. Evidence-first reproduction. Inspect WorkOrder::workspace_policy validation, resolve_workspace_action(), runtime resolution, resource requests, scheduler lock semantics, and all paths that submit AgentTurn jobs. Add a regression that creates a mutation-capable build work order with Shared and observes actual resolved action and resource keys. Verify whether resources can be bypassed by sibling normal AgentTurn/JobKind executions before deciding the truthful behavior.

B. Separate authority and placement. Introduce an effective mutation-capability signal based on resolved agent/tools/sandbox and command surface; if that signal cannot be known at release time, conservatively classify as mutation-capable. Never infer it from Shared. Make selection of Shared mutation either explicitly serialized under an enforceable policy or NeedsAttention with actionable reason. Preserve read-only sharing.

C. Canonical contention identity. Derive a typed/scoped exclusive key from canonical WorkspaceId or repository checkout identity, not path string or literal global singleton. Ensure two writers to the same checkout, including across different sessions, contend on the SAME key; unrelated checkouts do not. If existing global resource profiles bypass the scoped key, close that path or disallow shared mutation instead of claiming protection.

D. Failure semantics and docs. Prove no worktree is silently allocated when Shared/Serialized was selected; unavailable isolation records bounded attention and no tool execution. Reconcile architecture/worktree.md, architecture/scheduler.md, WorkOrder docs/projections with what is actually enforced. Do not update long-term source merely to match a bug.

## Required tests and evidence

- Shared write-capable job never takes ShareReadOnly; read-only job can share.
- Two write-capable WorkOrders sharing a checkout do not execute mutation concurrently; unrelated checkouts can run concurrently.
- Agent/permission/sandbox changes between authoring and materialization fail closed instead of widening privileges.
- AutoIsolated Git mutation remains leased; non-Git mutation without viable serialized backend gives attention; no release-gate/idempotency/restart regression.
- Cancellation while awaiting exclusivity releases admission safely; daemon restart does not lose ownership or issue a second mutation.
- Current focused suites: cargo test -p codegg-core work_order; cargo test --test work_orders_m002_materialization; cargo test --test work_orders_m007_trajectory; scheduler contention tests; cargo fmt --all -- --check; cargo clippy --workspace --all-targets --locked -- -D warnings; scripts/verify.sh quick; python3 scripts/check_scheduler_bypass.py; python3 scripts/check_execution_ownership.py. Record executed, skipped or unavailable commands accurately.

## Acceptance and stop conditions

Close only with a reproducing pre-fix failure, a post-fix canonical-identity contention test, a project-isolation negative test, exact source/commit evidence and no false read-only claims. If the actual generic AgentTurn writer can escape a scheduler-only reservation, disallow Shared mutation and file a bounded follow-up instead of claiming safe concurrency. No acceptance of ADR-0032 is implied.

## Closure record

Write plans/closure/agent-workstream-blackboard/001-status.md with original-closure references, defect root cause, why old tests missed it, requirement/evidence matrix, exact commands, security/restart checks, residuals, commit and downstream unblock audit. M005 remains blocked on M004 and proposed ADR-0032 irrespective of M001 closure.
