# Managed Agent Scratch M002 — Delegated Private and Explicitly Shared Scratch with Job Leases

Status: active — M001 strict closure recorded
Repository baseline: `c0d6ff2d324ee37005e828a5bf269feebfabff79` (revalidate after M001).
Source roadmap: `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`.
Long-term requirements: `plans/000-long-term-specification.md` §§4, 16–18, 26–29; `plans/001-terminology-and-domain-model.md` §§5–8; `plans/002-long-term-roadmap.md` Phases 9–10.
Applicable ADRs: ADR-0001, ADR-0002 and accepted ADR-0028; preserve delegated authority and worktree control.
Primary class: capability.

## 1. Objective

Create a distinct default private scratch resource for each delegated `AgentRun`. Allow a parent orchestrator to request named shared scratch resources and explicitly grant only selected owned children/jobs access by opaque `ScratchId`, without per-file tool calls or any permission expansion outside those grants. Keep scratch alive for admitted background jobs after the foreground turn ends.

## 2. Why not yet ready

M001 must first prove stable owner/provisioning and canonical model env/authorization. This milestone uses, not replaces, M001 manager. Existing durable run/job and parent/child authority interfaces are available.

## 3. Baseline evidence

`src/agent/worker.rs`, `src/tool/task.rs`, `crates/codegg-core/src/agent_run` represent child run/spawn identity; `src/scheduler/` and `JobSubmissionService` own background work and attempts. `architecture/agent.md` documents turn-owned root and run-owned nested delegation. `plans/subsystems/agent-workstream-blackboard-roadmap.md` owns collaborative awareness and worktree placement; no artifact exchange layer should be built there accidentally.

## 4. Invariants

No child may inherit scratch grants merely by knowing a path or by claiming a session/run ID; effective grants are subset of parent authority plus explicitly created child-private resource. Direct-parent authorization and scheduler permits remain authoritative. Separate child worktrees and `child_workspace_ceiling` still constrain mutations. No implicit shared `target/` cache; no global writable temp root or cross-session sharing.

## 5. Scope

In: typed shared-resource create/grant/revoke/reference through the existing orchestrator/subagent spawn surface, child-private allocation, bounded aliases passed in trusted prompt and child execution context, durable owner/consumer relationships, process/job attempt leases and refcount-safe reclamation. No new model-facing scratch tool.

Out: distributed scratch copy, remote node transports, arbitrary directory grant, cross-principal session sharing, task blackboard messages, build-cache dedupe, a second scheduler, shell command rewriting, sandbox backend creation.

## 6. Required production changes

**Core/domain:** attach immutable per-run scratch scope to accepted spawn; child-private `ScratchId` allocated idempotently under owning run or task invocation identity. Parent can create and name a shared resource under its session/run authority and provide a list of `ScratchId` grants (not literal paths). Resolve authorized children and enforce the owner-generation fence; deny unknown, expired, cross-session, stale or foreign parent resource IDs. Replays of the same accepted spawn must not allocate divergent private roots.

**Storage/protocol:** persist/derive resource owner, typed ID, explicit grantee and active job/attempt lease. If a new optional spawn DTO field is needed, default absent to private-only, validate size/count and reject forged caller/owner. Do not let client-provided raw paths or aliases generate storage paths; name collisions reject or deterministically resolve. Record migrations if needed.

**Runtime/env:** standard `TMPDIR`/`TMP`/`TEMP` and `CODEGG_SCRATCH_DIR` always refer to the child's **private** directory. Named shared roots are available through a bounded trusted manifest/prompt and deterministic env naming only after sanitizing aliases; never override private TMPDIR. Grant paths to structured tools and native sandbox per effective run/attempt; `Required` failing to apply grants must fail closed. Scope job attempts to the immutable accepted grant snapshot so later parent changes don't widen in-flight jobs.

**Leases/lifecycle:** shared resources hold owner and consumer leases; daemon/job attempt leases cover actual supervised process lifetime, retries and cancellations. Mark terminal only after process tree reap; on restart reconcile with durable JobStore/RunStore and run state using generation or ownership fencing. Session close must not remove scratch required by a running background job.

**Visibility:** provide orchestrator/child with only IDs, alias and paths they are authorized to access; blackboard may reference artifact handles later but is not modified in this milestone.

## 7. Ordered work packages

A. Trace accepted `task spawn`/run-group/standalone/daemon paths, direct-parent checks, and job attempt ownership; define a single typed scratch delegation DTO.
B. Implement private per-run creation, shared creation/ownership, idempotent grants, revocation and no-path-authority checks.
C. Carry immutable grant snapshot into model/tool/scheduler process env and backend allow roots; maintain private default temp.
D. Implement process/job/child leases, restart reconciliation, contention and safe release; document lifecycle and test shared-file handoff.

## 8. Failure/cancel/restart/contention

Simultaneous share creates with same idempotency key converge; conflicting alias/owner request fails. Cancellation revokes future admission but does not truncate already active processes. Retry on the same accepted job inherits the same authorized resource IDs, with a new attempt lease. After restart, do not trust unbounded stale PID or on-disk manifest as proof of an active lease; prefer terminal store state + helper/process ownership. A crashed daemon must conservatively retain ambiguous live artifacts. Revoke cannot be used to bypass explicit Deny or cause unauthorized access after new admission.

## 9. Compatibility/migration

Legacy child spawns without shared field receive only child-private scratch. `AgentRun`, task and job envelopes add optional/defaulted fields only; ensure restart decoding and persisted pending-job replay. Do not change external protocol unless required by a tested orchestrator path. Preserve existing run and worktree ownership.

## 10. Required tests

Two children distinct `TMPDIR` and paths; parent creates a shared ID, grants to A and B, excludes C; A writes fixture B reads it through actual tools, C is denied by structured tool and OS backend where supported. Path disclosure to C alone cannot grant access. Nested parent/foreign session and stale generation cannot share. Canceled/terminal child + active background compilation preserves shared files; cleanup after final release. Retry/double-spawn idempotency; crash-between-create/grant and durable-job-acceptance recovery; shared alias collision and unauthorized symlink escape. Test both sequential and concurrent orchestration, no Rust cache assumption.

## 11. Verification

```bash
cargo fmt --all -- --check
cargo test -p codegg-core
cargo test -p codegg --test sandbox_containment
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_sandbox_policy_wiring.py
bash scripts/check-core-boundary.sh
scripts/verify.sh quick
```

Add a focused real `task spawn` + admitted background job integration suite; run supported full workspace Clippy and existing agent-run/job integration tests. Native unsupported backends are reported explicitly.

## 12. Docs/guards

Update `architecture/agent.md`, `architecture/jobs.md`, `architecture/process-tool-execution-ownership.md`, `architecture/security.md`, and user-facing subagent/tool guidance. Update storage schema guard only if migration actually added.

## 13. Acceptance

An orchestrator creates a share once, assigns it to two children, exchanges a temporary artifact with real tools, proves a non-granted sibling receives no capability, and uses long-lived background jobs without prematurely erasing scratch. No new tool required. OS isolation claims backed by actual platform tests, not same-UID directory permissions alone.

## 14. Stop conditions

Do not bridge missing authorization with broad writable roots, shared host /tmp or global env. If existing delegated authority cannot gate create/grant robustly, record a blocker and propose a narrow ADR/contract decision. Do not force in-progress process cancellation on revoke without an agreed policy.

## 15. Closure evidence

`plans/closure/managed-agent-scratch/002-status.md`: accepted SHA, authority/grant and lease provenance matrix, positive/negative parent-child cases, actual background process lifetime/restart evidence, migrations, no-regression verification, accurate M003 unblock audit.

## 16. Handoff notes

Re-check completed M001 data model at implementation time. Do not infer scratch authorization from matching home directory or path strings. Do not merge this with blackboard shared-worktree feasibility experiments or Rust `target/` sharing.
