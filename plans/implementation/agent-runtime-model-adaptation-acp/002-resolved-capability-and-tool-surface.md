# Agent Runtime, Model Adaptation, and ACP Milestone 002 — Resolved Capability and Tool Surface

Status: blocked — requires Milestone 001 closure

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-002--resolved-capability-and-tool-surface`

Long-term requirements:

- `plans/000-long-term-specification.md#8.7-project-authorization`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: infrastructure/invariant

## 1. Objective

Create one typed `ResolvedToolSurface` and companion `AgentCapabilitySet` for each root or child turn. The resolved surface must be the single source of truth for prompt descriptions, provider tool definitions, permission checks, tool execution, delegation eligibility, context-palette policy, model-adapter aliases, and telemetry.

This milestone removes label-driven and call-site-specific tool filtering. It must prevent the current class of errors where a prompt advertises a tool that was filtered out, a provider receives a schema without a runtime backend, a read-only agent loses safe delegation merely because of its role name, or a model-specific alias bypasses canonical permission/provenance handling.

## 2. Dependencies

Hard dependency:

- Milestone 001 must close with one canonical prompt compiler and deterministic agent resolution.

Interface dependencies already available:

- existing `ToolRegistry`, `ToolDefinition`, `ToolCategory`, permission checker, plan-mode filtering, task tool runtime, model profiles, context-policy palette reduction, MCP registration, and plugin tool hooks;
- immutable asset/turn context and daemon-owned tool execution.

Milestone 007 will add declarative model adapters. This milestone must provide the canonical/wire-name seam but must not implement adapter TOML parsing yet.

## 3. Current implementation evidence

The implementation agent must re-audit current code, including:

- `ToolRegistry::with_config`, session tool factories, and descendant `ToolRegistry` construction;
- permission rules and `tool_category_for_name`;
- plan-mode and read-only filters;
- `is_read_only_agent` and `read_only_blocked_tools` in `src/agent/worker.rs`;
- request-level denied tools and allowed paths;
- model-profile preferred/disabled tools and max parallel tools;
- MCP tool naming and dispatch;
- context-policy palette reduction and `base_request_tools`;
- task tool construction when a spawner is or is not present;
- prompt tool lists and tool-definition caching.

Known baseline defects include distributed filtering decisions, role/name-based inference, and possible disagreement between declared permissions, registered implementations, provider schemas, and prompt content.

## 4. Invariants

- Canonical internal tool identity is stable across models/providers.
- Wire aliases are resolved before permission checking and execution.
- A tool can be advertised only when a callable backend and permission/capability path exist.
- A backend can execute only through the canonical registry/broker path with normal provenance and policy.
- Prompt, schema, permission, execution, and telemetry consume the same resolved surface fingerprint.
- Tool-palette reduction starts from the immutable unreduced surface and can restore it.
- Agent roles influence defaults/prompts but do not implicitly determine authority.
- Read-only and delegation capability remain independent.
- Plugins and MCP tools cannot shadow canonical names ambiguously.
- Child surfaces can only be equal to or narrower than the parent ceiling.

## 5. Scope

### In scope

- Define typed capabilities such as filesystem read/write, shell read/mutate, Git read/write, network research, delegation, todo/goal management, commit, image, and terminal use.
- Resolve capabilities from agent definition, mode, session/config permissions, hard policy, parent ceiling, workspace/path policy, runtime backend availability, and plan mode.
- Define `ResolvedToolSurface` containing:
  - canonical tool identity;
  - actual backend/implementation kind;
  - category and mutation/risk metadata;
  - effective permission summary;
  - canonical-to-wire and wire-to-canonical maps;
  - inclusion/omission reason;
  - provider schema;
  - required/never-reduce flags;
  - stable fingerprint.
- Centralize filtering for root and descendant turns.
- Make task/delegation inclusion depend on a functional spawner plus delegation capability.
- Make tool-definition caching key on the surface fingerprint and model/adapter seam.
- Route context-palette reduction through the resolved surface.
- Detect alias collisions, missing backends, schema/backend mismatches, and permission contradictions early.
- Add bounded diagnostics exposing names, categories, aliases, backend kinds, and omission reasons without secret arguments.

### Out of scope

- Functional nested delegation implementation beyond exposing the correct capability/spawner seam.
- TOML adapter parsing and exact model aliases.
- Specialized security/research workflows.
- Replacing the permission checker or canonical tool broker.
- Broad tool API redesign.
- New dynamic plugin authority.

## 6. Required production changes

### Domain types

Introduce dependency-safe types near the agent/tool contract boundary. Avoid importing full runtime services into protocol/config crates.

Suggested shape:

```rust
pub struct AgentCapabilitySet {
    pub filesystem_read: bool,
    pub filesystem_write: bool,
    pub shell_readonly: bool,
    pub shell_mutating: bool,
    pub git_read: bool,
    pub git_write: bool,
    pub network_research: bool,
    pub delegate: bool,
    pub manage_todos: bool,
    pub manage_goals: bool,
    pub terminal: bool,
    pub image: bool,
}

pub struct ResolvedToolSurface {
    pub tools: Vec<ResolvedTool>,
    pub canonical_to_wire: BTreeMap<String, String>,
    pub wire_to_canonical: BTreeMap<String, String>,
    pub fingerprint: String,
    pub omissions: Vec<ToolOmission>,
}
```

Exact fields may differ, but the result must remain inspectable and deterministic.

### Resolution pipeline

Resolve in a documented order:

1. registered canonical implementations;
2. runtime/backend availability;
3. hard runtime restrictions;
4. principal/session/config policy seam;
5. parent authority ceiling for children;
6. agent definition and mode;
7. workspace/path scope;
8. plan-mode restrictions;
9. model/profile/adapter preference seam;
10. conservative context-palette selection.

A later layer may narrow but not widen a hard or parent restriction. Preserve the reason for every omission.

### Prompt/provider/runtime integration

- Prompt compiler receives `ResolvedToolSurface`, not independently assembled names.
- Provider request definitions are generated from the same surface.
- Incoming wire names are normalized to canonical names before permission checking.
- Execution uses the canonical name and records both canonical/wire names in bounded diagnostics.
- Tool result messages use the provider-expected wire identity where protocol semantics require it while retaining canonical internal provenance.

### Plugins and MCP

- Namespace MCP tools deterministically.
- Reject ambiguous reverse aliases.
- Ensure plugin tools declare category/risk/backend metadata.
- Do not permit a plugin or MCP server to replace a canonical mutating tool without explicit configuration and an accepted authority contract.

### Storage/protocol

No durable migration is expected. Additive internal event/projection diagnostics may carry a surface fingerprint and selected tool count; do not expose complete schemas unless explicitly requested through a bounded diagnostic surface.

## 7. Ordered work packages

### A — Inventory and failing fixtures

- inventory every tool-filter/schema-building path;
- create fixtures demonstrating prompt/schema/backend disagreement, role-driven task removal, missing spawner advertisement, and palette-reduction restoration;
- define canonical capability and omission vocabulary.

### B — Capability resolution

- map existing permission/mode/tool-category state into typed capabilities;
- remove role/name heuristics from execution authority;
- define parent-ceiling intersection API for Milestone 003;
- add deterministic diagnostics.

### C — Tool surface construction

- construct one surface from registry/backend/policy inputs;
- detect collisions and missing backends;
- produce stable definitions and fingerprint;
- preserve base/full surface independently from reduced selection.

### D — Production integration

- migrate prompt compiler, provider definitions, permission/execution normalization, descendants, MCP/plugin dispatch, and tool cache keys;
- remove duplicate filtering paths or turn them into thin calls to the resolver;
- ensure `task` is omitted when no spawner exists and retained for read-only delegators when allowed.

### E — Documentation and guards

- document canonical versus wire names, capability resolution order, and omission diagnostics;
- add one narrow static guard preventing new role/name-based execution filters or duplicate provider schema assembly outside the resolver, if a stable guard is feasible.

## 8. Failure, cancellation, restart, and contention semantics

- Surface construction fails before provider invocation on alias collision, missing required backend, or invalid schema.
- Optional unavailable tools are omitted with diagnostics rather than causing turn failure.
- Surface state is immutable for one provider call; changes in MCP/plugin availability are applied on the next safe rebuild.
- Cancellation during dynamic MCP discovery returns no partially published surface.
- Concurrent turns may share immutable definitions/fingerprints but must not share mutable omission or call counters unsafely.
- Palette reduction failure or empty selection restores the base surface and applies bounded backoff.

## 9. Compatibility

- Canonical tool names used by permissions, plugins, telemetry, and native clients remain stable.
- Existing provider schemas remain equivalent absent an adapter alias.
- Existing model-profile preferred/disabled tool settings become inputs to the resolver rather than separate mutation paths.
- Existing plan mode remains behaviorally compatible while becoming capability-driven.
- Existing custom agent permission maps continue to work through typed resolution.

## 10. Required tests

Focused:

- capability intersection and no-widening properties;
- read-only plus delegation capability;
- functional/missing task spawner behavior;
- canonical/wire alias round trip;
- collision and ambiguous reverse alias rejection;
- permission/schema/backend agreement;
- plan-mode surface;
- MCP/plugin namespace behavior;
- deterministic fingerprint independent of map iteration;
- base-surface restoration after reduction.

Production-shaped:

- root build turn, plan turn, read-only security child, research child, and custom inherited agent surfaces;
- provider emits an aliased tool call that executes through canonical permission/provenance;
- unavailable MCP server removes only its tools and leaves native tools intact.

Negative/security:

- alias cannot map a read-only name to a mutating canonical tool without normal permission checking;
- child surface cannot include a parent-denied tool;
- plugin/MCP cannot shadow `bash`, `git`, `write`, or `task` silently;
- diagnostics omit secret-bearing schemas/arguments.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test tool::
cargo test permission::
cargo test agent::policy
cargo test --test subagent
cargo test --test agent_loop_harness
cargo check --workspace
```

Run one broad local library suite at milestone handoff. Do not add a broad provider matrix or release automation.

## 12. Acceptance criteria

- One immutable resolved surface is authoritative for prompt, schema, permission, execution, and telemetry.
- Roles/names no longer directly determine runtime authority.
- Canonical/wire names are distinct and reversible.
- Tool/backend/schema mismatches fail or omit before provider execution.
- Read-only agents can retain safe bounded delegation capability.
- Parent-ceiling intersection is ready for Milestone 003.
- Context-palette reduction is base-derived and restoration-safe.
- Existing native tools and providers remain operational.

## 13. Stop conditions

Stop if:

- canonical tool renaming would break a public native protocol without a compatibility plan;
- plugin/MCP shadowing requires a new public plugin authority decision;
- parent authority cannot be represented without implementing the full team authorization subsystem;
- provider-specific transforms exceed the generic alias seam and belong to Milestone 007;
- resolving tool availability requires creating a second scheduler or broker.

## 14. Closure evidence

Include:

- before/after inventory of filtering/schema paths;
- capability intersection matrix;
- canonical/wire mapping fixtures;
- prompt/schema/backend agreement evidence;
- read-only delegator fixture;
- palette restoration evidence;
- focused and broad local command results;
- remaining compatibility seams and closure recommendation.
