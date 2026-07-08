# Preflight Module

Harness-side eggsact preflight integration for automatic validation before mutating operations.

## Overview

**Location**: `src/preflight/`

**Key Responsibilities**:
- Validate edits, config writes, and shell commands before execution
- Surface severity-classified findings (Block/Warn/Annotate)
- Integrate with the eggsact deterministic tool substrate
- Operate as harness-internal — never exposed as model-facing tool calls

**Config**: `[preflight]` section in opencode.json (schema: `PreflightConfig` in `crates/codegg-config/src/schema.rs`)

## Module Structure

```
src/preflight/
├── mod.rs          # Re-exports, module doc
└── service.rs      # PreflightService, types, tests
```

## Types

### PreflightSeverity

```rust
pub enum PreflightSeverity {
    Block,     // Deterministic violation — operation would be incorrect or unsafe
    Warn,      // Likely issue — should be surfaced but may not block
    Annotate,  // Informational — logs/provenance only
}
```

### PreflightLocation

```rust
pub struct PreflightLocation {
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}
```

### PreflightFinding

A structured finding from a preflight check:

```rust
pub struct PreflightFinding {
    pub severity: PreflightSeverity,
    pub machine_code: Option<String>,
    pub message: String,
    pub location: Option<PreflightLocation>,
    pub source_tool: String,
}
```

### PreflightDecision

The service's decision after running checks:

```rust
pub enum PreflightDecision {
    Allow { findings: Vec<PreflightFinding> },
    Warn { findings: Vec<PreflightFinding> },
    Block { findings: Vec<PreflightFinding> },
}
```

Methods: `is_blocked()`, `has_warnings()`, `findings()`, `summary()`.

### PreflightPolicy

Controls preflight behavior:

```rust
pub struct PreflightPolicy {
    pub enabled: bool,
    pub mode: PreflightMode,        // off | observe | warn | block_on_definite
    pub patch: bool,                 // edit/replace preflights
    pub config: bool,                // config write preflights
    pub shell: bool,                 // shell command preflights
    pub unicode: bool,               // unicode/identifier safety
    pub log_findings: bool,
    pub model_visible_findings: bool,
}
```

Default: enabled, mode `Warn`, all categories on.

`should_block(severity)` returns `true` only when mode is `BlockOnDefinite` and severity is `Block`.

### PreflightMode

```rust
pub enum PreflightMode {
    Off,             // No checks
    Observe,         // Log findings, never alter behavior
    Warn,            // Surface warnings, never block
    BlockOnDefinite, // Block on deterministic failures; warn on likely issues
}
```

### PreflightService

```rust
pub struct PreflightService {
    runtime: Arc<EggsactRuntime>,
    policy: PreflightPolicy,
}
```

Constructors:
- `PreflightService::new(policy)` — creates a fresh `EggsactRuntime` with `audience = "harness"`
- `PreflightService::with_runtime(runtime, policy)` — shares an existing runtime (testing/shared use)

Check methods:
- `check_text_replace(text, old, new)` → edit/replace preflight
- `check_json_valid(text)` → JSON validation
- `check_toml_valid(text)` → TOML validation
- `check_config(text)` → auto-detected config format validation
- `check_command(command)` → shell command risk analysis
- `check_text_security(text)` → unicode/confusable/hidden-char inspection

All methods return `PreflightDecision`. On eggsact failure, they return `Allow` (fail-open) with a debug log.

## Policy Configuration

Config schema in `crates/codegg-config/src/schema.rs`:

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

`PreflightPolicy::from_config()` converts from the schema type. All fields are `Option<T>` with sensible defaults.

## Integration Points

The preflight service is designed to be called by mutating tools **before** executing their primary operation. Current integration points:

| Tool | Check Method | What It Validates |
|------|-------------|-------------------|
| `edit`, `replace`, `apply_patch`, `multiedit` | `check_text_replace` | Replacement exists, is unambiguous |
| Config write operations | `check_json_valid`, `check_toml_valid`, `check_config` | Config syntax validity |
| `bash` | `check_command` | Shell command risk patterns |
| All tools | `check_text_security` | Unicode confusables, hidden chars |

Tool integration is opt-in. Each tool calls the relevant check method and acts on the `PreflightDecision`:
- `Block` in `BlockOnDefinite` mode → tool returns error
- `Warn` → findings are appended to tool output (if `model_visible_findings` is on)
- `Allow` → proceed normally

## How Findings Are Surfaced

1. **Logging**: If `log_findings` is enabled, findings are logged at appropriate levels (WARN for Block, INFO for Warn, DEBUG for Annotate)
2. **Tool output**: If `model_visible_findings` is enabled, `PreflightDecision::summary()` is appended to the tool's output string
3. **Blocking**: Only in `BlockOnDefinite` mode with `Block` severity findings

## Relationship to Deterministic Tools

The preflight module and the deterministic tools (`src/tool/deterministic.rs`) both use the eggsact runtime but serve different purposes:

| Aspect | Deterministic Tools | Preflight |
|--------|-------------------|-----------|
| Visibility | Model-facing (registered in ToolRegistry) | Harness-internal (not in ToolRegistry) |
| Purpose | Expose eggsact capabilities to the model | Validate before tool execution |
| Interface | `Tool::execute()` via ToolRegistry | `PreflightService::check_*()` methods |
| Audience | `agent` | `harness` |
| Error handling | Returns ToolError | Returns `Allow` (fail-open) |

## Avoiding Recursive Tool Pollution

The preflight service avoids tool execution cycles through:

1. **Direct runtime usage**: Calls `EggsactRuntime::call_json()` directly, bypassing `ToolRegistry` entirely
2. **Separate audience**: Constructed with `audience = "harness"` (vs `"agent"` for model-facing tools)
3. **No tool registration**: `PreflightService` is not a `Tool` and is never registered in any registry
4. **Fail-open**: Eggsact failures return `Allow` — preflight never prevents execution due to its own errors

## Tests

Unit tests in `src/preflight/service.rs` cover:
- Default policy assertions
- `should_block` behavior across modes
- Decision helper methods (`is_blocked`, `has_warnings`, `summary`)
- `parse_match_count` for text_replace_check output
- String truncation

## See Also

- [tool.md](tool.md) — Deterministic tools (eggsact) and tool system
- `crates/codegg-config/src/schema.rs` — `PreflightConfig` schema
- `src/eggsact/adapter.rs` — EggsactRuntime used by preflight
