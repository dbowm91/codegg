# Agent Runtime, Model Adaptation, and ACP Milestone 009 — Closure Status

Status: closed

Source implementation plan:

- plans/implementation/agent-runtime-model-adaptation-acp/009-context-plan-and-cache-convergence.md

Source subsystem roadmap:

- plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-009--context-plan-and-cache-convergence

Repository baseline reviewed: 1acb540685a5792c0d893348d9c92a09ba41a80a

Implementation commits:

- 5fdb9da — converge context plans and compound cache identity

## 1. Executive finding

Milestone 009 is complete and strictly closed. ContextPlan is now the
provider-facing source of truth at the final request boundary. Full mode is
lossless and chronological; the packer remains a diagnostic cost view.
Conservative tool-palette reduction remains reversible and authorized by the
existing M002 policy. Cache usage is keyed by provider/model/adapter/compiler/
tool-surface/mode identity rather than model name alone.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed plan and deterministic identity | src/context/plan.rs; context_plan_convergence tests | pass | Stable-prefix, plan, tool-surface, adapter, and compiler fingerprints are deterministic. |
| Actual provider request consumption | AgentLoop::apply_context_plan before provider calls; integration test | pass | Applied after hooks, history hardening, compaction opportunity, and palette policy. |
| Chronological transcript/tool protocol | validate_tool_protocol; focused plan tests | pass | Assistant calls and matching tool results remain ordered; invalid pairing is rejected. |
| Stable/slow/volatile/control classification | ContextPlan::packing_blocks; context tests | pass | Diagnostic packing derives from the same plan, without supplying the request. |
| Conservative active reduction and restore | Existing M002 apply_tool_palette_policy_if_active | pass | Reduction is base-surface-derived, opt-in, backoff-aware, and full-surface safe. |
| Compound cache telemetry | CacheIdentity, record_usage_with_identity, loop recording path | pass | Production usage records the bounded compound plan key; legacy API remains compatible. |
| Private-content containment | ContextPlan::diagnostics; negative integration test | pass | Diagnostics expose hashes/counts only; private reasoning is not serialized into the summary. |
| Artifact/compaction convergence | Plan applied after existing projection/compaction paths | pass | Existing artifact handles and compaction output enter the one final ordered plan. |
| Documentation and operations | architecture/cache-aware-context.md | pass | Request source, diagnostics, identity, and reduction boundaries documented. |

## 3. Production implementation evidence

- Added ContextPlan, PlannedMessage, CacheIdentity, diagnostics, and
  protocol validation under src/context/plan.rs.
- The agent loop applies a full plan after initial setup and again immediately
  before provider dispatch, after plugin transforms and history hardening.
- Diagnostic candidate construction now derives from the same plan rather than
  the former independent candidate builder.
- Provider finish usage is stored under the compound plan identity.
- Existing tool execution, permissions, artifact storage, compaction, and
  model-adapter authority remain owned by their existing components.

## 4. Verification executed

### Commands run

    rtk cargo fmt --all -- --check
    rtk cargo test --test context_plan_convergence
    rtk cargo test -p codegg --lib context::
    rtk cargo test -p codegg --lib context::cache_stats
    rtk cargo test -p codegg --lib agent::compaction
    rtk cargo test -p codegg --lib agent::loop
    rtk cargo test --test agent_loop_harness
    rtk cargo test --test asset_snapshot
    rtk cargo check -p codegg
    rtk cargo check --workspace --all-targets --locked
    rtk bash scripts/check-core-boundary.sh
    rtk python3 scripts/check_scheduler_bypass.py
    rtk scripts/verify.sh quick

### Results

All commands passed. Focused results included 4 context-plan integration
tests, 266 context tests, 29 compaction tests, 40 agent-loop harness tests,
and 8 asset-snapshot tests. The filtered agent::loop unit command matched no
tests and passed; the production-shaped harness provided the loop evidence.
Verification emitted only existing dead-code warnings. No live provider cache
telemetry was attempted; it is supplementary and not required for local
correctness closure.

## 5. Invariant review

- Message chronology and tool-call/result pairing are preserved by sequence-
  ordered plan messages and validation.
- Stable content precedes volatile diagnostic tiers, while provider messages
  are never globally sorted.
- Full mode does not omit required system, user, assistant, or tool protocol
  content.
- Planning cannot grant authority; it consumes the already resolved tool
  surface.
- Fingerprints are deterministic and content bodies are excluded from
  diagnostics.
- Adapter and compiler identities are included in cache identity.

## 6. Failure and recovery review

Planning validation returns a typed agent error before provider dispatch.
Existing compaction and projection failures retain their prior bounded
fallback behavior, after which the valid result is planned. Tool-surface
reduction is per-call reversible and restores the base surface on empty
selection, starvation, or backoff. Concurrent turns own immutable request
plans and local cache-stat updates. A restarted turn rebuilds its plan from
durable messages and current pinned runtime assets.

## 7. Migration and compatibility review

No storage or protocol migration is required. Existing ChatRequest and
provider message types remain unchanged. ContextCacheStats::record_usage
continues to support legacy model keys; production turns use the richer key.
Requests with tools = None retain None through plan application.

## 8. Security review

The plan has no permission or tool-authority grant path. Tool definitions are
copied from the already authorized request surface. Private reasoning is
represented only as a bounded classification in diagnostics, and plan
diagnostics do not contain user, tool, or reasoning bodies. Existing artifact
projection and path/redaction boundaries remain unchanged.

## 9. Documentation and operations

Updated architecture/cache-aware-context.md with the canonical request
boundary, chronology rule, diagnostic-vs-provider distinction, and compound
identity behavior. Static core-boundary and scheduler-bypass guards passed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Live provider cache-hit evidence was not collected locally | No correctness impact; provider credentials/network are not required for closure | Optional manual observation when a provider run is available. |

## 11. Roadmap disposition

Milestone 009 is closed. Its strict closure satisfies M010's stated
dependency along with already closed M003 and M006. M010 is promoted to
ready; M011 remains blocked on M010 and the remaining predecessor set.

## 12. Registry updates

- Removed M009 from dependency-ready work and recorded it under recently
  closed work.
- Updated the subsystem roadmap and implementation plan to reflect M009
  implementation/closure.
- Promoted M010 ACP v1 daemon/projection adapter to ready.
- Kept M011 blocked because M010 is not yet closed.
