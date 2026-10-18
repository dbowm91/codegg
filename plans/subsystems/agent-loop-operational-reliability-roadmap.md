# Agent Loop Operational Reliability Roadmap

Status: active planning; implementation not started

Audit baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (main, 2026-10-18)

Long-term references:
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/001-terminology-and-domain-model.md#7-agent-terms`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Decision gate: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md` (proposed; gates M002 termination semantics). Existing accepted shell policy ADR-0028 remains authoritative.

Prior foundations, not reopened:
- `plans/closure/agent-recovery-escalation-authority/001-status.md` (C001: removed tool-only ShortCycle, corrected equivalent-error action key and per-incident reset)
- `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md` (typed outcomes)
- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md` (closed disclosure/discovery work)
- `plans/subsystems/long-horizon-work-execution-roadmap.md` (closed WorkPlan/Goal/epochs)
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md` (closed retry/approval foundation)
- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md` (shell authority; do not supersede)
- `plans/subsystems/agent-workstream-blackboard-roadmap.md` (parallel worktree/coordination; independent)

## 1. Purpose and ownership boundary

Make a normal agent-driven source-read/build/test/edit/delegate loop reliably usable over long trajectories, including small/tool-fragile models, by fixing **observed harness friction** before increasing intelligence or orchestration. This successor is a **corrective integration workstream**, not a replacement agent loop.

Owns: model-facing output projection ergonomics and evidence retrieval; progress/recovery decision quality and bounded diagnostics; tool-discovery/path/preflight clarity; subagent transaction/provider compatibility and failure-report integration; deterministic sustained-trajectory qualification.

Consumes canonical owners: `src/context` artifact store and compaction; `ToolRegistry`/`ToolBroker`; `AgentLoop`/`AutonomyState`; SQLite `AgentRunStore` and scheduler; `ProviderCapabilities` and wire adapters; approval/sandbox policy. Does not create independent stores, schedulers, tool executors, or security policies.

## 2. Work classification

**Invariant:** requested content is either delivered within declared bounds or represented by an explicit, functional continuation; `context_read` cannot recursively hide its requested slice; incident identity is whole call + relevant state; unknown progress is not affirmative stagnation; typed status and run budgets remain authoritative; no unauthorized tool is discoverable/callable; mutating retries are idempotent and fenced.

**Capability:** model can inspect ordinary source files without a forced 20-line preview, use `glob` relative to workspace, discover permitted tools using normal task phrases, receive usable subagent failure information, and continue after transient provider/storage faults.

**Infrastructure:** bounded artifact-paging/projection policy, progress-evidence attribution, stale SQLite transaction retry, endpoint-validated tool-definition finalization.

**Polish:** fewer redundant calls, clearer advisories, operation-specific output summaries and trajectory diagnostics.

## 3. Non-goals

- Reopen closed C001 ShortCycle/EquivalentError fixes or remove their regression tests.
- Set every tool's output to unlimited; disable compaction/artifact storage.
- Make all shell commands permissible, bypass explicit Deny, or remove OS sandbox enforcement.
- Implement another queue, daemon, scheduler, context history DB, worktree service or general workflow engine.
- Change the master specification merely to adapt implementation details.
- Add live-provider CI, a new testing/benchmark lane, mandatory hour-long CI, SBC startup/compile tests, or release claims.

## 4. Current state and audit evidence

- `src/tool/read.rs` defaults to 2,000 lines; `src/context/projection.rs` defaults to 800 estimated tokens for success and truncates oversized output to the first **20 lines**. `src/agent/loop.rs` inserts `proj.model_text` into the model transcript, including results from `context_read`; `src/context/read_tool.rs` pages 20,000 bytes by default. The interaction, rather than `read.limit`, is the principal suspected read friction. Test the double-projection path explicitly before editing.
- `src/agent/progress_recovery.rs`: default incident recovery count 4; `AutonomyState::MAX_TRANSITIONS=4`, `MAX_POST_TOOL_CONTINUATIONS=1`. Successful process-class calls do not prove progress; `src/agent/loop.rs` only credits file-change bus events to direct writer calls, and `Stalled` breaks the turn. Earlier tool-only false positive bugs are **already fixed**.
- `src/tool/catalog.rs`: default keyword search uses whole-query substring matching; `src/tool/tool_search.rs` returns `no_results` on empty queries. Reported tool discovery failed for ordinary multiword phrases.
- `src/tool/glob.rs`: canonical absolute walk paths are passed to a matcher built from a relative pattern. Reported `scripts/verify*.sh` missed an existing `scripts/verify.sh`; verify with a failing fixture.
- `src/agent/tool_batch.rs` converts preflight findings to advisory/escalation summary; `src/tool/bash.rs` hard-blocks standalone preflight `Block`, but in authorized context reuses the summary. Explicit Deny/security ceilings are separate and must prevail.
- `src/tool/task.rs` propagates durable child create errors and returns terse `Task/Run/Status/Result` strings. The observed child test saw SQLite extended code 517 (`BUSY_SNAPSHOT`), then provider incompatibility with deferred tool definitions; determine transaction and wire truth, not just rewrite error strings.
- `src/agent/request_preparation.rs` partitions defer-loading by provider ID; the non-deferral branch sends all definitions but does not explicitly strip deferral bits. `crates/codegg-providers/src/provider_core.rs` advertises broad `openai`/`anthropic` deferral support, not necessarily every gateway or model. Verify actual failing provider wire.
- `ExecutionLimits` defaults: 100 turns, 1,000,000 tokens, 600-second loop timeout. These are per-execution safeguards and are not a substitute for durable long-horizon budgets or resumable jobs.

The source is evidence of code paths, *not* proof every reported runtime reproduced at this exact commit. Capture actual binary SHA, model/provider and raw typed outcomes before closing each corrective milestone.

## 5. Target architecture

Tool call -> authoritative executor/typed outcome -> durable bounded artifact -> **operation-aware, non-recursive** model projection (or exact bounded artifact page). Tool completion -> typed progress observation (call identity + resource revision + output completeness + child/job/test evidence) -> bounded recovery policy -> run lifecycle controlled separately by budgets, cancellation, verified terminal failure and, if ADR-0040 accepted, affirmed stagnant cycles. Provider requests pass through capability-aware final wire validation before dispatch; durable child creation uses transaction-safe bounded retry; failures remain visible to parent.

## 6. Dependency graph

```text
M001 source/artifact projection ──────────────┐
M003 discovery and glob correction ───────────┤
M004 preflight authority clarity ─────────────┤
M005 subagent durable admission/errors ───────┼──> M007 sustained qualification
M006 provider/tool wire compatibility ────────┤
ADR-0040 acceptance ──> M002 recovery ───────┘
```

M001, M003, M004, M005 and M006 are independently **ready**. M002 is **blocked** by the proposed ADR's decision on termination authority (evidence gathering/tests may be prepared but behavior must not be shipped as accepted). M007 is **blocked** until M001–M006 close with evidence. M005/M006 may run independently but must share one final subagent scenario for M007. No mandatory external dependency.

## 7. Milestones

| Milestone | Class | Status | Deliverable / exit condition |
|---|---|---|---|
| M001 | capability | **ready** | `plans/implementation/agent-loop-operational-reliability/001-tool-aware-projection-and-artifact-paging.md`; recoverable contiguous source and artifact reads, nonrecursive paging, bounded budgets. |
| M002 | invariant | **blocked: ADR-0040** | `plans/implementation/agent-loop-operational-reliability/002-progress-aware-recovery-and-terminal-stall-policy.md`; no false terminal stalls in revision-aware trajectories; confirmed no-op loop still bounded. |
| M003 | polish | **ready** | `plans/implementation/agent-loop-operational-reliability/003-workspace-glob-and-tool-discovery.md`; real relative glob hits and authorized natural-language discovery. |
| M004 | invariant | **ready** | `plans/implementation/agent-loop-operational-reliability/004-preflight-and-shell-authorization-semantics.md`; diagnostics and allow/ask/deny agree across entrypoints. |
| M005 | infrastructure | **ready** | `plans/implementation/agent-loop-operational-reliability/005-subagent-admission-and-failure-visibility.md`; bounded busy-snapshot recovery, idempotent spawn, actionable child termination details. |
| M006 | infrastructure | **ready** | `plans/implementation/agent-loop-operational-reliability/006-provider-tool-schema-compatibility.md`; no unsupported deferred-tool wire and bounded safe request downgrade. |
| M007 | capability | **blocked: M001–M006** | `plans/implementation/agent-loop-operational-reliability/007-sustained-autonomy-trajectory-qualification.md`; deterministic end-to-end replay and optional manual duration run; no permanent new CI lane. |

## 8. Cross-cutting requirements

**Authorization:** every tool, including discovery, paging and downgraded provider requests, stays within the same frozen policy ceiling. No scanner result can supersede an explicit hard Deny. Sensitive output never bypasses redaction due to "lossless" projection.

**Lifecycle:** child identities, completion notifications, bounded wait, cancellation and restart must not be confused with model-side polling. Explicit budget exhaustion and user cancellation are separately classified from "stalled". Persist only necessary bounded diagnostics in the existing stores.

**Compatibility:** preserve `ctx://` handle formats and existing provider requests unless capability negotiation requires omission of unsupported fields. Additive schema changes only, with tests for legacy clients/config. Do not rename closed historical milestones.

**Observability:** record redacted tool-size estimates, projected vs retrieved bytes, model-visible continuation, progress signal provenance, recovery action/incident identity, provider wire capability, SQLite extended error and parent-visible failure; no raw secrets or model reasoning in recovery metadata.

## 9. Verification and closure policy

Each milestone's implementation plan defines required focused tests, a native integration path, cancellation/restart/security negatives, and commands. Baseline canonical checks are `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `scripts/verify.sh quick`, proportionate to changed scope. No command is claimed executed by this planning commit. M007 owns a reproducible deterministic scripted-model workload and manual optional 30–60-minute model-backed smoke, *not* a fixed CI duration gate. No SBC compile/startup benchmark.

## 10. Risks and deferred work

- Progress revision capture can become over-permissive if it treats timestamp/log churn as productive. Require typed evidence.
- Large source paging raises context cost. Bound per-result by model-profile budget; give exact offsets.
- Provider downgrade must not mask a genuine authorization refusal or replay a completed side effect.
- SQLite contention may require read/write transaction-shape correction instead of blind sleep-and-retry.
- Ambiguous external model behavior requires recording an adapter limitation, not inferring user intent.
- Further work on peer shared-write placement, generalized sandboxing or a new scheduler is explicitly deferred to their owners.

## 11. Close condition

Roadmap is closed only when all seven milestones have accepted closure records, M002's decision gate is resolved, deterministic and existing regression suites pass on the claimed head, no critical/high unresolved correctness or security finding remains within this workstream, and M007 establishes observable sustained coding-loop reliability without a false terminal stall. A planning commit is not implementation or closure.
