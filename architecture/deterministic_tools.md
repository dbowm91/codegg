# Deterministic Tools (eggsact)

In-process deterministic correctness utilities backed by the `eggsact` crate. These provide compile-time-guaranteed validation, comparison, and inspection operations that never call external services.

## Overview

**Location**: `src/tool/deterministic.rs` (wrapper), `src/eggsact/adapter.rs` (runtime)

**Key Responsibilities**:
- Provide deterministic text comparison, diffing, and validation
- Offer config format validation (JSON, TOML)
- Perform security-oriented text inspection (hidden chars, confusables, prompt injection)
- Support harness-side preflight checks before mutating operations
- All operations are pure functions — no I/O, no network, no side effects

## Architecture

```
ToolRegistry
    │
    ├── EggsactTool (src/tool/deterministic.rs)
    │       └── calls EggsactRuntime::call_json()
    │               └── wraps eggsact::agent::ToolRegistry (in-process)
    │
    └── PreflightService (src/preflight/service.rs)
            └── calls EggsactRuntime::call_json() directly
                    └── same runtime, different audience ("harness")
```

The eggsact runtime is shared between model-facing deterministic tools and harness-internal preflight checks. The `audience` parameter distinguishes them:
- `"model"` — tool calls visible to the model (registered in ToolRegistry)
- `"harness"` — internal preflight calls (never appear as tool calls)

## Trust and Provenance

All eggsact tools use `LocalTrusted` provenance:

```rust
ToolProvenance {
    backend: "native",
    implementation: "eggsact/<tool_name>",
    trust: ToolTrust::LocalTrusted,
    ...
}
```

This reflects that eggsact operations are deterministic, have no side effects, and run entirely in-process.

## Tool Catalog

### Always-Visible Tools (8)

These tools are exposed to the model via `expose_in_definitions = true`:

| Tool | Description | Category |
|------|-------------|----------|
| `text_equal` | Compare two strings for equality under various modes (raw, normalized, casefolded, trimmed) | ReadOnly |
| `text_diff_explain` | Explain why two strings differ with Unicode-aware span analysis | ReadOnly |
| `text_replace_check` | Check whether a text replacement would apply cleanly before editing | ReadOnly |
| `validate_json` | Validate JSON syntax and report precise parse errors with line/column | ReadOnly |
| `validate_toml` | Validate TOML files and report parse errors with line/column | ReadOnly |
| `command_preflight` | Analyze a shell command before execution: parse argv, detect features, find risk patterns | ReadOnly |
| `path_normalize` | Normalize a filesystem path: collapse dot segments, resolve components | ReadOnly |
| `text_security_inspect` | Security-oriented text hygiene: detect hidden chars, confusables, prompt injection | ReadOnly |

### Deferred / Contextual Tools (5)

These tools are discoverable via `tool_search` but not shown by default:

| Tool | Description |
|------|-------------|
| `text_inspect` | Inspect a string for hidden characters, Unicode confusables, mixed scripts |
| `config_preflight` | Validate generated config text. Auto-detects format and runs appropriate validator |
| `identifier_inspect` | Inspect identifiers for validity and collisions |
| `structured_data_compare` | Compare structured config/data output (JSON) |
| `text_fingerprint` | Compute a deterministic SHA-256 fingerprint of text |

Deferred tools use `expose_in_definitions = false` and `defer_loading = true`. They are registered in the ToolCatalog but not sent to the model in tool definitions. The model can discover them via `tool_search`.

## Registration Flow

```
EggsactRuntime::new(config)
    │
    ├── Creates eggsact::agent::ToolRegistry with profile
    │
    └── Returns EggsactRuntime (owns registry)
            │
            └── build_eggsact_tools(runtime)
                    │
                    ├── Always-visible → ToolRegistry::with_options()
                    └── Deferred → ToolCatalog (discoverable via tool_search)
```

Key points:
- `EggsactRuntime::new()` is fallible — if it fails, deterministic tools are silently skipped
- Registration happens in `ToolRegistry::with_options()` (the authoritative constructor)
- The runtime is constructed from `DeterministicToolsRuntimeConfig` resolved by `integrated_config::resolve_integrated_config()`

## Configuration

### Schema (`[deterministic_tools]` in opencode.json)

```toml
[deterministic_tools]
enabled = true                    # master switch
backend = "native"                # "native" | "disabled"
profile = "codegg_core"           # eggsact profile: "codegg_core" | "codegg_core_min" | "default" | "full"
model_audience = "model"          # audience for model-facing tools
harness_audience = "harness"      # audience for preflight checks
expose_expert_tools = false       # expose deferred tools to model
max_output_chars = 12000          # truncation limit for tool output
```

### Profile Selection

The `profile` field controls which eggsact tools are available:
- `codegg_core` — curated subset for code analysis (default)
- `codegg_core_min` — minimal subset
- `default` — eggsact's default profile
- `full` — all available eggsact tools

### Runtime Config Resolution

`DeterministicToolsRuntimeConfig` is resolved from the schema by `integrated_config::resolve_integrated_config()` in `src/tool/integrated_config.rs`. The resolved config is passed through `ToolRegistryOptions` to `with_options()`.

## Integration with Preflight

The deterministic tools and the preflight system share the same eggsact runtime but serve different purposes:

| Aspect | Deterministic Tools | Preflight |
|--------|-------------------|-----------|
| Visibility | Model-facing (registered in ToolRegistry) | Harness-internal (not in ToolRegistry) |
| Purpose | Expose eggsact capabilities to the model | Validate before tool execution |
| Interface | `Tool::execute()` via ToolRegistry | `PreflightService::check_*()` methods |
| Audience | `"model"` | `"harness"` |
| Error handling | Returns `ToolError` | Returns `Allow` (fail-open) |

The preflight service (`src/preflight/service.rs`) calls `EggsactRuntime::call_json()` directly, bypassing the ToolRegistry to avoid recursive tool execution.

## Tests

### Unit Tests
- `format_response` — response formatting
- `to_structured_result` — structured result conversion
- `EggsactConfig` defaults

### Integration Tests
- All 8 always-visible tools with real eggsact calls
- All 5 deferred tools with real eggsact calls
- Provenance tagging (backend, implementation, trust)
- Audience filtering (model vs harness)
- Output truncation at `max_output_chars`
- Deferred tools not in default definitions but discoverable via `tool_search`

### Test Matrix (Phase 7)
- **Eggsact adapter**: Unit tests for formatting, conversion, defaults. Integration tests for all tools, provenance, audience, truncation.
- **Harness preflight**: Integration tests for all check methods with real eggsact calls. Policy mode tests for off/observe/warn/block_on_definite.
- **Tool registry**: Tests verifying deferred tools are hidden, descriptions imply no mutation, disabled backend hides wrappers.

## File Structure

```
src/tool/
├── deterministic.rs      # EggsactTool wrapper, build_eggsact_tools()
├── mod.rs                # Registration in with_options()
└── integrated_config.rs  # DeterministicToolsRuntimeConfig resolution

src/eggsact/
├── mod.rs                # Re-exports
└── adapter.rs            # EggsactRuntime wrapping eggsact::agent::ToolRegistry

src/preflight/
├── mod.rs                # Re-exports
└── service.rs            # PreflightService using same runtime
```

## See Also

- [tool.md](tool.md) — Tool registry, registration flow, ToolCategory
- [preflight.md](preflight.md) — Harness-side preflight integration
- [native_crates.md](native_crates.md) — Eggsact crate boundary and provenance model
- `crates/codegg-config/src/schema.rs` — `DeterministicToolsConfig` schema
