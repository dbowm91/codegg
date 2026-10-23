# Preflight Module

Harness-side eggsact preflight integration for automatic validation
before mutating operations. Preflight calls never appear as model-facing
tool calls. CodeGG resolves eggsact `1.2.5` under the repository's Rust `1.89`
MSRV.

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

### PreflightLocation (`src/preflight/service.rs:25-29`)

```rust
pub struct PreflightLocation {
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}
```

### PreflightFinding (`src/preflight/service.rs:33-39`)

```rust
pub struct PreflightFinding {
    pub severity: PreflightSeverity,
    pub machine_code: Option<String>,
    pub message: String,
    pub location: Option<PreflightLocation>,
    pub source_tool: String,
}
```

### PreflightDecision (`src/preflight/service.rs:43-47`, impl `:49-86`)

```rust
pub enum PreflightDecision {
    Allow { findings: Vec<PreflightFinding> },
    Warn { findings: Vec<PreflightFinding> },
    Block { findings: Vec<PreflightFinding> },
}
```

Methods: `is_blocked()`, `has_warnings()`, `findings()`, `summary()`.

### PreflightPolicy (`src/preflight/service.rs:90-107`, impl `:149-178`)

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

### PreflightMode (`src/preflight/service.rs:112-121`)

```rust
pub enum PreflightMode {
    Off,             // No checks
    Observe,         // Log findings, never alter behavior
    Warn,            // Surface warnings, never block
    BlockOnDefinite, // Block on deterministic failures
}
```

### PreflightService (`src/preflight/service.rs:181-184`, impl `:186-549`)

```rust
pub struct PreflightService {
    runtime: Arc<EggsactRuntime>,
    policy: PreflightPolicy,
}
```

Constructors:
- `new(policy) -> Result<Self, ToolError>` — creates a fresh
  `EggsactRuntime` with `audience = "harness"` and
  `max_output_chars: 8_000`; **fallible** — an unusable eggsact profile
  or runtime returns `Err`
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

### Schema (`[preflight]` in `codegg.jsonc`)

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
report. Profile names are validated by the linked eggsact runtime, not by a
duplicated CodeGG allowlist.

## Integration Points

The preflight service is called by mutating tools **before** executing
their primary operation. Current integration points (every call site
verified against source):

| Tool | Check Method | Call site | What It Validates |
|------|-------------|-----------|-------------------|
| `edit` | `check_text_replace` | `src/tool/edit.rs:183` | Replacement exists, is unambiguous |
| `edit` | `check_text_security` | `src/tool/edit.rs:196` | Unicode confusables, hidden chars in the replacement |
| `replace` | `check_text_replace` | `src/tool/replace.rs:191` | Replacement exists, is unambiguous |
| `replace` | `check_text_security` | `src/tool/replace.rs:204` | Unicode confusables, hidden chars in the replacement |
| `apply_patch` | `check_text_security` | `src/tool/apply_patch.rs:260`, `:573` | Unicode confusables, hidden chars in patch content and result |
| `bash` | `check_command` | `src/tool/bash.rs:593`, `src/agent/tool_batch.rs:704` | Shell command risk patterns |
| — | `check_json_valid`, `check_toml_valid`, `check_config` | no production call site | Config syntax validity (methods exist; unused today) |

`write` does **not** run a preflight check. `check_text_security` is not a
universal tool hook — it runs only on the three content-mutating tools
above.

Tool integration is opt-in. Each tool calls the relevant check method
and acts on the `PreflightDecision`:
- `Block` in `BlockOnDefinite` mode → tool returns error
- `Warn` → findings appended to tool output (if `model_visible_findings`)
- `Allow` → proceed normally

### Wiring

`AgentLoop` builds one `PreflightService` per loop from the resolved
`PreflightPolicy` (`src/agent/loop.rs:268-275`):

```rust
let command_preflight = crate::preflight::PreflightService::new(preflight_policy)
    .map_err(|error| tracing::warn!(%error, "command preflight unavailable"))
    .ok();
```

`PreflightService::new` is fallible — an unusable eggsact profile or
runtime yields `Err`, which the loop degrades to `None`. A missing
service means "no preflight", not "preflight failed".

## Preflight Is Advisory: Escalation Requires a Finding

Command preflight is wired by default, so `CommandPreflightResult` is
`Some` for essentially every `bash` call. Escalation is therefore gated
on a **finding**, never on the preflight having run
(`src/agent/tool_batch.rs:258-262`):

```rust
fn preflight_requires_escalation(
    command_preflight: Option<&crate::tool::authorization::CommandPreflightResult>,
) -> bool {
    command_preflight.is_some_and(|result| result.summary.is_some())
}
```

Two rules follow, and both are load-bearing:

- An advisory with nothing to say must never outrank the operator. The
  older `command_preflight.is_some()` test escalated *every* `bash` call,
  including clean ones.
- Because the escalation arm is reached from `PermissionResult::Allow` as
  well as `PermissionResult::Ask`, the old test also discarded stored
  `AlwaysAllow` grants — the agent silently outranked a prior human
  approval.

A separate predicate, `preflight_policy_denies_execution`
(`src/agent/tool_batch.rs:264-268`), gates on `result.blocked` — the
policy's own refusal, which *is* authoritative.

`BashTool` mirrors the split: when the accepted execution authorization
already carries a bounded command-preflight summary, the tool reuses it
instead of running a second scan (`src/tool/bash.rs:582-591`), so a
post-approval scan cannot produce a decision that contradicts the one the
operator approved.

## How Findings Are Surfaced

1. **Logging**: If `log_findings` enabled, findings logged at appropriate
   levels (WARN for Block, INFO for Warn, DEBUG for Annotate)
2. **Tool output**: If `model_visible_findings` enabled,
   `PreflightDecision::summary()` appended to tool output string
3. **Blocking**: Only in `BlockOnDefinite` mode with `Block` severity

## Invariants & Gotchas

- **Fail-open**: Eggsact failures return `Allow` — preflight never
  prevents execution due to its own errors
- **Typed dependency preflight deferred**: eggsact `1.2.5` provides
  `DependencyPreflight`, but CodeGG has no current dependency-edit preflight
  call site. The existing preflight service does not parse dependency results,
  so adopting the wrapper would add an unused parallel path rather than
  replace brittle handling.
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

## Source Verification

Verified 2026-10-23 against source. Corrected:

- The Integration Points table listed four rows that did not match the
  code. `apply_patch` was grouped under `check_text_replace` but actually
  calls only `check_text_security` (`src/tool/apply_patch.rs:260`, `:573`);
  `check_text_security` was claimed to run for "All tools" but runs only in
  `edit`, `replace`, and `apply_patch`; the "Config write operations" row
  had **no production call site** — `check_json_valid`, `check_toml_valid`,
  and `check_config` exist but nothing calls them; and `write` was not
  listed, because it runs no preflight check at all. The table now carries
  the verified call site for every row.
- `PreflightService::new` was documented as infallible; it returns
  `Result<Self, ToolError>` (`src/preflight/service.rs:188`), and
  `AgentLoop` degrades the error to "no preflight"
  (`src/agent/loop.rs:268-275`).

Added: a "Preflight Is Advisory: Escalation Requires a Finding" section
recording `preflight_requires_escalation`
(`src/agent/tool_batch.rs:258-262`), `preflight_policy_denies_execution`
(`:264-268`), and the `BashTool` reuse of the already-approved preflight
summary (`src/tool/bash.rs:582-591`) — the invariant that an advisory must
never create an escalation on its own.

Verified accurate: `PreflightSeverity` `service.rs:14-21`;
`PreflightLocation` `:25-29`; `PreflightFinding` `:33-39`;
`PreflightDecision` enum `:43-47` + impl `:49-86`; `PreflightPolicy`
struct `:90-107` + impl `:149-178`; `PreflightMode` `:112-121`;
`PreflightService` struct `:181-184` + impl `:186-548`; all six
`check_*` methods and their eggsact tool names
(`text_replace_check`, `validate_json`, `validate_toml`,
`config_preflight`, `command_preflight`, `text_security_inspect`) with
their policy gates (`patch`, `config` ×3, `shell`, `unicode`); the three
`parse_*` methods; the helper functions; `PreflightPolicy::default()`
(enabled, `Warn`, all categories on, `service.rs:123-136`); harness
`max_output_chars: 8_000` (`service.rs:192`) vs model `12_000`
(`src/eggsact/adapter.rs:82`); and the `PreflightConfig` schema
(`crates/codegg-config/src/schema.rs:4309-4326`).

Verified 2026-10-06 against source. Corrected (the prior "off by 1-4 lines"
estimate was roughly right, but several refs pointed at an `impl` block range
while the heading claimed the type): `PreflightLocation` `24-29` → `25-29`,
`PreflightFinding` `32-39` → `33-39`, `PreflightDecision` `42-86` → enum
`43-47` + impl `49-86`, `PreflightPolicy` `89-178` → struct `90-107` + impl
`149-178`, `PreflightMode` `110-121` → `112-121`, `PreflightService`
`181-548` → struct `181-184` + impl `186-549`. Verified accurate: 2 files in
`src/preflight/`, 3 `PreflightSeverity` variants, 3-field
`PreflightLocation`, 5-field `PreflightFinding`, 3-variant
`PreflightDecision`, 8-field `PreflightPolicy`, 4 `PreflightMode` variants,
the six `check_*` and three `parse_*` methods, `PreflightPolicy::from_config`/
`should_block`/`should_surface`, `PreflightDecision::is_blocked`/`has_warnings`,
harness `max_output_chars: 8_000` (`service.rs:192`) vs model `12_000`
(`src/eggsact/adapter.rs:82`), and eggsact `1.2.5` (`Cargo.toml:281`).
