# Agent Tool Runtime Reconciliation Roadmap

Status: active — M001–M002 closed; M003–M004 ready

Research baseline: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03` (main, 2026-10-13)

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/003-planning-process.md`

Related closed owners, not reopened:

- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md` (M001–M007 closed);
- `plans/subsystems/coding-agent-tool-surface-post-closure-evidence-polish-addendum.md` (closed);
- `plans/subsystems/agent-context-discovery-surface-roadmap.md` (closed);
- `plans/subsystems/plugin-ecosystem-interoperability-roadmap.md` (closed);
- `plans/subsystems/tool-programs-roadmap.md` (closed);
- `architecture/tool_broker.md`, `architecture/agent-tool-surface.md`, `architecture/mcp.md`, `architecture/tool.md`.

Applicable ADRs: existing authority and runtime ownership are unchanged; no new ADR is required for these adapter/correctness milestones. Stop and propose an ADR if a change would give McpService authorization authority, create a second broker, change public durable run authorization semantics, or move provider-wire control into the model.

## 1. Purpose and ownership

Make the advertised, discoverable, and callable tool universe truthful for native, plugin, and third-party MCP tools; make all model-controlled execution pass through one authorization/contract boundary; prove child authority can only narrow; and qualify effective deferral across provider wire surfaces.

Ownership remains:

- `McpService`: connection, protocol negotiation, OAuth, tool list/structured call, protocol-level cancellation.
- `ToolRegistry` + live `ToolCatalog`: session-pinned, canonical `Tool` instances and discoverable metadata.
- `ResolvedToolSurface`: deterministic per-turn authority/advertisement projection; no execution.
- `ToolBroker` + permission/approval/sandbox: tool execution authorization, contracts, output, artifact and audit normalization.
- `AgentLoop`/`ToolBatchExecutor`: permission receipts, hooks, bounded scheduling/batching, transcripts.
- `PluginService`: plugin-specific execution; `PluginToolAdapter` already joins canonical broker.
- `codegg-providers` and resolved model profiles: provider-wire encoding and **observed**, not guessed, capability support.

No new model tool names, MCP client library, broker, permission database, provider abstraction, or global mutable tool registry should be introduced.

## 2. Evidence and research

Verified in baseline code:

1. `src/agent/tool_batch.rs` splits `mcp__*` calls to `McpService::call_tool()` after permission checks, bypassing the native `ToolBroker`, normal pre/post hooks and structured result preservation. Native/tool-program/plugin paths already use the broker.
2. `src/agent/request_preparation.rs` appends raw MCP `ToolDefinition` values but `tool_search` reads the native `ToolCatalog`; allowed-name injection cannot create missing catalog entries. `McpTool` already retains input/output schemas, annotations, metadata and `McpToolCallResult` retains `is_error`, structured output and result type.
3. The `parent_ceiling` argument of `ResolvedToolSurface` exists, but production callers in `src/agent/worker.rs`, `src/agent/turn_runtime.rs`, and `src/agent/request_preparation.rs` pass `None`. Other child limitations, including sandbox, deny lists and permission inheritance, do exist: a bypass is **not** proven by this finding alone.
4. `ProviderCapabilities::for_provider` labels `openai` and `anthropic` as deferral-capable, yet `request_preparation.rs` returns immediate+deferred definitions together. `ResponsesTool::from_tool_definition` does not propagate `defer_loading`. Other OpenAI-compatible providers get the complete allowed palette. The on-wire effect of deferral requires explicit capture tests, not merely a flag.
5. `PluginToolAdapter` already registers per session before `AgentLoop`; preserve that working pattern. Tool-definition plugin hooks run after initial surface resolution: their output must not create unregistered execution authority.

External protocol references checked 2026-10-13:

- MCP tools specification, 2026-07-28: https://modelcontextprotocol.io/specification/2026-07-28/server/tools (tools/list, tools/call, schema, structured results, `isError`, listChanged notifications, per-request auth).
- OpenAI Responses tool search: https://developers.openai.com/api/docs/guides/tools-tool-search (requires actual tool-search support and correctly deferred functions/namespaces; provider/model specific).
- Anthropic tool search: https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool (deferred definitions still supplied at API boundary, provider tool references and search tool required; cache-sensitive).
- CodeGG local API adapter/contracts are controlling when external vendor docs differ; tests must capture real serialization and not infer hosted behavior from generic labels.

## 3. Invariants

- Advertisement, discovery, and invocation share an identical policy-allowed, session-pinned tool identity; hidden/denied/disabled/missing-backend tools are not resurrected by discovery.
- All direct **model-controlled** native, plugin and raw MCP tool calls cross an authorized broker/permission boundary before external side effects; no MCP special-case bypass or duplicate permission system.
- One server-provided name cannot shadow native/plugin tools; canonical `mcp__{server}__{tool}` names are validated and collision checked.
- Remote `inputSchema`, `outputSchema`, annotations, and descriptions are untrusted declarations, not proof of side-effect class, trust level, or authorization.
- Output `isError=true` is not success. `structuredContent` is retained and externally untrusted; response size, artifacts, logging and redaction obey canonical broker rules.
- Tool calls bind to the pinned active server/descriptor generation. Unexpected disconnect, replacement or tool-list drift fails closed; a refreshed catalog affects future turns, not in-flight authority.
- Child tool authority is the intersection of parent surface authority, requested child role restrictions, project/session permissions, model disables, sandbox and backend availability. Neither aliases nor tool_search nor MCP/extension changes widen that intersection.
- Retrying externally effectful tools without idempotency proof is prohibited. Cancellation/timeouts stop waiting but do not assert that the remote side effect was undone.
- Provider-native deferral and host-side visibility/palette reduction are named and measured separately. Unknown providers do not inherit capabilities on ID similarity alone.
- No new durable storage required for ordinary MCP registrations, search snapshots or provider deferral. Durable job, permission and transcript formats keep their existing owners.

## 4. Non-goals

New coding primitives, automatic third-party permission grants, trusting MCP `readOnlyHint` to downgrade authorization, wholesale agent-role redesign, full hosted OpenAI/Anthropic agents migration, bypassing network/remote-MCP restrictions, a second external search index, background tool-list polling, browser automation, remote MCP auto-install, Tool Program write/MCP promotion, or redesign of native LSP/Git/task tools.

## 5. Target architecture

```text
McpService (connected server + descriptor generation)
       -> session-pinned MCP ToolAdapter(s): Tool trait / conservative contract
       -> ToolRegistry.register -> shared ToolCatalog -> ResolvedToolSurface
       -> immediate or deferred provider projection; tool_search discovers both
       -> AgentLoop approval receipt + hooks -> ToolBroker (ALL direct model calls)
       -> MCP adapter -> McpService.call_tool_structured
       -> ToolValue {terminal status, structured, provenance, bounded artifact}
```

For child agents, transport a host-minted parent authority snapshot (with generation/fingerprint), intersect before the child's registry/model definitions, and check it again in direct invocation admission. Preserve existing child sandbox/profile/worktree restrictions.

For providers, establish a capability matrix backed by actual serialized requests and scoped model/provider qualification. Unsupported backends use explicit host-local discovery/re-request only when semantics are proven; otherwise retain the current full-allowed-schema compatibility path and report it truthfully.

## 6. Dependencies and milestones

```text
Existing ToolBroker + MCP Service + plugin adapter [closed]
      |
      v
M001 MCP canonical broker adapter + no-bypass guard [ready]
      |
      v
M002 external catalog/discovery + lifecycle coherence [sequenced; hard M001]
      |                                         \
      v                                          v
M003 descendant capability ceiling               M004 provider deferral qualification
[sequenced; hard M002]                           [sequenced; hard M002]
```

### M001 — Raw MCP execution through canonical broker
Class: invariant. One conservative `Tool` adapter per connected, exposed raw MCP tool; eliminate direct `mcp__` dispatch; preserve `isError`, structured responses, provenance and audited permission behavior. Hard dependencies already closed. Exit: executable fake MCP traversal through the broker, hooks and failure/timeout/cancellation tests; raw bypass static guard.

Plan: `plans/implementation/agent-tool-runtime-reconciliation/001-mcp-broker-transport-adapter.md`.

### M002 — Single discoverable external catalog and snapshot lifecycle
Class: capability. External tool descriptors enter the real session `ToolCatalog`, share policy/disclosure truth, and survive tool-list refresh/reconnect without stale exposure. Hard dependency: M001. Exit: two-stage `tool_search` finds policy-allowed third-party MCP tools with exact schemas in Full/Curated/Minimal; negative/refresh/generation tests.

Plan: `plans/implementation/agent-tool-runtime-reconciliation/002-external-tool-discovery-and-generation.md`.

### M003 — End-to-end descendant capability ceilings
Class: invariant. Explicitly carry parent authority/capability ceiling through durable and compatibility subagent paths to model surface **and** broker invocation. Hard dependency: M002, so MCP/plugin descendants are included in fixtures. Exit: forged/inherited/stale descendant authority tests across nested worktrees, plan mode and dynamic tools; no ambient `None` child ceiling.

Plan: `plans/implementation/agent-tool-runtime-reconciliation/003-descendant-tool-authority-ceilings.md`.

### M004 — Provider-native deferral and host fallback qualification
Class: infrastructure. Distinguish tested native tool search from host-local staged visibility. Hard dependency: M002 for complete deferred catalog identity; M003 is a soft security-review dependency. Exit: captured OpenAI Responses / Anthropic Messages / generic OpenAI-compatible wire snapshots, correct unsupported fallback, bounded prompt/token measurements and concrete negative protocol fixtures. External provider live tests are operational, not prerequisite to deterministic closure; do not claim native support without qualification.

Plan: `plans/implementation/agent-tool-runtime-reconciliation/004-provider-deferral-wire-qualification.md`.

## 7. Cross-cutting requirements

Storage: no new persistent table. Retain existing policy receipts, run/artifact stores and session transcript formats. Transport DTO changes additive only; preserve MCP clients' compatibility `call_tool()` projection.

Security: validate external names/schema sizes at protocol boundary, never honor effect hints as permissions, keep default `Mutating`/non-idempotent behavior unless host proves stricter. Preserve managed eggsearch raw-hide default and sandbox profiles. Require negative tests for tool injection and cross-project/workspace calls.

Runtime: bound lock waits, timeout, cancellation and result bytes. Never await network while holding registry/catalog write locks. Pin each agent turn to immutable catalog/descriptor revisions. Future refreshes may rebuild future turn registries; they never modify active tool permissions.

Observability: report registered/advertised/discoverable/callable counts and named omission reasons; backend and generation hashes, provider mode and broker result classification without leaking endpoints, tokens, complete tool parameters, untrusted responses or private paths.

Compatibility: canonical names and provider aliases stable; disabled native backends still disappear; no accidental exposure of raw managed eggsearch; plugin tools remain existing broker adapters. Tested fallback must preserve currently usable models.

## 8. Verification and stop conditions

Minimum focused suite: native+fake MCP broker trajectory, tool search/deferred/refresh, child capability matrix, provider wire snapshots, plugin/managed-backend negatives and existing Tool Programs regressions. Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, relevant integration tests, `scripts/verify.sh quick`, and existing static-guard scripts, recording exact environment/outcomes in each closure.

Stop and propose an ADR instead of improvising if fixing this requires replacing McpService, changing the canonical ToolBroker authority contract for all tools, persisting model-visible tool schemas as authorization grants, relying on remote annotations for privilege, or unreviewed provider protocol/connection migration. Tool execution side effects cannot be proven undone by cancellation.

## 9. Status

| Milestone | Status | Plan | Blocker |
|---|---|---|---|
| M001 | closed | `implementation/agent-tool-runtime-reconciliation/001-mcp-broker-transport-adapter.md` | None — closure `plans/closure/agent-tool-runtime-reconciliation/001-status.md` |
| M002 | closed | `implementation/agent-tool-runtime-reconciliation/002-external-tool-discovery-and-generation.md` | None — closure `plans/closure/agent-tool-runtime-reconciliation/002-status.md` (hard M001 satisfied) |
| M003 | ready | `implementation/agent-tool-runtime-reconciliation/003-descendant-tool-authority-ceilings.md` | Hard M002 satisfied; rebaseline before code change |
| M004 | ready | `implementation/agent-tool-runtime-reconciliation/004-provider-deferral-wire-qualification.md` | Hard M002 satisfied (M003 soft); rebaseline before code change |

Roadmap closes only on separate accepted closure records with traced commits, executable evidence and honest operational limitations. Prior closed milestone history is unchanged.
