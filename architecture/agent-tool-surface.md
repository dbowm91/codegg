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
| `src/agent/loop.rs:482` | `apply_tool_exposure_filter()` — applies mode + disabled_tools |
| `src/tool/mod.rs` | `ToolRegistry` — canonical tool definitions |
| `src/permission/mod.rs` | `tool_category_for_name()` — maps tool names to categories |

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
        → determine category (ReadOnly/SafeMutating/Mutating/ShellExec)
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

12 capability kinds model the execution authorities a tool grants:

```
FilesystemRead, FilesystemWrite, ShellReadonly, ShellMutating,
GitRead, GitWrite, NetworkResearch, Delegate, ManageTodos,
ManageGoals, Terminal, Image
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

### Parent Ceiling

For subagent turns, an `AgentCapabilitySet` ceiling is passed in. The
surface intersects its capabilities with the ceiling, removing any tool
whose required capabilities exceed the parent's authority. This prevents
subagents from gaining capabilities the parent does not have.

### Plan Mode Filtering

When plan mode is active, only these tools are allowed:
`read`, `glob`, `grep`, `list`, `codesearch`, `webfetch`, `lsp`, `skill`,
`todoread`, `todowrite`, `bash`, `plan_enter`, `plan_exit`, `tool_search`.

All other tools are omitted with `ToolOmissionReason::PlanMode`.

### Context Palette Reduction

`ResolvedToolSurface::reduce(max)` narrows the unreduced surface to at
most `max` tools, always keeping `required` and `never_reduce` tools
(currently: `read`, `tool_search`). A failed or empty reduction can
restore `definitions()` without reconstructing registry state.

### MCP Tools

MCP tool names are namespaced (`mcp__server__tool`) and therefore cannot
shadow native tools. They are added by `AgentLoop` after the native
surface is resolved, before provider dispatch.

## Key Types & APIs

### ResolvedToolSurface (`src/agent/tool_surface.rs:131`)

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

### ResolvedTool (`src/agent/tool_surface.rs:120`)

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

### Capability (`src/agent/tool_surface.rs:14`)

```rust
pub enum Capability {
    FilesystemRead, FilesystemWrite, ShellReadonly, ShellMutating,
    GitRead, GitWrite, NetworkResearch, Delegate, ManageTodos,
    ManageGoals, Terminal, Image,
}
```

### AgentCapabilitySet (`src/agent/tool_surface.rs:32`)

Typed authority summary with `allows(cap)`, `intersect(ceiling)`,
`capabilities()` methods. Independent of agent names and roles — labels
select prompts, never execution authority.

### ToolOmission (`src/agent/tool_surface.rs:114`)

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

### SurfaceError (`src/agent/tool_surface.rs:141`)

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
| `from_registry()` | `tool_surface.rs:151` | Resolve from ToolRegistry |
| `from_registry_with_aliases()` | `tool_surface.rs:168` | Resolve with provider aliases |
| `resolve()` | `tool_surface.rs:207` | Resolve from policy-filtered definitions |
| `resolve_with_aliases()` | `tool_surface.rs:228` | Full resolution with aliases |
| `definitions()` | `tool_surface.rs:335` | Extract ToolDefinition list |
| `reduce()` | `tool_surface.rs:342` | Context palette reduction |
| `canonical_name_for_wire()` | `tool_surface.rs:360` | Wire→canonical lookup |

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

## Related Docs

- [agent.md](agent.md) — AgentLoop, execution policy, tool exposure
- [model-adapters.md](model-adapters.md) — adapter tool aliases
- [permission.md](permission.md) — permission system
- [tool.md](tool.md) — ToolRegistry, Tool trait
