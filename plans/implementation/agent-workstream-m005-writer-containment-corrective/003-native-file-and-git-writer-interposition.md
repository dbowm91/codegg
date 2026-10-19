# M005 Writer-Containment Corrective C003 — Native File and Git Writer Interposition (Q2)

Status: blocked — C002 strict closure; accepted ADR-0042 required upstream
Primary class: invariant / capability foundation
Planning baseline: plan/agent-workstream-m005-writer-audit audit 2026-10-19; recheck current source before implementation.
Source: plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md C003
Original: plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md, Q2
ADRs: ADR-0002, ADR-0032 and **accepted ADR-0042 (option B, 2026-10-19)**. The ADR gate is satisfied upstream; C002 strict closure is not.

## Objective

Apply the C002 writer protocol at *final native effects*, proving that two distinct AgentRuns on one managed checkout cannot overlap checked file or typed Git mutations. Refusal must occur before a filesystem/index/ref effect. C003 must not enable Shared placement broadly: non-broker and process paths still require C004/C005.

## Work packages

A. Discover authoritative checkout writer scope at tool execution, from daemon ProjectBinding/AgentRun/WorktreeService data, not model-provided path or user-provided generation. Extend execution context with optional typed reservation/lease generation while retaining backward compatibility for old isolated runs. Ensure a forged path/worktree/group cannot mint a permit, and a symlinked/linked checkout resolves to the canonical WorktreeId before effect.

B. Instrument the final native file transactions (write/edit/replace/apply_patch and any LSP-preview application reachable from native tool routing) across ToolBroker and direct execution. Acquire C002 permit AFTER approval but BEFORE first mutation; hold it through atomic write/rename/rollback. ToolBatchExecutor's intra-batch overlap checks remain useful but must not substitute for cross-run permit. If a batch contains multiple effects, define permit granularity to prevent release between irreversible combined operations; avoid double acquiring in nested dispatch.

C. Instrument typed Git index/working tree/refs mutation, stage/commit, integration and recovery; worktree create/remove is a separate lifecycle owner and must reject a conflicting active group reservation or participate under approved higher-level mutation transaction. Acquire one canonical permit for checked operation, preserve typed Git authorization and repository-level conflicts across separate worktrees. No automatic merge. Do not hold checkout permit while awaiting child job or external approval; avoid Git lock vs permit inversion.

D. Make disabled/unknown native effect classes fail closed for any requested shared profile. Unsupported network/ref mutators never fall back to raw shell or a compatibility Git path. Maintain the previous isolated-worktree behavior without new expensive acquisition by default.

E. Add typed audit/journal entries and bounded status for acquire/effect/release/deny, with exact run/job/attempt attribution and redact private data; daemon restart must reconcile uncertain in-flight effects to quarantine rather than replay.

## Qualification matrix

Two real AgentRun/Job contexts, one Git checkout, barrier overlap native file vs native file, native file vs Git stage/commit, Git vs Git recovery; prove one active writer, second waits/typed-denied, readers concurrently proceed. Run distinct worktrees in parallel. Inject cancellation before first effect, during rename/rollback, after commit before release, stale lease/restart and invalid group membership. Include permission/sandbox narrowing and path alias/rename/symlink cases; Git status/index/branch/dirty/untracked preservation before/after.

Focused tests in native file tools/ToolBatchExecutor/GitExecution/WorktreeService, core daemon, authorization and integration. fmt, quick, Clippy, core boundary, scheduler and Git safety guards. Include a production-shaped temporary Git repo, not mocks alone.

## Stop / closure

Stop if an effect can occur before the permit or without group scope, if legacy Git APIs remain uncontrolled in the advertised profile, if nested broker acquisition deadlocks or repository lock ordering is inconsistent. No broad opt-in enablement yet. Closure at plans/closure/agent-workstream-m005-writer-containment-corrective/003-status.md; record surface-by-surface tests, exact SHAs, protocol changes, canceled effects, no bypass and C004 unblock audit.
