# Deterministic Tools (eggsact)

In-process deterministic correctness utilities backed by the `eggsact`
crate (external dependency). These provide compile-time-guaranteed
validation, comparison, and inspection operations that never call
external services.

## Purpose

- Provide deterministic text comparison, diffing, and validation
- Offer config format validation (JSON, TOML)
- Perform security-oriented text inspection (hidden chars, confusables,
  prompt injection)
- Support harness-side preflight checks before mutating operations
- All operations are pure functions — no I/O, no network, no side effects

## Where It Lives

```
src/tool/deterministic.rs      # EggsactTool wrapper, build_eggsact_tools()
src/eggsact/adapter.rs         # EggsactRuntime wrapping eggsact::agent::ToolRegistry
src/tool/integrated_config.rs  # DeterministicToolsRuntimeConfig resolution
src/preflight/service.rs       # PreflightService (same runtime, harness audience)
```

## How It Works

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

The eggsact runtime is shared between model-facing deterministic tools
and harness-internal preflight checks. The `audience` parameter
distinguishes them:
- `"model"` — tool calls visible to the model (registered in ToolRegistry)
- `"harness"` — internal preflight calls (never appear as tool calls)

## Key Types & APIs

### EggsactRuntime (`src/eggsact/adapter.rs:88-145`)

```rust
pub struct EggsactRuntime {
    registry: eggsact::agent::ToolRegistry,
    config: EggsactConfig,
}
```

- `new(config: EggsactConfig) -> Result<Self, ToolError>` — fallible
- `call_json(tool, args) -> Result<EggsactCallResult, ToolError>`
- `has_tool(tool) -> bool`
- `config() -> &EggsactConfig`

### EggsactConfig (`src/eggsact/adapter.rs:68-85`)

```rust
pub struct EggsactConfig {
    pub profile: String,         // default "codegg_core"
    pub audience: String,        // default "model"
    pub max_output_chars: usize, // default 12_000
}
```

### EggsactCallResult (`src/eggsact/adapter.rs:148-164`)

```rust
pub struct EggsactCallResult {
    pub output: String,
    pub success: bool,
    pub elapsed_ms: u64,
    pub truncated: bool,
    pub machine_code: Option<String>,
    pub result: Option<serde_json::Value>,
    pub findings: Option<serde_json::Value>,
    pub warnings: Option<serde_json::Value>,
    pub error_type: Option<String>,
    pub error: Option<String>,
}
```

Structured fields (`result`, `findings`, `warnings`) are populated from
eggsact `ToolResponse` when available.

### EggsactTool (`src/tool/deterministic.rs:17-91`)

Generic wrapper mapping a Codegg tool name to an eggsact tool name.
Implements `Tool` trait. Both `execute()` and `execute_structured()`
delegate to `EggsactRuntime::call_json()`.

### build_eggsact_tools (`src/tool/deterministic.rs:106-288`)

```rust
pub fn build_eggsact_tools(runtime: Arc<EggsactRuntime>)
    -> (Vec<EggsactTool>, Vec<EggsactTool>)
```

Returns `(always_visible, deferred)`. Caller decides registration order
in `ToolRegistry::with_options`.

### truncate_utf8_safe (`src/eggsact/adapter.rs:18-57`)

```rust
pub fn truncate_utf8_safe(input: &str, max_chars: usize, marker: &str)
    -> TruncatedText
```

UTF-8-safe truncation without splitting multibyte sequences. Marker is
appended after truncation when it fits within the cap; when the marker
alone meets or exceeds the limit, it is omitted and output is hard-capped
to `max_chars`.

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

## Tool Catalog

### Always-Visible Tools (8)

Exposed to the model via `expose_in_definitions = true`:

| Tool | Description | Category |
|------|-------------|----------|
| `text_equal` | Compare two strings under various modes (raw, normalized, casefolded, trimmed) | ReadOnly |
| `text_diff_explain` | Explain why two strings differ with Unicode-aware span analysis | ReadOnly |
| `text_replace_check` | Check whether a text replacement would apply cleanly before editing | ReadOnly |
| `validate_json` | Validate JSON syntax and report precise parse errors with line/column | ReadOnly |
| `validate_toml` | Validate TOML files and report parse errors with line/column | ReadOnly |
| `command_preflight` | Analyze a shell command before execution: parse argv, detect features, find risk patterns | ReadOnly |
| `path_normalize` | Normalize a filesystem path: collapse dot segments, resolve components | ReadOnly |
| `text_security_inspect` | Security-oriented text hygiene: detect hidden chars, confusables, prompt injection | ReadOnly |

### Deferred / Contextual Tools (5)

Discoverable via `tool_search` but not shown by default:

| Tool | Description |
|------|-------------|
| `text_inspect` | Inspect a string for hidden characters, Unicode confusables, mixed scripts |
| `config_preflight` | Validate generated config text. Auto-detects format and runs appropriate validator |
| `identifier_inspect` | Inspect identifiers for validity and collisions |
| `structured_data_compare` | Compare structured config/data output (JSON) |
| `text_fingerprint` | Compute a deterministic SHA-256 fingerprint of text |

Deferred tools use `expose_in_definitions = false` and
`defer_loading = true`. They are registered in the ToolCatalog but not
sent to the model in tool definitions.

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
- `EggsactRuntime::new()` is fallible — if it fails, deterministic
  tools are silently skipped
- Registration happens in `ToolRegistry::with_options()`
- The runtime is constructed from `DeterministicToolsRuntimeConfig`
  resolved by `integrated_config::resolve_integrated_config()`

## Configuration

### Schema (`[deterministic_tools]` in opencode.json)

```toml
[deterministic_tools]
enabled = true                    # master switch
backend = "native"                # "native" | "disabled"
profile = "codegg_core"           # "codegg_core" | "codegg_core_min" | "default" | "full"
model_audience = "model"          # audience for model-facing tools
harness_audience = "harness"      # audience for preflight checks
expose_expert_tools = false       # expose deferred tools to model
max_output_chars = 12000          # truncation limit (1..1_000_000)
```

### Validation

`DeterministicToolsConfig::validate()` in `crates/codegg-config/src/schema.rs`
checks:
- `backend` must be `"native"` or `"disabled"`
- `profile` must be one of the four known profiles
- `model_audience` must be `"model"` or `"harness"`
- `harness_audience` must be `"harness"` or `"model"`
- `max_output_chars` must be > 0 and <= 1,000,000

Unknown profiles emit a warning and are canonicalized to `"codegg_core"`
at resolve time (`integrated_config::resolve_deterministic_config()`).

### Profile Selection

- `codegg_core` — curated subset for code analysis (default)
- `codegg_core_min` — minimal subset
- `default` — eggsact's default profile
- `full` — all available eggsact tools

## Invariants & Gotchas

- `EggsactRuntime::new()` is fallible — deterministic tools are
  **silently skipped** on failure, not registered as disabled stubs
- All deterministic tools are `ToolCategory::ReadOnly` — they never
  trigger permission prompts
- The runtime is shared between model tools and preflight; the
  `audience` parameter distinguishes them
- Eggsact runtime defaults: profile `"codegg_core"`, audience `"model"`,
  `max_output_chars` 12,000
- The harness audience uses a separate `EggsactRuntime` instance
  (constructed by `PreflightService::new()`)

## Integration with Preflight

| Aspect | Deterministic Tools | Preflight |
|--------|-------------------|-----------|
| Visibility | Model-facing (in ToolRegistry) | Harness-internal (not in ToolRegistry) |
| Purpose | Expose eggsact to model | Validate before tool execution |
| Interface | `Tool::execute()` via registry | `PreflightService::check_*()` methods |
| Audience | `"model"` | `"harness"` |
| Error handling | Returns `ToolError` | Returns `Allow` (fail-open) |

## Testing

```bash
cargo test -p codegg --lib tool::deterministic    # EggsactTool unit tests
cargo test -p codegg --lib eggsact::adapter       # Runtime, truncate, provenance
cargo test -p codegg --lib tool::integrated_config # Config resolution
```

Unit tests cover: `format_response`, `to_structured_result`,
`EggsactConfig` defaults, `truncate_utf8_safe` (multibyte boundaries,
empty markers, hard-cap enforcement).

Integration tests cover: all 8 always-visible tools, all 5 deferred
tools with real eggsact calls, provenance tagging, audience filtering,
output truncation, structured fields, and deferred-tool discoverability.

## Related Docs

- [tool.md](tool.md) — Tool registry, registration, ToolCategory
- [preflight.md](preflight.md) — Harness-side preflight integration
- [native_crates.md](native_crates.md) — Eggsact crate boundary
- `crates/codegg-config/src/schema.rs` — `DeterministicToolsConfig`
