# Agent Workstream Blackboard M003 — Atomic Task Claims, Dependencies, and Ownership Handoff

Status: closed — strict closure recorded in `plans/closure/agent-workstream-blackboard/003-status.md`

Planning baseline: de49884f3fdc6d528620f2a2645694b9b19fc087 (2026-10-15)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M003
ADRs: accepted ADR-0002/0004/0005; ADR-0032 proposed (shared-write portion explicitly out of scope here).
Canonical: plans/000-long-term-specification.md §§6, 16, 17, 27–29; plans/001-terminology-and-domain-model.md §§5, 7–8.
Primary class: infrastructure and invariant. This milestone does NOT grant filesystem mutation privileges.

## Objective

Introduce a small durable, transactional intent layer: atomic task/responsibility claims, declared intended paths/interfaces, dependency edges, blockers and authorized handoff. It must prevent duplicate claim acceptance and stale ownership from being reported as current. This is not a general task tracker, BPM/workflow engine, project WorkOrder replacement, Git file lock, or permission engine.

## Baseline / boundary

M002 supplies an authenticated bounded workstream snapshot. AgentRunGroup provides membership and owner identity; WorkOrders, WorkPlan and Todo retain their existing meanings. Prefer minimal new tables in codegg-core's SQLite schema for claim/dependency data only; canonical AgentRun records remain the source for execution status.

## Implementation packages

A. Contract and migration. Define WorkstreamClaim with typed group/task/run/actor IDs, claim kind (task/path/interface), normalized bounded target, state (offered/claimed/released/blocked/completed), version, owner, timestamps and lease generation/deadline where applicable. Define dependency edges (source claim → prerequisite claim/run) with cycle rejection and capacity bounds. DDL with unique active ownership constraints and supporting bounded indexes; upgrade/reopen tests and reversible failure handling.

B. Atomic mutations. Create/claim/renew/release/handoff/block/unblock with CAS against expected revision/generation and optional idempotency key. Validate direct member/owner, project scope and actual run state on each mutation. Deterministic winner for simultaneous claims; fail with typed conflict and current owner rather than silently stealing. Reassignments are owner-mediated or require explicit authorization, not sibling control.

C. Overlap/namespace semantics. Exact task claims are authoritative for task identity. Path/interface intents are advisory until a later runtime enforcement capability exists. Detect simple overlapping normalized relative path globs and report conflicts; reject absolute paths, '..', symlink escape, case-fold collision on relevant filesystems, oversized wildcards, malicious Unicode labels and accidental whole-repo broad claims. Do not promise sound disjoint-file locking using globs. A path claim MUST NOT expand filesystem allowed_paths or Git privileges.

D. Lifecycle. A terminal/cancelled owner cannot renew; expiry and replacement are generation-fenced; restart finds current states and preserves uncompleted work without duplicating claims. Dependent work can become eligible when prerequisite commits/evidence are observed, but this milestone does not auto-submit new jobs.

E. Projection and audit. Publish bounded claim summaries and transition provenance via M002; keep raw descriptions redacted/length-limited. Preserve exact source/run IDs, sequence and idempotency. Provide an operator-authorized inspection path.

## Required tests

- 2–16 members contend on identical task, one winner; retried request idempotent, conflicting payload rejected.
- Stale handoff/renew/release after cancellation, lease expiry and reuse rejected; no ABA across restart and double completion.
- Dependencies cannot create cycles, self-edges, phantom IDs, unbounded fanout or cross-project references.
- Overlapping paths, prefix/traversal, linked worktrees, symlink/junction, case-folding and permission reductions are correctly rejected or explicitly advisory; no false claim of kernel-enforced safety.
- Unauthorized sibling assignment/steering cannot expand authority; team principals observe only authorized groups; secrets/agent prompts never stored in claim labels.
- Migration upgrade/reopen and concurrent transactional isolation; no busy polling/repeated model calls.
- Focused core/sqlite/concurrency tests, cargo fmt, core boundary, relevant authorization/audit/projection guards, scripts/verify.sh quick and workspace Clippy recorded.

## Compatibility and stop

No change to default isolated SubagentExecutor placement. Clients without blackboard support continue executing original group workflows. Stop if task claims require redefining WorkOrder/WorkPlan semantics, inventing a new admission queue, trusting filename hints as a sandbox, or making direct sibling chat/control. Do not call this shared-mutation protection.

## Acceptance / closure

A complete authorized, versioned claim lifecycle with restart-race evidence and a real M002 projection consumer; no 'claimed' status from stale actors. File plans/closure/agent-workstream-blackboard/003-status.md with migration/negative/loop tests, source SHAs, exact commands, unresolved findings and M004 unblocking audit.
