# Phase 6: Backend Configuration and Policy Unification

## Goal

Unify Codegg configuration and runtime policy for eggsearch-backed evidence tools and eggsact-backed deterministic/preflight tools. All production entry points should resolve the same backend configuration, register the same intended tools, and report the same backend state.

This phase prevents split-brain behavior between CLI, TUI, daemon, tests, and future frontends.

## Current problem to avoid

Codegg already has multiple constructors and startup paths. Some registry constructors preserve loaded config, while older/default constructors may drop backend settings. As the eggsearch and eggsact integration expands, any path that bypasses the resolved config can silently register the wrong tool surface or use the wrong fallback behavior.

## Scope

- Extend config schema for expanded search/evidence tools.
- Add deterministic/preflight backend config.
- Ensure all registry construction uses resolved config in production.
- Ensure daemon/TUI/CLI startup paths bootstrap eggsearch consistently.
- Ensure eggsact profile/audience/preflight policy resolves once and is passed into the registry/preflight service.
- Add diagnostics for effective config.

## Implementation steps

### 1. Extend backend domains

Current per-domain tool backend config covers selected domains such as LSP, security, and context. Extend the schema to cover new domains without making config unwieldy.

Suggested domains:

```toml
[tool_backends.evidence]
backend = "mcp"              # eggsearch-backed by default
server_name = "eggsearch"
fallback_to_native = false
expose_raw_mcp_tools = false

[tool_backends.deterministic]
backend = "native"           # eggsact-backed by default
profile = "codegg_core_min"
expose_expert_tools = false

[tool_backends.preflight]
backend = "native"           # eggsact harness-side checks
enabled = true
mode = "warn"
```

If adding arbitrary fields to `ExternalToolBackendConfigSchema` would make it too generic, add dedicated sections:

- `[search]` for eggsearch/evidence.
- `[deterministic_tools]` for model-facing eggsact wrappers.
- `[preflight]` for harness-side eggsact calls.

Either shape is acceptable as long as resolution is centralized.

### 2. Centralize resolution

Create a single resolver module that converts raw config into runtime structs.

Suggested structs:

```rust
pub struct EvidenceBackendRuntimeConfig { ... }
pub struct DeterministicToolsRuntimeConfig { ... }
pub struct PreflightRuntimeConfig { ... }
pub struct IntegratedToolRuntimeConfig { ... }
```

The resolver should:

- Fill defaults.
- Validate enum strings.
- Clamp unreasonable caps/timeouts.
- Resolve server names.
- Resolve eggsact profiles and audiences.
- Produce warnings for unknown profiles or unsupported backend values.

### 3. Update `ToolRegistryOptions`

Add fields for deterministic and preflight runtime configuration, or one aggregate runtime config.

Example:

```rust
pub struct ToolRegistryOptions {
    ...
    pub integrated_tools: IntegratedToolRuntimeConfig,
    pub preflight_service: Option<Arc<PreflightService>>,
}
```

Avoid recomputing config inside individual tools. Tool wrappers should receive the already-resolved config or an adapter initialized with it.

### 4. Audit registry constructors

Audit all `ToolRegistry` constructors and production call sites:

- `with_defaults`
- `with_config`
- `with_session_defaults`
- `with_session_config_defaults`
- daemon session construction
- TUI session construction
- tests and harness utilities

Production paths should use config-preserving constructors. Test-only constructors may keep all-native defaults, but the docs should label them clearly.

### 5. Bootstrap consistency

Verify `bootstrap_search_backend` is called in every production entry point that can execute search/evidence tools:

- CLI main path.
- TUI path.
- daemon/core path.
- exec/headless path.
- server mode if applicable.

If expanded evidence tools depend on the same state slots as `websearch`/`webfetch`, no new global state should be introduced unless necessary.

### 6. Raw MCP exposure policy

Preserve the default that raw eggsearch MCP tools are hidden. Extend filtering tests to ensure the expanded eggsearch tools remain hidden unless explicitly configured.

If a user enables raw MCP exposure, Codegg should still keep native wrappers registered. Raw exposure should be additive, not a replacement for stable Codegg tools.

### 7. Eggsact profile policy

Resolve eggsact model-facing and harness profiles separately.

Suggested defaults:

- Model profile: `codegg_core_min` or `codegg_core`.
- Harness profile: `codegg_preflight` plus domain-specific profiles where needed.
- Debug profile: only when explicit debug config is enabled.

Unknown profile names should fail config validation or warn and fall back to a safe default. Do not silently use `full` for model-facing tools.

### 8. Diagnostics

Extend `/tool-backends` or add a related diagnostic to report:

- Effective eggsearch backend and server.
- Eggsearch expanded wrapper availability.
- Raw MCP exposure status.
- Eggsact backend status.
- Eggsact model profile.
- Eggsact harness profile.
- Preflight mode.
- Number of model-visible eggsact wrappers.
- Number of deferred eggsact wrappers.
- Whether config-preserving registry construction was used.

## Validation

Add tests for:

- Default config resolves to eggsearch for evidence and eggsact native for deterministic tools if the dependency is available.
- Disabled evidence backend hides or disables evidence tools correctly.
- Disabled deterministic backend hides eggsact wrappers.
- Preflight observe/warn/block modes resolve correctly.
- Unknown eggsact profile is rejected or safely downgraded with a warning.
- Production registry construction preserves backend config.
- Raw MCP exposure false hides `mcp__eggsearch__...` tools.
- Raw MCP exposure true includes raw MCP tools without removing native wrappers.

## Acceptance criteria

- All production paths use a centralized resolved runtime config.
- Expanded eggsearch and eggsact behavior is consistent across CLI, TUI, daemon, and headless execution.
- Diagnostics show the effective backend state clearly.
- No production path silently reverts to all-native defaults when user config requested eggsearch or eggsact behavior.

## Risks

The main risk is config sprawl. Keep advanced knobs available but defaulted. The ordinary user should only need to install eggsearch and leave defaults alone.
