# Deterministic Tools (eggsact)

In-process deterministic correctness utilities backed by the `eggsact`
crate (external dependency). The manifest and lockfile resolve eggsact
`1.2.5` under CodeGG's Rust `1.89` MSRV. These provide compile-time-guaranteed
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

### EggsactRuntime (`src/eggsact/adapter.rs:88-91`, impl `:93-150`)

```rust
pub struct EggsactRuntime {
    registry: eggsact::agent::ToolRegistry,
    config: EggsactConfig,
}
```

- `new(config: EggsactConfig) -> Result<Self, ToolError>` — fallible; invalid
  profiles fail with the upstream accepted-profile list and never fall back
  to `default`
- `call_json(tool, args) -> Result<EggsactCallResult, ToolError>`
- `has_tool(tool) -> bool`
- `config() -> &EggsactConfig`

Full method list is in `src/eggsact/adapter.rs:93-150`; see source for the complete set.

### EggsactConfig (`src/eggsact/adapter.rs:68-75`, `Default` impl `:77`)

```rust
pub struct EggsactConfig {
    pub profile: String,         // default "codegg_core"
    pub audience: String,        // default "model"
    pub max_output_chars: usize, // default 12_000
}
```

### EggsactCallResult (`src/eggsact/adapter.rs:153-169`)

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

### EggsactTool (`src/tool/deterministic.rs:17-26`, impl `:72`)

Generic wrapper mapping a Codegg tool name to an eggsact tool name.
Implements `Tool` trait. Both `execute()` and `execute_structured()`
delegate to `EggsactRuntime::call_json()`.

`name()` returns `self.codegg_name` — a **runtime-supplied** name, not a
literal in the `impl Tool` block. A single `impl Tool for EggsactTool` block
therefore resolves all 15 production names, so a scan of `impl Tool for … name()` for a
string literal cannot see any of them. They are counted instead at their
`EggsactTool::new()` call sites in `src/tool/deterministic.rs` (16 call
sites; the 16th, `probe`, is a `#[cfg(test)]`-only constructor the scan
still counts), and they *do*
appear in the catalogued set of distinct production tool names in
`architecture/tool.md` — 79 measured in total (63 literal + these 16 counted;
true production 78).
`defer_loading()` returns `self.defer` and `expose_in_definitions()` returns
`self.expose`, which is how the always-visible / deferred split is carried
per instance.

### build_eggsact_tools (`src/tool/deterministic.rs:628-855`)

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

### Deferred / Contextual Tools (7)

Discoverable via `tool_search` and flagged as deferred to the provider:

| Tool | Description |
|------|-------------|
| `text_inspect` | Inspect a string for hidden characters, Unicode confusables, mixed scripts |
| `config_preflight` | Validate generated config text. Auto-detects format and runs appropriate validator |
| `identifier_inspect` | Inspect identifiers for validity and collisions |
| `structured_data_compare` | Compare structured config/data output (JSON) |
| `text_fingerprint` | Compute a deterministic SHA-256 fingerprint of text |
| `math_eval` | Evaluate arithmetic/calculator/scientific expressions deterministically (bound to the math-capable `full` runtime, since the default runtime profile lacks them) |
| `unit_convert` | Convert a numeric value between units deterministically (same `full`-runtime binding as `math_eval`) |

Deferred tools are constructed with `defer_loading = true` and
`expose_in_definitions = true`
(`src/tool/deterministic.rs:743-852`). They **are** present in the
model-facing tool definitions; `defer_loading` is a provider-level
extension flag on the emitted `ToolDefinition`
(`crates/codegg-providers/src/provider_core.rs:439`, serialized as
`"defer_loading": true` at `:449`), not a removal from the list.
`ToolRegistry::definitions()` (`src/tool/mod.rs:1484-1493`) filters on
`expose_in_definitions()` and then marks each definition with
`defer_loading`. Providers that do not support the extension (OpenAI,
OpenRouter) ignore the flag.

The `expose_in_definitions = false` treatment belongs to a different
family: the hidden program-only adapters (`git_read`, `lsp_read`), the
disabled-backend stubs, and the `invalid` diagnostic tool. The
always-visible and deferred deterministic wrappers are the same in this
respect — both are advertised, and only the deferred ones carry the
deferral flag.

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
                    └── Deferred → ToolRegistry::with_options()
                                   (registered, catalogued as deferred,
                                    discoverable via tool_search)
```

Key points:
- `EggsactRuntime::new()` is fallible — invalid upstream profiles produce an
  actionable error; registry construction logs the error and skips the
  deterministic wrappers rather than changing policy
- Registration happens in `ToolRegistry::with_options()`
  (`src/tool/mod.rs:1261-1289`), gated on
  `det_cfg.enabled && det_cfg.backend != "disabled"`
- The runtime is constructed from `DeterministicToolsRuntimeConfig`
  resolved by `integrated_config::resolve_integrated_config()`
  (`src/tool/integrated_config.rs:137`)

## Configuration

### Schema (`[deterministic_tools]` in `codegg.jsonc`)

```toml
[deterministic_tools]
enabled = true                    # master switch
backend = "native"                # "native" | "disabled"
profile = "codegg_core"           # validated by the linked eggsact runtime
model_audience = "model"          # audience for model-facing tools
harness_audience = "harness"      # audience for preflight checks
expose_expert_tools = false       # resolved but not consulted at registration
max_output_chars = 12000          # truncation limit (1..1_000_000)
```

`expose_expert_tools` is currently **inert**: it is resolved into
`DeterministicToolsRuntimeConfig`
(`src/tool/integrated_config.rs:204`) and printed by the `codegg` status
command (`src/main.rs:3300`), but no registration or disclosure path
reads it. The always-visible / deferred split is fixed in
`build_eggsact_tools()`, not by this key.

### Validation

`DeterministicToolsConfig::validate()` in `crates/codegg-config/src/schema.rs`
checks:
- `backend` must be `"native"` or `"disabled"`
- `profile` must be non-empty in the config crate; the linked eggsact
  `Profile::from_str_opt()` and `mcp::registry::available_profiles()` are the
  authoritative profile validation and diagnostics source
- `model_audience` must be `"model"` or `"harness"`
- `harness_audience` must be `"harness"` or `"model"`
- `max_output_chars` must be > 0 and <= 1,000,000

Unknown profiles remain visible in resolved configuration and fail when the
in-process runtime is initialized. No fallback profile is substituted.

### Profile Selection

The authoritative profile list is the linked eggsact runtime's
`eggsact::mcp::registry::available_profiles()` (`src/eggsact/adapter.rs:97`
reports it in the error message for an invalid profile). Profiles referenced
inside this repository are:

- `codegg_core` — curated subset for code analysis (default)
- `codegg_core_min` — minimal subset (used by
  `src/tool/integrated_config.rs` tests)
- `default` — eggsact's default profile
- `full` — all available eggsact tools

Additional upstream purpose-specific profiles exist but are not named in this
repository, so they are not listed here; read them from
`available_profiles()` rather than from this table.

The upstream profile set is larger than CodeGG's immediate palette. CodeGG
currently exposes eight always-visible wrappers and five deferred wrappers;
new upstream utilities and the upstream `tool_search`/`tool_invoke` facade
remain intentionally excluded. They must not be imported as a second
progressive-disclosure system.

## Invariants & Gotchas

- `EggsactRuntime::new()` is fallible — deterministic tools are logged and
  skipped on initialization failure, not registered under a different profile
  or as disabled stubs
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

## Source Verification

Verified 2026-10-23 against source. Corrected:

- The deferred-tools section claimed `expose_in_definitions = false` and
  "not sent to the model in tool definitions". Both are wrong: the 5
  deferred wrappers are constructed with `defer_loading = true` **and**
  `expose_in_definitions = true` (`src/tool/deterministic.rs:222-286`),
  so they do appear in `ToolRegistry::definitions()`
  (`src/tool/mod.rs:1484-1493`); `defer_loading` is a provider-level
  extension flag on the emitted definition
  (`crates/codegg-providers/src/provider_core.rs:439-465`), not a
  removal from the list.
- `expose_expert_tools` was documented as "expose deferred tools to
  model". It is **inert**: it is resolved into
  `DeterministicToolsRuntimeConfig`
  (`src/tool/integrated_config.rs:204`) and printed by the status command
  (`src/main.rs:3300`), but no registration or disclosure path reads it.
- The config file name was `opencode.json`; the accepted user filenames
  are `codegg.jsonc` / `codegg.json` / `config.json`
  (`crates/codegg-config/src/paths.rs:63-72`).
- The Registration Flow diagram implied deferred wrappers land only in the
  `ToolCatalog`; they are registered in the `ToolRegistry` as well
  (`src/tool/mod.rs:1277-1279`).

Added: a note on `EggsactTool::name()` resolving at runtime
(`self.codegg_name`), which is why a literal-name scan cannot see the 13
deterministic tool names and why the production tool-name count needs a
second source — their `EggsactTool::new()` call sites in
`src/tool/deterministic.rs`.

Corrected 2026-10-27: the family grew from 13 to 15 production validators —
`math_eval` and `unit_convert` (`deterministic.rs:827-852`, bound to a
math-capable `full` `EggsactRuntime` because the default runtime profile
cannot see them). The measured distinct-name count is now 79 (63 literal +
16 counted `EggsactTool::new()` call sites), with true production at 78
because the 16th call site, `probe` (`deterministic.rs:995-997`), is
`#[cfg(test)]`-only. `build_eggsact_tools` moved to
`deterministic.rs:628-855` with an 8 always-visible / 7 deferred split, and
the `impl Tool for EggsactTool` block is at `:72`.

Corrected 2026-10-23: the note above said the 13 names "do not appear in the
62 distinct production tool names catalogued in `architecture/tool.md`". The
count was wrong twice over — it is 75, not 62 (the 13 are now counted at
their constructor call sites), and the names *do* appear in the catalogued set.
The mechanism the note was explaining is unchanged and still correct: one
`impl Tool for EggsactTool` block resolves all production names at runtime.

Verified accurate 2026-10-27: `EggsactTool` `deterministic.rs:17-26` + impl
`:72`; `build_eggsact_tools` `:628-855` and its 8 always-visible / 7
deferred split (names and order); all 15 production tools `ToolCategory::ReadOnly`;
the `#[cfg(test)]`-only `probe` at `:995-997`.

Verified accurate 2026-10-06 (carried forward; re-verify line numbers on next
pass): `EggsactRuntime` struct `adapter.rs:88-91` + impl
`:93-150`; `EggsactConfig` `:68-75` + `Default` `:77`; `EggsactCallResult`
`:153-169` (10 fields); `EggsactTool` `deterministic.rs:17-26` + impl
`:28`; `build_eggsact_tools` `:106-289` and its 8 always-visible / 5
deferred split (names and order); all 13 tools `ToolCategory::ReadOnly`;
`truncate_utf8_safe` `adapter.rs:18-57`; `format_response` `:185`;
`to_structured_result` `:172` and its `LocalTrusted` provenance;
`eggsact = "1.2.5"` (`Cargo.toml:274`);
`eggsact::mcp::registry::available_profiles()` used at `adapter.rs:97`;
`DeterministicToolsConfig::validate()` in
`crates/codegg-config/src/schema.rs:3982-4008`; the four validation
rules; the four named profiles; harness `max_output_chars: 8_000` vs
model `12_000`; and the registration gate
`det_cfg.enabled && det_cfg.backend != "disabled"`
(`src/tool/mod.rs:1263`).

Verified 2026-10-06 against source (line numbers since moved — see the
2026-10-27 addendum above). Corrected: `EggsactRuntime`
`adapter.rs:88-145` → struct `:88-91` + impl `:93-150`;
`EggsactConfig` `adapter.rs:68-85` → struct `:68-75` + `Default` impl `:77`;
`EggsactCallResult` `adapter.rs:148-164` → `:153-169`; `EggsactTool`
`deterministic.rs:17-91` → struct `:17-26` + impl `:28` (now `:72`); added a
see-source pointer for the `EggsactRuntime` method list. Verified accurate:
`build_eggsact_tools` (then `deterministic.rs:106-289`) and its then 8 always-visible +
5 deferred split, `truncate_utf8_safe` (`adapter.rs:18-57`), the 10
`EggsactCallResult` fields, the two `EggsactConfig`/`EggsactTool` field sets
listed, eggsact `1.2.5`, `format_response` (`adapter.rs:185`),
`to_structured_result` (`adapter.rs:172`). Corrected: the profile-selection
table asserted 11 named upstream profiles, but only `codegg_core`,
`codegg_core_min`, `default`, and `full` are traceable in this repository —
the rest now defer to `eggsact::mcp::registry::available_profiles()`.
