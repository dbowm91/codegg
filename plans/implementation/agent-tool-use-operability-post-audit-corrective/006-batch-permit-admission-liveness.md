# Agent Tool-Use Operability Corrective C006 — Separate Tool Batch Admission from Execution Deadlines

Status: ready for handoff
Repository baseline: `4c86bac2a608dd717be085247838fd2f37d81ab6` (main, 2026-10-19)
Source roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `#28-observability`; `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.
Applicable ADRs: ADR-0001 (programmatic authority), ADR-0004 (approval routing), ADR-0028 (execution authority), only insofar as touched. No ownership-changing decision is authorized.
Primary class: invariant

## 1. Objective
Prevent legitimate queued tool calls being classified as execution failures solely because an earlier call held the batch permit; retain bounded liveness and clear queue telemetry.

## 2. Why this milestone is ready
No hard dependencies: the per-batch semaphore and timeout configuration are already in `src/agent/tool_batch.rs`. This does not authorize changing JobScheduler global admission; integration C007 follows.

## 3. Current implementation evidence
- In `src/agent/tool_batch.rs` a new semaphore is constructed for each regular-tool batch from effective_max. `permit_timeout = Duration::from_secs(tool_timeout_secs)` wraps `sem.acquire()`, then a per-tool timeout starts only after admission; `join_all(futures)` waits for all. Timeouts surface `tool did not acquire an execution permit within ...`. This semaphore alone does not establish global parent/subagent contention. A model-requested per-tool timeout may differ from the permit wait budget.

## 4. Invariants that must not regress
- No queued call executes after cancellation, host denial or terminal turn. Concurrency stays bounded; per-batch admission never replaces scheduler-owned process/resource admission. Tool execution watchdog must retain partial child output. No automatic retry of mutating or uncertain-side-effect calls and no unbounded waiting.

## 5. Scope
In: the bounded tool contract and owning files described here, focused negative/positive regression tests, documentation and explicit compatibility corrections.
Out: new schedulers or authorization engines, Python sandbox expansion, new public network clients, multi-provider live CI, new release/packaging work, unrelated UI refactors, full repository feature sweeps and SBC compile/startup benchmarks.

## 6. Required production changes
Explicitly separate `queued`, `admitted`, `executing`, and `completed/failed/cancelled` phases in existing tool outcome/telemetry without a new durable service. Audit how `checkpoint_serialize` reduces concurrency to one and whether long-running children block trivial calls. Use a bounded, independently configured/derived queue admission deadline not implicitly equal to execution timeout; make timeout errors identify queue wait vs run time, effective parallelism and actual wait. Preserve fair ordering under Tokio semaphore unless evidence justifies small stable behavior change. Optional fast read-only prioritization is out of scope without fairness analysis.
Storage: no migration planned; stop if a persisted schema change becomes necessary.
Protocol: preserve existing durable identities and transport messages; any model-visible schema evolution must be backward compatible or explicitly justified.
Security: no widening of command, workspace, tool or user authority. No explicit Deny recovery by retry or silent fallback.
Documentation/static guards: prefer focused existing Rust tests over new broad static checkers.

## 7. Ordered work packages
### A — Reproduce controlled queue
Use an admission barrier to hold one slot, queue short reads, and show two distinct timeout clocks; run with effective_max 1, 2 and 4. Capture current behavior and reason codes.
### B — Fix bounded queue-state semantics
Resolve separately a queue wait budget and execution budget; propagate cancellation while queued; allow process/background APIs to continue using their own scheduler contract. Prevent a queued call from being mislabeled as a tool execution timeout.
### C — Prove liveness and no hidden retries
Test fairness, slow preceding call, checkpoint-serialized mode, cancellation, timeout boundary and delivery of complete ordered result set.

## 8. Failure, cancellation, restart, and contention semantics
Queued call must be cancelled promptly and never receive an execution permit after cancellation. Tool that times out after admission returns its native timeout/partial output when available. Restart must not resurrect ephemeral batch-waiters. All tasks in the batch eventually receive terminal outcome; no indefinite `join_all` stall. Contention diagnostics must not claim shared-global permit if only local.

## 9. Compatibility and migration
Internal outcome/reason-code additions should preserve provider-visible tool IDs and existing terminal statuses; no public protocol migration. A truly useful background command can be submitted through already accepted background job capability, not a new queue in this milestone.

## 10. Required tests
- Deterministic one/two/four-permit barrier matrix with long tool + quick reads; all queued result IDs accounted.
- Cancellation before acquire; deadline race; timeout after acquire; long-running native command retains bounded partial output.
- Parent and subagent simultaneous batches traced to show actual semaphore ownership, without assumed global linkage.
- No unexpected second scheduler or automatic mutating retry; bounded memory/permits.

## 11. Required verification commands
```bash
cargo test -p codegg --lib agent::tool_batch
cargo test -p codegg --lib agent::loop
scripts/verify.sh quick
```
Required handoff commands, not executed during this planning-only pass. Constrain tests to the touched crate/feature and record any unavailable binary/platform evidence.

## 12. Documentation updates
`architecture/agent.md`, `architecture/tool.md`, `architecture/scheduler.md` only if current admission contract wording changes.

## 13. Acceptance criteria (externally observable)
- Queued vs executing is visible and counted; a waiting tool is never described as already executed.
- Under controlled saturation, authorized short calls either execute after available admission or terminate with a specific queue deadline; batch remains bounded, without silent timeout masquerading as tool failure.

## 14. Stop conditions
If behavior would change JobScheduler-owned admission, cross-agent resource quotas, host containment, or require a new global scheduler/permission authority; create an explicit ADR proposal and stop.
Do not improvise an unapproved ADR, silently weaken security, substitute mocks for required production-path qualification or rewrite historical closure records.

## 15. Closure evidence required
Closure `plans/closure/agent-tool-use-operability-post-audit-corrective/006-status.md`: exact implementation SHAs, before/after repro, evidence matrix, tests/commands with pass/fail/unrun, failure/restart/contention/security analysis, residual findings and unblock audit; update roadmap and registry in the same closure commit.

## 16. Handoff notes
Coordinate potential future per-workspace compiler/build admission with `plans/subsystems/agent-workstream-blackboard-roadmap.md` M007; this plan owns only the agent tool-batch semaphore.
