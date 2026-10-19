# Agent Tool-Use Operability Corrective C001 — Preserve Verified Authority Through Broker and Tool Program

Status: active
Repository baseline: `4c86bac2a608dd717be085247838fd2f37d81ab6` (2026-10-19 main)
Source roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `#28-observability`; `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.
Applicable ADRs: ADR-0001 programmatic authority; ADR-0004 approval routing; ADR-0028 execution authority (only where applicable). No new ADR required if existing ownership remains unchanged.
Primary class: invariant

## 1. Objective
Preserve Verified Authority Through Broker and Tool Program. Close only the specified real-model operating contract; do not conflate source review with successful runtime reproduction.

## 2. Why this milestone is ready
No unresolved hard dependency: existing tool, authorization and test seams are present. Interface dependencies are current repository contracts; re-check their SHAs at implementation.

## 3. Current implementation evidence
- `src/agent/tool_batch.rs::build_tool_execution_context` sets `decision_id/outcome` from an accepted receipt but leaves workspace path-policy ID/revision absent. `src/tool/broker.rs::execution_context_for_attempt` reconstructs a per-attempt context with those fields None. `src/tool/tool_program_context.rs::to_core_context` requires nonempty trusted decision, principal, path-policy and policy revision fields. Report's arithmetic-only Tool Program failed `missing decision_id`; this is a production-path propagation gap, not evidence that the restricted language needs imports.

## 4. Invariants that must not regress
- One immutable, verified permission decision governs all attempts and retries. Missing/revoked/expired permission must fail closed. Workspace authority is not inferred from user/model input, current-directory strings or audit correlation IDs. No generic widening of Tool Program's restricted IR.

## 5. Scope
In: source paths and bounded tool-contract behavior listed below, focused regressions, model-facing diagnostics, and affected docs.
Out: unrelated runtime refactors, new global scheduler, network CI, public release and SBC compilation/startup benchmarks; do not weaken authorization or workspace containment.

## 6. Required production changes
Core: identify the canonical verified receipt/path-policy owner and project its values into the broker's per-attempt ToolExecutionContext. Avoid duplicating permit decisions or manufacturing identifiers. Runtime: preserve immutable receipt across retry, cancellation and scheduler submission; refuse mismatched workspace/agent/session IDs. Security: preserve caller/effect ceilings and frozen tool catalog; do not treat a path root's presence as path-policy authorization.
Storage: no new schema or migration expected; any proposed persistence change requires a new documented contract.
Protocol: preserve existing names/fields except the explicit compatibility corrections below; no remote protocol migration.
Security: no model-supplied source may grant privilege; fail closed on an absent required authority.
Documentation/static guards: update only owning source docs and focused tests/guards; do not duplicate existing cross-cutting scanners.

## 7. Ordered work packages
### A — Trace the accepted provenance chain
Follow agent loop permission evaluation → `BrokerInvocationContext`/verified grant → `execution_context_for_attempt` → `ToolProgramTool::execute_impl` → `to_core_context`. Document which fields exist from a verified producer and which are currently None. Evidence: an assertion that captures actual per-attempt fields.
### B — Repair projection at one trusted boundary
Plumb the real accepted receipt and effective path-policy identity/revision (if available) without fallback tokens. Reject inconsistent grant/decision identity. Evidence: positive normal-path broker execution, not direct struct invocation.
### C — Negative and resume coverage
Test missing/revoked/expired receipts, cross-session/root forgery, cancellation and background/foreground submission; confirm no side-effect before rejection.

## 8. Failure, cancellation, restart, and contention semantics
A rejected submission must create no executable job or side effect; retries reuse the same authorization snapshot but fresh budget and cancellation accounting. Restart revalidates durable grants through the existing scheduler owner rather than trusting stale serialized context.

## 9. Compatibility and migration
No breaking tool names/manifest changes; ephemeral decision metadata only. If path-policy revision is not obtainable from any genuine trusted boundary, stop and request an authority-contract ADR rather than synthesize `workspace:...` or `agent:...` tokens.

## 10. Required tests
- Unit: broker per-attempt field projection and receipt identity; Tool Program context accepted/missing path policy.
- Integration: real agent→broker→native `tool_program` with `emit(str(1+1))` using an allowable tool manifest; foreground + background; distinguish compile success from execution.
- Security: missing/mismatched/revoked/expired decision and changed workspace policy all deny before job creation.
- Cancellation/restart: no replay with invalidated authority.

## 11. Required verification commands
```bash
cargo test -p codegg --lib tool::broker
cargo test -p codegg --lib tool::tool_program
cargo test -p codegg --lib agent::tool_batch
scripts/verify.sh quick
```
These are required for implementation closure when applicable, not claims about work run while authoring the plan. Use `--locked` and no `--all-features` unless explicitly justified for a narrow target; no required live-provider or SBC run.

## 12. Documentation updates
`architecture/permission.md`, `architecture/tool_programs.md`, `architecture/tool.md` if context shape changes.

## 13. Acceptance criteria
- Through the real broker path, valid minimal Tool Programs no longer fail with `missing decision_id` or missing path policy; return a structured result or an unrelated specific error.
- One negative decision fails closed, correctly attributed, with zero scheduler execution. Retries cannot widen authority.

## 14. Stop conditions
If the only proposed solution is forged decision/path-policy revisions, bypassing Broker, relaxing `to_core_context`, changing caller effects or granting Python shell imports.
In all cases stop rather than silently changing accepted ADRs, inventing authority, dropping scope requirements, rewriting historical closure or creating unbounded work.

## 15. Closure evidence required
Exact implementation commit and branch; before/after failure trace; requirement-to-test matrix; tests/guards actually run (and explicitly unrun); negative security cases; contention/restart treatment; known limitations with severity; `plans/closure/agent-tool-use-operability-post-audit-corrective/001-status.md` and registry/roadmap status update and unblock audit.

## 16. Handoff notes
Prefer `execution_authorization` / verified grant as source; inspect broker's currently separate trusted audit projection, which does NOT automatically populate permission receipt. Do not assume direct test calls use the normal runtime construction path.
