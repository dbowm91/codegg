# Preflight Module

Harness-side eggsact preflight integration for automatic validation
before mutating operations. Preflight calls never appear as model-facing
tool calls.

## Purpose

- Validate edits, config writes, and shell commands before execution
- Surface severity-classified findings (Block/Warn/Annotate)
- Integrate with the eggsact deterministic tool substrate
- Operate as harness-internal only — never exposed as model-facing tools

## Where It Lives

```
src/preflight/
├── mod.rs          # Re-exports, module doc
└── service.rs      # PreflightService, types, tests

crates/codegg-config/src/schema.rs  # PreflightConfig schema
src/tool/integrated_config.rs       # PreflightRuntimeConfig resolution
```

## How It Works

`PreflightService` wraps an `EggsactRuntime` with
`audience = "harness"` and a `PreflightPolicy`. Each check method
(e.g., `check_text_replace`, `check_json_valid`) calls an eggsact tool
via `call_json()` directly, bypassing `ToolRegistry` to avoid recursive
tool execution.

The two-tier parsing approach:
1. **Structured fields first**: Read `result`, `findings`, `warnings`,
   `error`, `error_type` from `EggsactCallResult`
2. **String parsing fallback**: If structured fields absent, parse
   `output` text for patterns

## Key Types & APIs

### PreflightSeverity (`src/preflight/service.rs:14-21`)

```rust
pub enum PreflightSeverity {
    Block,     // Deterministic violation — incorrect or unsafe operation
    Warn,      // Likely issue — surfaced but may not block
    Annotate,  // Informational — logs/provenance only
}
```

### PreflightLocation (`src/preflight/service.rs:24-29`)

```rust
pub struct PreflightLocation {
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}
```

### PreflightFinding (`src/preflight/service.rs:32-39`)

```rust
pub struct PreflightFinding {
    pub severity: PreflightSeverity,
    pub machine_code: Option<String>,
    pub message: String,
    pub location: Option<PreflightLocation>,
    pub source_tool: String,
}
```

### PreflightDecision (`src/preflight/service.rs:42-86`)

```rust
pub enum PreflightDecision {
    Allow { findings: Vec<PreflightFinding> },
    Warn { findings: Vec<PreflightFinding> },
    Block { findings: Vec<PreflightFinding> },
}
```

Methods: `is_blocked()`, `has_warnings()`, `findings()`, `summary()`.

### PreflightPolicy (`src/preflight/service.rs:89-178`)

```rust
pub struct PreflightPolicy {
    pub enabled: bool,
    pub mode: PreflightMode,
    pub patch: bool,
    pub config: bool,
    pub shell: bool,
    pub unicode: bool,
    pub log_findings: bool,
    pub model_visible_findings: bool,
}
```

Key methods:
- `should_block(severity)` — returns `true` only when mode is
  `BlockOnDefinite` and severity is `Block`
- `should_surface()` — returns `true` when enabled and
  `model_visible_findings` is on
- `from_config(config: &PreflightConfig)` — fills defaults for missing
  fields

Default: enabled, mode `Warn`, all categories on.

### PreflightMode (`src/preflight/service.rs:110-121`)

```rust
pub enum PreflightMode {
    Off,             // No checks
    Observe,         // Log findings, never alter behavior
    Warn,            // Surface warnings, never block
    BlockOnDefinite, // Block on deterministic failures
}
```

### PreflightService (`src/preflight/service.rs:181-548`)

```rust
pub struct PreflightService {
    runtime: Arc<EggsactRuntime>,
    policy: PreflightPolicy,
}
```

Constructors:
- `new(policy)` — creates fresh `EggsactRuntime` with
  `audience = "harness"` and `max_output_chars: 8_000`
- `with_runtime(runtime, policy)` — shares existing runtime (testing)

Check methods (all return `PreflightDecision`):
- `check_text_replace(text, old, new)` — edit/replace preflight
- `check_json_valid(text)` — JSON validation
- `check_toml_valid(text)` — TOML validation
- `check_config(text)` — auto-detected config format validation
- `check_command(command)` — shell command risk analysis
- `check_text_security(text)` — unicode/confusable/hidden-char inspection

All methods are `async`. On eggsact failure, they return `Allow`
(fail-open) with a debug log.

Parse methods (public for testing):
- `parse_replace_check_result(result) -> PreflightDecision`
- `parse_command_result(result) -> PreflightDecision`
- `parse_text_security_result(result) -> PreflightDecision`

### Helper Functions

- `structured_error_message(result, fallback_prefix)` — extracts error
  from structured fields, falls back to truncated output
- `structured_location(result)` — extracts `PreflightLocation` from
  structured `line`/`column`/`file`/`path` fields
- `parse_match_count(output)` — parses "match_count: N" from text output

## Configuration Surface

### Schema (`[preflight]` in opencode.json)

```json
{
  "preflight": {
    "enabled": true,
    "mode": "warn",
    "patch": true,
    "config": true,
    "shell": true,
    "unicode": true,
    "log_findings": true,
    "model_visible_findings": true
  }
}
```

All fields are `Option<T>` with sensible defaults.
`PreflightPolicy::from_config()` fills defaults for missing fields.
The `mode` field is an enum validated at deserialization time.

### Runtime Config Resolution

`PreflightRuntimeConfig` is resolved by
`integrated_config::resolve_preflight_config()` in
`src/tool/integrated_config.rs`. The resolved config includes a
`profile` field (always `"codegg_core"`) for the `/tool-backends`
report.

## Integration Points

The preflight service is called by mutating tools **before** executing
their primary operation. Current integration points:

| Tool | Check Method | What It Validates |
|------|-------------|-------------------|
| `edit`, `replace`, `apply_patch`, `multiedit` | `check_text_replace` | Replacement exists, is unambiguous |
| Config write operations | `check_json_valid`, `check_toml_valid`, `check_config` | Config syntax validity |
| `bash` | `check_command` | Shell command risk patterns |
| All tools | `check_text_security` | Unicode confusables, hidden chars |

Tool integration is opt-in. Each tool calls the relevant check method
and acts on the `PreflightDecision`:
- `Block` in `BlockOnDefinite` mode → tool returns error
- `Warn` → findings appended to tool output (if `model_visible_findings`)
- `Allow` → proceed normally

## How Findings Are Surfaced

1. **Logging**: If `log_findings` enabled, findings logged at appropriate
   levels (WARN for Block, INFO for Warn, DEBUG for Annotate)
2. **Tool output**: If `model_visible_findings` enabled,
   `PreflightDecision::summary()` appended to tool output string
3. **Blocking**: Only in `BlockOnDefinite` mode with `Block` severity

## Invariants & Gotchas

- **Fail-open**: Eggsact failures return `Allow` — preflight never
  prevents execution due to its own errors
- **No recursion**: `PreflightService` calls `EggsactRuntime` directly,
  bypassing `ToolRegistry` entirely
- **Separate audience**: Constructed with `audience = "harness"` vs
  `"model"` for model-facing tools
- **Not a tool**: `PreflightService` is never registered in any registry
- **`check_text_security` blocks at Warn, not Block**: Even when verdict
  is "block", findings get `PreflightSeverity::Warn` severity — Unicode
  issues default to warn behavior, not block
- **Default `max_output_chars` for harness is 8,000** (vs 12,000 for
  model audience)

## Relationship to Deterministic Tools

| Aspect | Deterministic Tools | Preflight |
|--------|-------------------|-----------|
| Visibility | Model-facing (in ToolRegistry) | Harness-internal (not in ToolRegistry) |
| Purpose | Expose eggsact capabilities to model | Validate before tool execution |
| Interface | `Tool::execute()` via ToolRegistry | `PreflightService::check_*()` methods |
| Audience | `"model"` | `"harness"` |
| Error handling | Returns `ToolError` | Returns `Allow` (fail-open) |

## Testing

```bash
cargo test -p codegg --lib preflight::service    # unit tests
```

Unit tests cover:
- Default policy assertions (`policy_default_is_warn`)
- `should_block` behavior across modes
- Decision helpers (`is_blocked`, `has_warnings`, `summary`)
- `parse_match_count` for text_replace_check output
- Truncation via `truncate_utf8_safe`
- Structured-field parsing (synthetic `EggsactCallResult` with
  `result`/`findings`/`warnings`)
- `structured_error_message` and `structured_location` helpers
- Fallback to string parsing when structured fields absent
- `parse_replace_check_result` with structured fields

## Related Docs

- [tool.md](tool.md) — Deterministic tools and tool system
- [deterministic_tools.md](deterministic_tools.md) — Eggsact tools
- `crates/codegg-config/src/schema.rs` — `PreflightConfig` schema
- `src/eggsact/adapter.rs` — `EggsactRuntime` used by preflight
