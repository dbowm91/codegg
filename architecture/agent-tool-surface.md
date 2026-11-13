# Resolved Agent Tool Surface

## Purpose

Each agent turn resolves one immutable `ResolvedToolSurface` that
determines the exact set of tools advertised to the LLM. The surface
is built from registered tool definitions after native plan/model
exposure filtering and before provider deferral. It makes the
advertised surface deterministic and provides one auditable input for
prompt and schema construction.

**Which tools exist is not enumerated here.** That enumeration is generated
from the registry into [tool-catalog.md](tool-catalog.md) — four declared
surfaces, one row per tool, with each hidden row carrying the reason its own
stub or gate states. This page keeps the *why*: how a surface is narrowed, what
each narrowing means, and which module owns it. A tool added to the registry
changes the catalog, not this page, and `cargo test --test tool_catalog` fails when
the catalog is stale.

## Where It Lives

| File | Role |
|------|------|
| `src/agent/tool_surface.rs` | `ResolvedToolSurface`, `Capability`, `AgentCapabilitySet`, resolver logic |
| `src/agent/policy.rs` | `ToolExposureMode`, `ExecutionPolicy` — drives which tools enter the surface |
| `src/agent/request_preparation.rs` | `apply_tool_exposure_filter()` — applies the exposure mode and the model profile's disabled-tool list (palettes owned by disclosure) |
| `src/tool/disclosure.rs` | **Canonical disclosure source (M002)**: `ToolDisclosure`, the palettes, and the plan-mode allowlist |
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
  from canonical via adapter aliases (e.g., MiniMax sends `bash` under a different
  wire name).
- The resolver keeps both directions. Aliases are applied before omission
  and capability checks.

### Capability System

`Capability` names the execution authorities a tool can grant:

```rust
pub enum Capability {
    FilesystemRead, FilesystemWrite, ShellReadonly, ShellMutating,
    GitRead, GitWrite, NetworkResearch, Delegate, ManageTodos,
    ManageGoals, ManageWorkOrders, Terminal, Image, MemoryRead,
    PluginExecute,
}
```

`tool_capabilities()` (`src/agent/tool_surface.rs`) derives the set from the
tool's name and safety category. A few families carry an authority of their own
because their category cannot express it — delegation, work-order management,
memory reads, plugin execution, and goal/work-plan state. Everything else falls
back to its category: a read-only tool reads the filesystem, a mutating one
writes it, and a shell tool carries both shell authorities. Network research,
terminal, image and git authorities are attached by name where they exceed the
category. The mapping is the source, not this page: read `tool_capabilities()` for
the authority of a specific tool.

### Parent Ceiling

For subagent turns, an `AgentCapabilitySet` ceiling is passed in. The
surface intersects its capabilities with the ceiling, removing any tool
whose required capabilities exceed the parent's authority. This prevents
subagents from gaining capabilities the parent does not have.

### Plan Mode Filtering

Plan-mode surface is owned by `src/tool/disclosure.rs::PLAN_ALLOWED`, the single
allowlist consumed by both the disclosure filter and the resolved surface. When
plan mode is active only the tools on that list survive; every other tool is
omitted with `ToolOmissionReason::PlanMode`. The list is not repeated here — the
generated catalog's `plan-mode` section *is* the list, rendered by building the
registry and applying the same constant.

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
registrations such as context artifact recovery, Goal, WorkPlan, and WorkOrder
therefore enter discovery as soon as the registry admits them. Curated and
Minimal palettes only change initial advertisement; policy-allowed omissions are
marked deferred and remain in the discovery universe. Denied, hidden,
disabled, unavailable, plan-excluded, and parent-ceiling-ineligible tools are
removed before this projection.

| State | Meaning | Mechanism |
|---|---|---|
| `Core` | Ordinary coding immediate | defer-loading off, in the core palette |
| `Deferred` | Registered, deferred initially, discoverable | defer-loading on, in the catalog + `tool_search` |
| `ProfileSpecific` | Deferred by default; immediate for named roles | deferred + `immediate_for_agent(name, role)` |
| `Hidden` | Never model-visible (internal diagnostics, program-only adapters, compat aliases) | hidden from the model's definitions |

Two hiding causes are separate from those states and are worth naming because a
reader of the catalog will meet them as `hidden (<reason>)` rows:

- **A disabled backend domain keeps its stable tool name.** `DisabledTool`
  registers under the name of the tool it replaces and reports the tool as
  unexposed, so `/tool-backends` and the catalog can name the domain while the
  model is never offered a tool whose every call is a guaranteed failure. The
  stub advertises its configured reason in the schema it hands the model, and
  that is the reason the catalog prints.
- **A program-only adapter declares it in its own contract.** The operation-
  scoped read adapters over canonical Git and LSP are broker-callable by Tool
  Programs and declare a programmatic-only caller policy; being unexposed is
  their contract, not a per-turn decision.

Runtime-unavailable (a disabled backend, a task without a spawner) is omitted as
`MissingBackend`/`NonCallable`, never advertised as merely deferred. Specialist
roles — research, security review, verifier — receive their small
role-appropriate immediate palette; ordinary coding keeps the core edit-loop
primitives plus `tool_search`.

Contextual disclosure is host-derived during request preparation. An active
Goal promotes the goal reads and progress updates, an active WorkPlan promotes
the plan reads and item updates. WorkOrder remains separately registered and
discoverable, but is not guessed from prompt wording or conflated with
live-child `task` delegation. Context artifact recovery is actionable only when
the session registry has admitted the recovery tool.

`tool_search` is a two-stage discovery contract. Broad queries return compact
selection descriptors (canonical name, purpose, category, risk, disclosure, and
whether a schema is available) rather than embedding every matching input schema.
After selection, the model requests an exact current schema with
`{query, name, detail: "schema"}`. Both stages use the live catalog and the
same policy allow-list and hidden-tool filter, so discovery does not grant
authority or replace broker invocation.

M003 adds the deferred `git_query` for bounded status, diff, log, and branch
inspection. It delegates to the existing Git reader and cannot name mutation,
recovery, network, or raw-subcommand operations. LSP and task remain their
canonical multiplexed surfaces because the census did not justify new names
without duplicating their operation-family routing and authority.

The coding profile also exposes `verify` in Curated profiles and keeps it
discoverable for Minimal profiles. `verify` is a structured projection over
the configured command-intent path, not a second process executor; its schema
cannot express arbitrary shell syntax or mutation.

The LSP preview-apply tool (`src/tool/lsp_preview_apply.rs`) is a direct-only,
mutating exception to the read-oriented LSP surface. It is advertised only when
LSP is enabled and the session registry has the daemon-owned checked-apply
dependencies — which is why it is not in the catalog's options-only sections.
Its schema accepts only an opaque preview ID; host-owned registry data constructs
the canonical apply DTO. Plan, exploration, research, security-review, and
verifier agents explicitly deny it, and Tool Programs cannot invoke it.

### Context Palette Reduction

`ResolvedToolSurface::reduce(max)` narrows the unreduced surface to at
most the limit it is given, always keeping the required and never-reduced set (`read` and
`tool_search` today). A failed or empty reduction can restore the registry's
definitions without reconstructing registry state.

### MCP Tools

MCP tool names are namespaced (`mcp__<server>__<tool>`) and therefore cannot
shadow native tools. They are added by `AgentLoop` after the native
surface is resolved, before provider dispatch.

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
    pub required: bool,
    pub never_reduce: bool,
}
```

### Capability (`src/agent/tool_surface.rs:15`)

The enum is reproduced above, under [Capability System](#capability-system).

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
| `[model_profile.<model>].disabled_tools` | Removes tools from the surface |
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
- **Delegation is its own authority**: The task tool does not grant
  filesystem mutation merely because its implementation is conservatively
  categorized as mutating.
- **Alias collisions are errors**: If two wire names map to the same
  canonical name, resolution fails with `AliasCollision`.
- **MCP names are namespaced**: an MCP tool's wire name is namespaced, so it
  cannot shadow a native name.
- **Fingerprint is deterministic**: SHA-256 over sorted tool names,
  wire names, parameters, aliases, and capabilities. Order-independent.

## Testing

- Unit tests in `src/agent/tool_surface.rs::tests`
- The generated catalog and its freshness gate:
  ```bash
  cargo test --test tool_catalog
  ```
  and the structural half that runs without tests,
  `python3 scripts/check_tool_catalog.py`.
- Narrowest run for this page's subject:
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

- [tool-catalog.md](tool-catalog.md) — the generated enumeration of the
  registry's surface, per declared option set
- [agent.md](agent.md) — AgentLoop, execution policy, tool exposure
- [model-adapters.md](model-adapters.md) — adapter tool aliases
- [permission.md](permission.md) — permission system
- [tool.md](tool.md) — ToolRegistry, Tool trait

## Source Verification

The per-tool enumeration and every count this page used to carry are gone: they
were hand-maintained, they drifted, and they now live in the generated
[tool-catalog.md](tool-catalog.md), which `cargo test --test tool_catalog` compares against
the registry. What remains is prose, and the anchors in the code blocks above
were last checked statically against `src/agent/tool_surface.rs`; a moved anchor
is a stale pointer, not a stale fact.
