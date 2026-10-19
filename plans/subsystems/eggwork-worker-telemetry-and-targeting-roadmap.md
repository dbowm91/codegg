# Eggwork Worker Targeting and Host Telemetry — Subsystem Roadmap

Status: active planning; M001 ready, M002 blocked on Eggwork telemetry API closure, M003 ready after M001
Primary class: scheduler / infrastructure
Authority: `plans/000-long-term-specification.md`, `plans/001-terminology-and-domain-model.md`, `plans/002-long-term-roadmap.md`, `plans/003-planning-process.md`.
Related existing subsystem: `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md` (M001+C001+M002+M002a+M003 **closed**, its M004 whole-AgentRun remains **deferred**).
Upstream authority: `eggstack/eggwork: plans/adrs/ADR-0006-optional-gregg-host-telemetry.md` and `plans/subsystems/optional-gregg-host-telemetry-roadmap.md`.

## Purpose and ownership

Make current CodeGG fixed-target Eggwork execution **usable through the ordinary job and schedule interfaces**, then consume Eggwork's optional, authenticated Gregg-backed host telemetry for operator visibility and constrained, explicit-target admission. CodeGG alone owns global fair queue, retry, recovery, node-scoped reservations and optional future placement. Eggwork owns authenticated execution, leases, local protective admission and resource enforcement; greggd (optional, unchanged) owns native host observations. No new daemon is created in CodeGG.

## Baseline findings (2026-10-19)

- `src/scheduler/eggwork.rs` implements `ExecutionTarget::EggworkNode { node_id }` for Build/Lint/Format/ManagedProcess/Shell with canonical argv, mTLS, preflight, leased idempotent execution, transfer and RunStore artifact import. Remote Test and AgentTurn/Subagent are explicitly not eligible.
- `crates/codegg-core/src/jobs/mod.rs` persists target and exact attempt handle; `src/scheduler/executor.rs` selects executor by target. No remote failure falls back locally.
- `crates/codegg-protocol/src/dto.rs::JobSubmitDto` has **no target**; `crates/codegg-core/src/protocol_conversions.rs::job_submit_from_dto` hardcodes `ExecutionTarget::default()`. `JobTemplate` for `ScheduleStore` likewise has no target and schedule materialization cannot preserve a named node.
- `EggworkNodePosture` caches authenticated capabilities, draining and active/max slots; `node_postures()` has no confirmed production caller/public projection. No CPU/RAM/disk/network utilization is available.
- `build_spec` sends `Requirement::NotRequested` for memory/cpu/pids despite Eggwork resource enforcement. Terminal remote `ExecutorCompletion.metrics` is default/empty; host-level Gregg telemetry cannot correctly fill per-attempt metrics.

## Invariants / non-goals

1. Scheduler and JobSubmissionService remain sole CodeGG execution admission; no tool or frontend bypass and no second queue.
2. Explicit named `EggworkNode` persists durably, using configuration-owned endpoints and mTLS only, never arbitrary URLs or key paths in user job payloads.
3. Existing local jobs/schedules and serialized DTOs remain local when target omitted. Authorized remote requests fail closed for invalid/unknown/disallowed targets, unsupported kinds/policies or inaccessible nodes; no hidden local fallback.
4. Exact remote handle/lease fencing, no duplicate submit on restart, RunStore artifacts and bounded workspace transfer must not regress.
5. Gregg is never called by CodeGG; CodeGG consumes only Eggwork's mTLS-authenticated, bounded observation. Telemetry is advisory; absent/stale is not zero, cannot override required isolation/resources or prove that remote jobs stopped.
6. Host metrics must not be falsely assigned to a single attempt; quantitative per-job metrics need Eggwork-owned per-execution accounting and a separate contract.
7. Preserve declared scheduler admission fairness and do not charge remote CPU/memory as local machine execution forever; resource accounting changes require explicit contention qualification.

## Dependency graph

```text
CodeGG M001 DTO + schedule explicit target [ready] --------> M003 remote limits/admission [after M001]
Eggwork M001 local Gregg adapter [ready] -> Eggwork M002 mTLS telemetry API [blocked]
                                             |
                                             v
                               CodeGG M002 telemetry projection [blocked]
                                             |
                                             v
         future cross-node resource-aware placement [deferred, not in this batch]
```

M003 may start from existing Eggwork resource-enforcement feature contract when M001 closes; its terminal metrics enhancement must not block its limit/admission work. M002 becomes dependency-ready only with upstream Eggwork M002 closure and a pinned client/API revision.

## Milestones

**M001 — Explicit remote targeting in job and schedule protocols (ready).**
Plan: `plans/implementation/eggwork-worker-telemetry-and-targeting/001-explicit-target-job-and-schedule-boundary.md`.
User-visible exit: a user/agent with appropriate permission submits and schedules a finite supported job to a named Eggwork node; read/list/status/cancel/restart remain durable; old local requests behave identically.

**M002 — Consume authenticated Eggwork host telemetry (blocked: upstream Eggwork M002).**
Plan: `plans/implementation/eggwork-worker-telemetry-and-targeting/002-remote-node-telemetry-projection.md`.
User-visible exit: CodeGG's existing scheduler/operator read API exposes per-node sanitized CPU/memory/disk/network (where supported), timestamp/freshness, reachability/drain/slot separation and diagnostics; no Gregg network route, client poll multiplication or admission coupling.

**M003 — Explicit-target remote resource-policy convergence (ready after M001).**
Plan: `plans/implementation/eggwork-worker-telemetry-and-targeting/003-remote-resource-admission-and-limits.md`.
User-visible exit: CodeGG can configure typed per-kind remote request limits with clear required/best-effort semantics, preserves local/remote capacity distinction, and handles typed remote busy/resource failures with bounded queue/retry; no automatic target selection.

## Cross-cutting risks

API auth escalation if target is user-controlled; irreversible schedule migrations; resource tokens incorrectly translated into cgroup CPU time quotas; extra scheduler permit retention artificially throttling controller; stale metrics driving false availability; multi-workspace contention; old-client compatibility; pin drift and Linux/non-Linux enforcement differences. Each implementation plan contains negative tests and stop conditions.

## Deferred, explicitly not claimed

Automatic multi-node placement, worker discovery, whole remote AgentRun/subagent/Test, reverse-connect, Gregg collector extraction, per-execution cgroup timeseries and dynamic migration of accepted jobs. These require separate scoped planning/evidence; this roadmap **does not** rename or close the existing Eggwork remote AgentRun M004.

## Completion

Each milestone closes only after verified real mTLS Eggwork node exercises and a closure record under `plans/closure/eggwork-worker-telemetry-and-targeting/`. Register completed upstream dependency pins and exact deployed behavior; no Gregg repository change.
