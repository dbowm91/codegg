# Resolved Agent Tool Surface

## Purpose

Each agent turn resolves one immutable `ResolvedToolSurface` that
determines the exact set of tools advertised to the LLM. The surface
is built from registered tool definitions after native plan/model
exposure filtering and before provider deferral. It makes the
advertised surface deterministic and provides one auditable input for
prompt and schema construction.

## Where It Lives

| File | Role |
|------|------|
| `src/agent/tool_surface.rs` | `ResolvedToolSurface`, `Capability`, `AgentCapabilitySet`, resolver logic |
| `src/agent/policy.rs` | `ToolExposureMode`, `ExecutionPolicy` — drives which tools enter the surface |
| `src/agent/request_preparation.rs` | `apply_tool_exposure_filter()` — applies mode + disabled_tools (palettes owned by disclosure) |
| `src/tool/disclosure.rs` | **Canonical disclosure source (M002)**: `ToolDisclosure`, `CORE/CURATED/MINIMAL/PLAN` palettes, role overrides |
| `src/tool/mod.rs` | `ToolRegistry` — canonical tool definitions |
| `src/tool/catalog.rs` | Live shared registration metadata, including native category/disclosure semantics |
| `src/permission/mod.rs` | Conservative name fallback for permission checks when no native tool instance exists |

## How It Works

### Resolution Pipeline

```
ToolRegistry::list()
  → filter by expose_in_definitions()
  → ToolDefinition { name, description, parameters, defer_loading }
  → ResolvedToolSurface::from_registry_with_aliases(
        registry, denied, disabled, plan_mode,
        parent_ceiling, wire_to_canonical_aliases)
    → for each definition:
        → resolve canonical name (alias map or identity)
        → use the registered Tool::category() for native tools (external/MCP names use a conservative fallback)
        → determine backend (Native/Mcp/Shell)
        → check omission: Denied | PlanMode | DisabledByModel |
          NonCallable (task without spawner) | ParentCeiling
        → add capabilities from tool name + category
        → build canonical↔wire alias maps
    → intersect capabilities with parent ceiling
    → retain only tools whose capabilities are allowed
    → compute SHA-256 fingerprint
  → ResolvedToolSurface { tools, aliases, capabilities, fingerprint, omissions }
```

### Canonical vs Wire Names

- **Canonical name**: Stable identity used by permission checks, broker
  execution, and prompt construction (e.g., `bash`).
- **Wire name**: Provider-facing name sent in the API request. May differ
  from canonical via adapter aliases (e.g., MiniMax renames `bash` → `shell`).
- The resolver keeps both directions. Aliases are applied before omission
  and capability checks.

### Capability System

15 capability kinds model the execution authorities a tool grants:

```
FilesystemRead, FilesystemWrite, ShellReadonly, ShellMutating,
GitRead, GitWrite, NetworkResearch, Delegate, ManageTodos,
ManageGoals, ManageWorkOrders, Terminal, Image, MemoryRead,
PluginExecute
```

Each tool maps to one or more capabilities:
- `read` → `FilesystemRead`
- `write`/`edit`/`apply_patch` → `FilesystemWrite`
- `bash` → `ShellReadonly` + `ShellMutating`
- `git` → `GitRead` + `GitWrite`
- `task` → `Delegate` (its own authority, not filesystem mutation)
- `websearch`/`webfetch` → `NetworkResearch`
- `terminal` → `Terminal`
- `image` → `Image`
- `todowrite`/`todoread` → `ManageTodos`
- Goal/WorkPlan state tools → `ManageGoals`
- `work_order` → `ManageWorkOrders` (not filesystem write)
- `memory_search`/`memory_get` → `MemoryRead`
- plugin-executing tools → `PluginExecute`

### Parent Ceiling

For subagent turns, a host-minted `ParentToolAuthoritySnapshot`
(`src/agent/parent_authority.rs`) is enforced — never a bare
capability set and never `None` on the model-driven path. The worker
validates the snapshot (integrity fingerprint, expiry, session scope,
MCP generation freshness) and resolves the child surface via
`ResolvedToolSurface::from_registry_with_parent_authority` /
`resolve_with_parent_authority`, which intersect **both** the exact
eligible canonical names and the typed capability bits: a tool sharing
a capability bit with an eligible tool is still denied when its name is
absent (reported as `ParentCeiling`), and an eligible name whose
capabilities exceed the ceiling is still denied. Wire aliases are
canonicalized before membership checking; alias collisions fail the
whole resolution. The surface fingerprint binds the parent snapshot
fingerprint, so parent generation/policy changes invalidate descendant
cache identity. Nested children re-narrow through
`narrow_for_child`, retaining the ancestry bound transitively.

Carriage: `task` execution resolves the snapshot from the live
execution context (freshest, minted at delegation time by
`AgentLoop::mint_delegation_ceiling`) or the registration-time
child-effective snapshot, validates it, and attaches it to the
`SubAgentRequest` plus the additive `parent_authority` JSON field on
scheduler `Subagent`/`SubagentRun` payloads — never from model
arguments. Missing/invalid on the model-driven path fails closed;
explicit root/system delegations (research, security review, legacy
payloads) carry `None` with existing denied/filtered behavior.
Guard: `scripts/check_descendant_tool_ceiling.py`.

### Plan Mode Filtering

Plan-mode surface is owned by `src/tool/disclosure.rs::PLAN_ALLOWED`
(single source consumed by both `filter_tools_for_model` and the resolved
surface). When plan mode is active, only these 18 tools are allowed
(`PLAN_ALLOWED`, `src/tool/disclosure.rs:262-280`): `read`, `glob`, `grep`,
`list`, `codesearch`, `repo_search`, `webfetch`, `lsp`, `skill`, `todoread`,
`todowrite`, `bash`, `plan_enter`, `plan_exit`, `tool_search`,
`mcp_resource_search`, `mcp_resource_read`.

All other tools are omitted with `ToolOmissionReason::PlanMode`.

### Disclosure States (M002 Progressive Disclosure)

Four states distinguish capability from advertisement. One tool name maps
to one state via `disclosure_for()`; registration, invocation, and policy
remain with their existing owners.

```text
registered   — in ToolRegistry::with_options (callable via broker lookup)
advertised   — in the immediate provider definitions for this turn
discoverable — registered + policy-allowed, found via tool_search
callable     — broker + permission + contract allow this caller/input
```

`ToolCatalog::clone()` is a shared live view, not a snapshot. Session-late
registrations such as `context_read`, Goal, WorkPlan, and WorkOrder therefore
enter discovery as soon as the registry admits them. Curated and Minimal
palettes only change initial advertisement; policy-allowed omissions are
marked deferred and remain in the discovery universe. Denied, hidden,
disabled, unavailable, plan-excluded, and parent-ceiling-ineligible tools are
removed before this projection.

| State | Meaning | Mechanism |
|---|---|---|
| `Core` | Ordinary coding immediate | `defer_loading=false`, in `CORE_PALETTE` |
| `Deferred` | Registered, deferred initially, discoverable | `defer_loading=true`, in catalog + `tool_search` |
| `ProfileSpecific` | Deferred by default; immediate for named roles | `deferred` + `immediate_for_agent(name, role)` |
| `Hidden` | Never model-visible (e.g. `invalid`, M003 `git_read`/`lsp_read` program-only adapters) | `expose_in_definitions=false` |

Runtime-unavailable (disabled backend, task without spawner) is separate:
omitted as `MissingBackend`/`NonCallable`, never advertised as merely
deferred. Specialist roles (`research`, `security-review`, `verifier`)
receive their small role-appropriate immediate palette; ordinary coding
keeps the core edit-loop primitives plus `tool_search`.

Wire honesty (M004): palette states above are host-side visibility. The
actual bytes per provider surface are selected by the qualified
`ToolDisclosureWireMode` (`qualify_tool_disclosure`): generic Chat
drops `defer_loading` while sending full schemas; Responses/Messages
preserve flags but emit no provider search entry; only a reviewed
native mode changes emission, and none is qualified — production is
`FullCompatibility` with per-turn mode telemetry. Native emitters are
fixture-proven and gated (`wire::apply_native_*_tool_search`).

Contextual disclosure is host-derived during request preparation. An active
Goal promotes `goal_get`, `goal_update_progress`, and
`goal_request_completion`; an active WorkPlan promotes `work_plan_get` and
`work_plan_update_item`. WorkOrder remains separately registered and
discoverable, but is not guessed from prompt wording or conflated with
live-child `task` delegation. Context artifact recovery is actionable only
when the session has a registered `context_read` path.

`tool_search` is a two-stage discovery contract. Broad queries return compact
selection descriptors (`canonical_name`, purpose, category, risk, disclosure,
and `schema_available`) rather than embedding every matching input schema.
After selection, the model requests an exact current schema with
`{query, name, detail: "schema"}`. Both stages use the live catalog and the
same policy allow-list and hidden-tool filter, so discovery does not grant
authority or replace broker invocation.

External reconciliation (M002 agent-tool-runtime-reconciliation): third-party
MCP descriptors enter the real session `ToolCatalog` through `McpToolAdapter`
registration (single canonical origin, `src/tool/mcp_adapter.rs`), so
`tool_search` resolves exact schemas for `mcp__*` with the same
category/effect metadata as native tools. One immutable
`ToolDescriptorSnapshot` (`src/agent/tool_descriptor_snapshot.rs`) pins the
turn's MCP generation, visible revision, server origins, permitted names,
policy revision, plugin activation revision, agent identity, hidden set, and
resolved surface fingerprint; its input fingerprint (never the surface
fingerprint itself) keys `tool_def_cache`, so server add/remove/reconnect,
`tools/list_changed` (turn-boundary rebuild), plugin activation changes, and
agent changes all invalidate cached definitions. In-flight calls use the
pinned adapter and fail closed on generation mismatch; refresh affects future
turns only. Plugin `dispatch_tool_definition` output is confined by
`confine_hook_definitions` in both cache-hit and uncached paths: additions,
renames, invalid schemas and parameter widening are rejected, canonical
parameters/deferral are preserved, and only bounded description rewordings
survive. Guards: `scripts/check_mcp_broker_adapter.py`,
`scripts/check_tool_descriptor_snapshot.py`.

M003 adds deferred `git_query` for bounded status, diff, log, and branch
inspection. It delegates to the existing Git reader and cannot name mutation,
recovery, network, or raw-subcommand operations. LSP and task remain their
canonical multiplexed surfaces because the census did not justify new names
without duplicating their operation-family routing and authority.

The coding profile also exposes `verify` in Curated profiles and keeps it
discoverable for Minimal profiles. `verify` is a structured projection over
the configured Bash/command-intent path, not a second process executor; its
schema cannot express arbitrary shell syntax or mutation.

The deferred `lsp_preview_apply` tool is a direct-only, mutating exception to
the read-oriented LSP surface. It is advertised only when LSP is enabled and
the session registry has the daemon-owned checked-apply dependencies. Its
schema accepts only an opaque preview ID; host-owned registry data constructs
the canonical apply DTO. Plan, exploration, research, security-review, and
verifier agents explicitly deny it, and Tool Programs cannot invoke it.

### Context Palette Reduction

`ResolvedToolSurface::reduce(max)` narrows the unreduced surface to at
most `max` tools, always keeping `required` and `never_reduce` tools
(currently: `read`, `tool_search`). A failed or empty reduction can
restore `definitions()` without reconstructing registry state.

### MCP Tools

MCP tool names are namespaced (`mcp__server__tool`) and validated strictly
(no `__` inside server/tool components, bounded lengths) so one
server-provided name cannot shadow native/plugin tools. Each exposed raw
tool is a session-pinned `McpToolAdapter` (`src/tool/mcp_adapter.rs`)
registered in the ordinary `ToolRegistry` before broker snapshot
construction (turn/worker async pass plus the `AgentLoop::new` sync safety
net). The registry is the single canonical origin: request preparation
derives model-facing definitions from registered tools and no second
`list_filtered_tools` append path exists (M001). The adapter's conservative
`DirectOnly`/`NonIdempotent`/no-retry contract, pinned-schema check, and
`isError`-as-failure translation preserve broker hooks, grants, structured
results, provenance and artifact semantics.

## Key Types & APIs

### ResolvedToolSurface (`src/agent/tool_surface.rs:147`)

```rust
pub struct ResolvedToolSurface {
    pub tools: Vec<ResolvedTool>,
    pub canonical_to_wire: BTreeMap<String, String>,
    pub wire_to_canonical: BTreeMap<String, String>,
    pub capabilities: AgentCapabilitySet,
    pub fingerprint: String,
    pub omissions: Vec<ToolOmission>,
}
```

### ResolvedTool (`src/agent/tool_surface.rs:136`)

```rust
pub struct ResolvedTool {
    pub canonical_name: String,
    pub wire_name: String,
    pub backend: ToolBackendKind,   // Native | Mcp | Shell
    pub category: ToolCategory,     // ReadOnly | SafeMutating | Mutating | ShellExec
    pub definition: ToolDefinition,
    pub required: bool,             // read, tool_search
    pub never_reduce: bool,         // read, tool_search
}
```

### Capability (`src/agent/tool_surface.rs:15`)

```rust
pub enum Capability {
    FilesystemRead, FilesystemWrite, ShellReadonly, ShellMutating,
    GitRead, GitWrite, NetworkResearch, Delegate, ManageTodos,
    ManageGoals, ManageWorkOrders, Terminal, Image, MemoryRead,
    PluginExecute,
}
```

### AgentCapabilitySet (`src/agent/tool_surface.rs:36`)

Typed authority summary with `allows(cap)`, `intersect(ceiling)`,
`capabilities()` methods. Independent of agent names and roles — labels
select prompts, never execution authority.

### ToolOmission (`src/agent/tool_surface.rs:130`, reason enum `:120`)

```rust
pub struct ToolOmission {
    pub canonical_name: String,
    pub reason: ToolOmissionReason,
}

pub enum ToolOmissionReason {
    Denied,             // Agent permission deny
    PlanMode,           // Not in plan-allowed list
    DisabledByModel,    // Model profile disabled_tools
    MissingBackend,     // No callable backend
    NonCallable,        // task without functional spawner
    ParentCeiling,      // Exceeds parent capability ceiling
}
```

### SurfaceError (`src/agent/tool_surface.rs:157`)

```rust
pub enum SurfaceError {
    AliasCollision(String),       // Two wire names map to same canonical
    AmbiguousReverseAlias(String), // Two canonical names map to same wire
    InvalidDefinition(String),    // Empty tool name
}
```

### Key Functions

| Function | Location | Description |
|----------|----------|-------------|
| `from_registry()` | `tool_surface.rs:166` | Resolve from ToolRegistry |
| `from_registry_with_aliases()` | `tool_surface.rs:183` | Resolve with provider aliases |
| `resolve()` | `tool_surface.rs:228` | Resolve from policy-filtered definitions |
| `resolve_with_aliases()` | `tool_surface.rs:250` | Full resolution with aliases |
| `definitions()` | `tool_surface.rs:385` | Extract ToolDefinition list |
| `reduce()` | `tool_surface.rs:392` | Context palette reduction |
| `canonical_name_for_wire()` | `tool_surface.rs:410` | Wire→canonical lookup |

## Configuration Surface

| Config Key | Effect |
|------------|--------|
| `[model_profile.<model>]` | `disabled_tools` removes tools from the surface |
| `compaction.*` | Affects context budget which triggers `reduce()` |
| Adapter `tools.rename` | Maps canonical→wire names in the surface |
| Adapter `tools.arguments` | Maps argument names per tool |

## Invariants & Gotchas

- **Surface is immutable per turn**: Built once, not reconstructed.
  Later configuration refreshes affect later turns only.
- **Resolution is monotonic**: Registered definitions are narrowed by
  explicit denies, model disables, plan mode, callable-backend
  availability, and parent ceiling. Never widened.
- **Permission checker and tool broker are execution authorities**: The
  surface controls advertisement only; it does not duplicate permission
  prompts or tool execution.
- **Roles and agent names are prompt metadata only**: They never grant
  execution authority.
- **`Delegate` is its own authority**: The task tool does not grant
  filesystem mutation merely because its implementation is conservatively
  categorized as mutating.
- **Alias collisions are errors**: If two wire names map to the same
  canonical name, resolution fails with `AliasCollision`.
- **MCP names are namespaced**: `mcp__` prefix ensures no shadow of
  native tools.
- **Fingerprint is deterministic**: SHA-256 over sorted tool names,
  wire names, parameters, aliases, and capabilities. Order-independent.

## Testing

- Unit tests in `src/agent/tool_surface.rs::tests`
- Narrowest run:
  ```bash
  cargo test -p codegg --lib tool_surface
  ```

## Advisor visibility projection

An explicitly configured advisor `promote` mode adds a bounded pre-turn
visibility projection after `ResolvedToolSurface` and before provider
definitions are finalized. It can remove deferral from at most two tools
already present in the resolved deferred universe; it never registers tools,
changes schemas, grants permissions, or replaces `tool_search`. Off, observe,
failure, abstention, and budget overflow preserve the existing palette.

## Related Docs

- [agent.md](agent.md) — AgentLoop, execution policy, tool exposure
- [model-adapters.md](model-adapters.md) — adapter tool aliases
- [permission.md](permission.md) — permission system
- [tool.md](tool.md) — ToolRegistry, Tool trait

## Source Verification

Verified 2026-10-06 against source. Corrected: capability count 13 → 15
(`MemoryRead` and `PluginExecute` were missing from the prose list, and
`ManageWorkOrders` was missing from the enum code block — the enum block now
matches the source verbatim); plan-mode allowlist 15 → 18 names
(`mcp_resource_search`/`mcp_resource_read` were missing). Line refs:
`ResolvedToolSurface` `:131` → `:147`, `ResolvedTool` `:120` → `:136`,
`AgentCapabilitySet` `:32` → `:36`, `ToolOmission` `:114` → `:130`
(reason enum `:120`), `SurfaceError` `:141` → `:157`, and all seven Key
Functions rows (`:151/168/207/228/335/342/360` →
`:166/183/228/250/385/392/410`). Verified accurate: `ResolvedToolSurface`
6 fields, `ResolvedTool` 7 fields, `ToolOmission` 2 fields,
6 `ToolOmissionReason` variants, 3 `SurfaceError` variants, 15
`AgentCapabilitySet` bool fields, `PLAN_ALLOWED` ordering, and the
four disclosure states with `ProfileSpecific`.
