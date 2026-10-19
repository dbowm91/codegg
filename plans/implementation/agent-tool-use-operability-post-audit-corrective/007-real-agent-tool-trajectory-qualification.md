# Agent Tool-Use Operability Corrective C007 — Qualify the Normal Agent Tool-Use Trajectory

Status: active implementation (2026-10-19); all hard predecessors C001–C006 and C008 are strictly closed, and C002's follow-up closure is `plans/closure/agent-tool-use-operability-post-audit-corrective/002-follow-up-status.md`. Historical blocker record `plans/closure/agent-tool-use-operability-post-audit-corrective/007-status.md` remains unchanged.
Repository baseline: `4c86bac2a608dd717be085247838fd2f37d81ab6` (main, 2026-10-19)
Source roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `#28-observability`; `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.
Applicable ADRs: ADR-0001 (programmatic authority), ADR-0004 (approval routing), ADR-0028 (execution authority), only insofar as touched. No ownership-changing decision is authorized.
Primary class: capability

## 1. Objective
Prove across ordinary daemon/agent/broker dispatch that a scripted coding agent can perform local discovery, first-use edit, verification, restricted Tool Program execution, explicit todo updates, durable subagent lookup and concurrent tool calls without the reported unexplained failures.

## 2. Why this milestone is ready
Hard dependencies C001–C006 and C008 have strict closure records; C002's authenticated workspace binding and remaining capability matrix are strictly closed in the follow-up record. Existing deterministic fake-provider and real pinned Eggsearch private-child fixtures are stable interfaces. No ownership-changing decision is required.

## 3. Current implementation evidence
- The 2026-10-19 assessment exercised each tool individually and found mixed failures. Isolated unit/child tests can pass while production-path context is lost (notably Tool Program). `tests/agent_loop_harness.rs`, `tests/tool_execution.rs`, `tests/eggsearch_pair_compat.rs` already provide building blocks; the roadmap requires one end-to-end path, not a new permanent benchmark pipeline.

## 4. Invariants that must not regress
- Single authenticated workspace root for entire turn; no real public provider/network calls. All model outputs and external result snippets remain untrusted; permissions and explicit Deny continue to apply. No background task silently acquires parent turn authority; no agent forced to launch privileged shell for straightforward local reads.

## 5. Scope
In: the bounded tool contract and owning files described here, focused negative/positive regression tests, documentation and explicit compatibility corrections.
Out: new schedulers or authorization engines, Python sandbox expansion, new public network clients, multi-provider live CI, new release/packaging work, unrelated UI refactors, full repository feature sweeps and SBC compile/startup benchmarks.

## 6. Required production changes
Add one scripted provider-driven production loop fixture reusing the existing daemon/tool registry/broker construction, pinned sidecar configuration and isolated filesystem. Include real tool call IDs and model-visible output assertions. Separate fixture assertions for success, legitimate deny, unavailable backend and timeout; record any baseline fixture limitation rather than modifying unrelated agent loop design. Assert no silently lost todo update, unnecessary read prerequisite, wrong task/run ID, opaque verification argv, web fallback presented as local, or replay of cancelled call.
Storage: no migration planned; stop if a persisted schema change becomes necessary.
Protocol: preserve existing durable identities and transport messages; any model-visible schema evolution must be backward compatible or explicitly justified.
Security: no widening of command, workspace, tool or user authority. No explicit Deny recovery by retry or silent fallback.
Documentation/static guards: prefer focused existing Rust tests over new broad static checkers.

## 7. Ordered work packages
### A — Assemble normal-path fixture
Use a temporary repository fixture with unique symbol and optional Cargo crate, bind session/root through production construction, connect pinned local-only eggsearch child; scripted provider emits canonical tool_calls rather than invoking implementations directly.
### B — Positive workflow
`repo_search` local find → `grep` first-use → `apply_patch` create/update → `verify(lint)` → restricted Tool Program → two-in-progress explicit todowrite → spawn/wait/status/get durable child. Assert task IDs and tool outcomes.
### C — Negative/restart/contention matrix
Disabled local index, malformed patch, wrong path, denied permission, expired receipt, 1-slot long call + short reads, queued cancellation and daemon restart of durable job state. No live-provider CI.
### D — Closure
One coherent quick and existing hosted CI only if readily available/mandated; record exact tested revisions; audit downstream blocked work and publish C007 closure.

## 8. Failure, cancellation, restart, and contention semantics
Deterministic fixtures exercise normal cancellation and restart; work is bounded by existing timeouts and tool output caps. No negative path may yield an unearned success. A cancelled queued call never later executes. Child restart consumes only durable authorized state; no fabricated results.

## 9. Compatibility and migration
No new remote public API or storage migration. Use test-only scripted provider and private sidecar; do not export this harness as a new general code execution surface or permanent performance benchmark.

## 10. Required tests
- Integration e2e: all listed positive calls through production registry/broker, explicit tool ID/state assertions, no direct-only stand-ins.
- Negative authority, containment, no-index, source/provenance, task-ID, hunk-format and verification schema cases.
- Contention/cancellation/restart; verify bounded output/context growth.
- Optional manual real model smoke may be reported separately, not required for deterministic closure and never disguised as CI evidence.

## 11. Required verification commands
```bash
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test tool_execution
cargo test -p codegg --test eggsearch_pair_compat
scripts/verify.sh quick
```
Required handoff commands, not executed during this planning-only pass. Constrain tests to the touched crate/feature and record any unavailable binary/platform evidence.

## 12. Documentation updates
`architecture/agent.md`, `architecture/tool.md`, `architecture/search_backend.md`, `plans/registry.md`; update only source owners actually modified.

## 13. Acceptance criteria (externally observable)
- One same-session scripted trajectory performs all tool classes and yields correct model-visible results, no false stall or silent missing authority.
- Negative trajectories produce bounded truthful failures; no unauthorized filesystem/network work.
- C001–C006 closure evidence integrated, residual high findings named, closure and unblock audit recorded.

## 14. Stop conditions
If any predecessor is not strictly closed, tests silently bypass broker/real sidecar, or a fix requires a new ownership/authorization/IPC contract or new upstream pin without independent qualification.
Do not improvise an unapproved ADR, silently weaken security, substitute mocks for required production-path qualification or rewrite historical closure records.

## 15. Closure evidence required
Closure `plans/closure/agent-tool-use-operability-post-audit-corrective/007-status.md`: exact implementation SHAs, before/after repro, evidence matrix, tests/commands with pass/fail/unrun, failure/restart/contention/security analysis, residual findings and unblock audit; update roadmap and registry in the same closure commit.

## 16. Handoff notes
This is integration/qualification, not an excuse to reopen already closed tool-seam scope. Explicitly identify source-verified fixes vs live validated behavior and do not claim real-model autonomous endurance from scripted outcomes.
