# Agent Tool Runtime Reconciliation — Post-Closure Corrective Addendum

Status: active — C001 closed; C002 active; C003 blocked on C002

Audit baseline: `a54d18c7e7b92d1c9ac4971d7cdb8cc916418a5d` on `planning/agent-tool-runtime-reconciliation` (2026-10-13).
Integration base at audit: `b9ba494c1b21638c2e2f660301ba207ce8859682` (`main`); refs have diverged.

Predecessor (retain historical records, do not retrospectively rewrite closures):

- `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md`, M001–M004 marked closed.
- `plans/implementation/agent-tool-runtime-reconciliation/001-mcp-broker-transport-adapter.md` and `plans/closure/agent-tool-runtime-reconciliation/001-status.md`.
- `plans/implementation/agent-tool-runtime-reconciliation/003-descendant-tool-authority-ceilings.md` and `plans/closure/agent-tool-runtime-reconciliation/003-status.md`.
- `plans/implementation/agent-tool-runtime-reconciliation/002-external-tool-discovery-and-generation.md` and `004-provider-deferral-wire-qualification.md`; both accepted as foundation.
- `plans/closure/agent-tool-runtime-reconciliation/{002,004}-status.md`.
- `plans/003-planning-process.md#7-corrective-passes`.

Long-term references:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/003-planning-process.md`.

ADR disposition: no new ADR to repair the existing broker and subagent invariants. Stop and request an ADR if closing C001 requires redefining ToolBroker retry ownership, or C002 would change who can authorize a child, durable-run inheritance policy, or `ProcessExec` permission semantics. Existing sandbox, provider, scheduler, grant and Tool Program decisions remain binding.

## 1. Purpose and ownership boundary

Post-closure inspection found defects **not caught by the M001/M003 acceptance evidence**, plus operational closure gaps. This is a separate corrective campaign. It does not reopen previously implemented discovery/catalog logic or gated provider-native search.

Canonical owners remain:

- `AgentLoop` / `tool_batch.rs`: batch admission, permission receipt, hooks and ordering; **not** an independent retry authority.
- `ToolBroker` / `ToolContract` / `RetryContext`: effect-aware retry classification and authorization at invocation.
- `ParentToolAuthoritySnapshot` + `ResolvedToolSurface`: host-minted exact-name/capability/effect inheritance and generation binding.
- `SubAgentPool` / `TaskTool` / scheduler: functional child registration, durable lineage and lifecycle; never broader than parent.
- `scripts/verify.sh`, `.github/workflows/ci.yml` and planning registry: guard enforcement, hosted evidence, branch integration.

## 2. Evidence-driven defects

**F1 — Effect-unsafe outer retry.** `src/agent/tool_batch.rs` wraps `ToolBroker::execute` in `for attempt in 0..2`; on a broker `TimedOut` outcome, translation produces `ToolError::Timeout` and the outer `is_retryable()` branch repeats the submission. `McpToolAdapter::contract` explicitly supplies `NonIdempotent`, `DirectOnly`, `ToolRetryPolicy::none()`, but the outer caller ignores that policy. Re-running a remote write after an ambiguous timeout risks duplicate side effects, violating M001. The same outer-loop defect can impact other effectful tools; do not fix by special-casing only `mcp__`. The M001 closure's no-replay reasoning covered only broker-internal retries, not its batch caller.

**F2 — Functional nested task registered too late.** `src/agent/worker.rs` builds a functional `TaskTool` in `nested_task_tool` but computes `ResolvedToolSurface::from_registry_with_parent_authority` before `tool_registry.register(task_tool.with_parent_tool_authority(...))`. The initial default `task` has `has_functional_backend=false`, so the resolver omits it; the child snapshot and prompt miss a legitimately allowed `task`. Existing `worker_child_withholds_nested_task` tests only the negative path; a successful child→grandchild fixture is absent. M003's positive-delegation claim is therefore unqualified.

**F3 — Parent effect envelope is not verified at broker admission.** `ParentToolAuthoritySnapshot::check_tool(name,effect,time,workspace)` exists and is tested, but is not used in the production `ToolBroker::validate_pre_execution` path. Broker `allowed_tools` checks names and existing per-call grants independently, which **does not prove** a bypass of existing permissions; it does leave the new inherited effect ceiling unenforced as an independent security invariant. Moreover, `mint_delegation_ceiling` currently records `max_effect_class="non_idempotent"` unconditionally, while `ToolEffectClass::ProcessExec` has a stricter severity. A naive call to `check_tool` would break inherited shell/process tools, so C002 must reconcile the effect-envelope semantics using reviewed per-tool contracts and existing sandbox policy, not widen to `any`.

**F4 — Unqualified integration gates.** `scripts/check_mcp_broker_adapter.py`, `check_tool_descriptor_snapshot.py`, `check_descendant_tool_ceiling.py`, `check_tool_disclosure_wire.py` were executed manually but are not called by `scripts/verify.sh quick` or `.github/workflows/ci.yml`. No hosted runs on this branch were found at audit. Branch is ahead of the planning baseline yet diverged from `main`; original closure records still say `Pending commit` rather than authoritative SHAs. These are merge/readiness issues, not reasons to assert the original implementation never existed.

## 3. Invariants / non-goals

- At most one remote dispatch for an **unacknowledged, non-idempotent** tool call; timeout/cancellation/error never implies rollback and must not cause a transparent replay.
- Safe retries, if supported, are explicitly contract-qualified and budgeted by the broker, with stable invocation identity, not ad hoc `ToolError::is_retryable` logic in another layer.
- Child effective tool names/capabilities and effect envelope can only narrow. Broker invocation must reject a widened contract even for an otherwise eligible name, before any side effect.
- Functional nested delegation must remain available **when** authorized, and absent when withheld. The same effective registry, surface, broker catalog, available-tool names and nested authority must agree.
- Shell/process execution is controlled by existing separate sandbox/approval constraints; an effect-envelope fix must not silently disable legitimate safe workflows nor grant broader authority. No fallback `any`.
- Original M001–M004 closures stay historical. This addendum owns the new evidence and actual commit reconciliation.
- No new provider-native model allowlists, runtime agent tools, MCP transport, broad policy rewrite, persistent permission database, Windows/Linux sandbox design change, or new Tool Program capability.

## 4. Milestones and dependencies

```text
M001–M004 implemented/closed historically at audit SHA a54d18c7
           /                                  \
 C001 effect-aware dispatch               C002 nested + effect ceiling
  (ready, invariant)                        (ready, invariant)
           \                                  /
            \--------> C003 qualification ---/
                       (blocked; hard both)
```

### C001 — Single owner of tool retry admission
Class: invariant. Replace/reconcile the batch retry loop with ToolBroker-owned contract-safe behavior across MCP, plugin, shell and native tools. Closed when no non-idempotent/ProcessExec operation is replayed following an ambiguous timeout; safe explicitly idempotent retries preserve bounded behavior.

Plan: `plans/implementation/agent-tool-runtime-reconciliation-post-closure-corrective/001-contract-safe-tool-retry-admission.md`.

Closure: `plans/closure/agent-tool-runtime-reconciliation-post-closure-corrective/001-status.md` — closed at implementation `8d13bfd6`; fake MCP after-dispatch timeout and native ambiguous-effect counters prove one AgentLoop dispatch. Focused suites, quick, Clippy and the MCP adapter guard passed.

Dependencies: M001 broker adapter closed, existing ToolBroker retry machinery closed. **Closed**.

### C002 — Descendant contract enforcement and positive nested delegation
Class: invariant. Enforce host-attested descendant effect/name scope at broker entry; align authoritative child registry with `task` availability; reconcile `ProcessExec` effects without granting a wider sandbox or permission. Closed when an allowed child can actually delegate to an authorized grandchild, forbidden descendants fail before execution, and contract drift is rejected.

Plan: `plans/implementation/agent-tool-runtime-reconciliation-post-closure-corrective/002-descendant-effect-and-nested-task-corrective.md`.

Dependencies: M003 descendant boundary closed, existing scheduler/task/permission APIs; C001 retry contract is now closed. **Active**.

### C003 — Enforced guards, current-main integration and hosted qualification
Class: infrastructure. Wire the four predecessor guards plus new corrective coverage into quick+CI; verify independent guard self-tests/negative fixtures; integrate against current main with no unrelated code loss; hosted CI/desktop verification; issue new corrective closure records and accurate implementation SHA ledger.

Plan: `plans/implementation/agent-tool-runtime-reconciliation-post-closure-corrective/003-guard-ci-and-integration-qualification.md`.

Dependencies: C001 **hard (satisfied)**, C002 **hard (open)**; CI access and main reconciliation **operational**. **Blocked** until C002's accepted closure.

## 5. Cross-cutting requirements

No new storage or protocol migration anticipated; preserve additive `parent_authority` scheduler payloads and old sessions. Security requires deterministic hostile input, contract drift, cancellation, timeout and cross-workspace denial fixtures. Concurrency requires at-most-once ambiguous external call and nested sibling/no-shared-mutable-authority tests. Runtime must preserve broker timeout-grace and partial-output behavior, hook counts, parallel batch ordering and grant/workspace identity. No remote service should be called by test fixtures; use fake MCP and fake child spawner.

Integration uses exact SHA and records OS/feature/hosted availability. No premature `closed` statuses for unrun CI. If hosted CI is unavailable, C003 remains conditional/blocked, with link to the missing run rather than a false green status. Do not conflate native tool-search fixture qualification with live provider enablement; it remains gated.

## 6. Verification strategy and completion definition

- C001 negative: timeout after first remote dispatch, the fake server records one call only; no second approval, hook or transport call. Positive: explicit retry-safe read uses a bounded broker retry context, not a second implicit caller loop.
- C002 positive: root→child→grandchild, functional `task` visible and callable only if permitted; negative: denied/forged/stale/widened-effect tool is not dispatched to MCP/plugin/native/shell. Preserve full sandbox/worktree isolation.
- C003: existing `cargo fmt`, Clippy, `scripts/verify.sh quick`, targeted regression suites, static guards and mutation self-tests, full workspace nextest and required `server,plugins,lsp-test-support` feature check; hosted `CI / verify` and Desktop E2E on exact final reconciled SHA.

All three must have separate accepted closure records at `plans/closure/agent-tool-runtime-reconciliation-post-closure-corrective/00N-status.md`, with failure evidence, implementation commits, explicit qualification gaps and honest status. Existing M001–M004 status remains historical. This addendum becomes **closed** only once C001–C003 close and both host and documentation are consistent.
