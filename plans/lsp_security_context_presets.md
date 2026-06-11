# LSP Security Context Presets Plan

## Purpose

Add configurable presets for `securityContext` so agents can request review-oriented context tuned to common security review modes without manually specifying category filters and limits every time.

The current stack is now stable enough for this:

- `securityContext` is read-only and schema-exposed.
- Risk scanning is deterministic and modularized in `src/tool/lsp_security.rs`.
- Truncation is precise and surfaced both in nested limits and top-level output/provenance.
- Security docs now distinguish context packets from vulnerability verdicts.

This pass should add preset-driven defaults only. It must not add recursive call traversal, dependency/CVE metadata, taint analysis, or external scanner execution.

## Target Feature

Support input like:

```json
{
  "operation": "securityContext",
  "file_path": "src/server/auth.rs",
  "line": 42,
  "column": 17,
  "security_preset": "rust_server"
}
```

And:

```json
{
  "operation": "securityContext",
  "file_path": "src/main.rs",
  "security_preset": "rust_cli"
}
```

The preset should tune defaults for:

- risk marker categories;
- excerpt radius;
- max risk markers;
- call hierarchy inclusion default;
- optional notes describing the selected preset;
- later, symbol/diagnostic prioritization knobs if needed.

Explicit user inputs should override preset defaults.

## Presets

Initial presets:

```text
rust_server
rust_cli
web_backend
dependency_review
unsafe_review
```

Preset intent:

### rust_server

For Rust network service, daemon, API, WAF, MCP server, or backend review.

Default categories:

```text
auth, network, serialization, filesystem, process, secrets, sql, path_traversal, crypto, unsafe, concurrency
```

Defaults:

```text
radius = 120
max_risk_markers = 120
include_call_hierarchy = true when line+column exists
```

### rust_cli

For CLI tools, local automation, process execution, filesystem, config parsing, and secret handling.

Default categories:

```text
process, filesystem, secrets, path_traversal, serialization, crypto, unsafe, concurrency
```

Defaults:

```text
radius = 100
max_risk_markers = 100
include_call_hierarchy = true when line+column exists
```

### web_backend

For web handlers, routing, request parsing, auth/session flows, database and serialization surfaces.

Default categories:

```text
auth, network, serialization, sql, secrets, filesystem, path_traversal, crypto
```

Defaults:

```text
radius = 120
max_risk_markers = 120
include_call_hierarchy = true when line+column exists
```

### dependency_review

For dependency-sensitive files, configuration, lockfiles, manifests, build scripts, and package-loading code.

Default categories:

```text
secrets, filesystem, process, network, serialization, crypto
```

Defaults:

```text
radius = 80
max_risk_markers = 80
include_call_hierarchy = false unless explicitly requested
```

Notes:

This preset should not yet parse dependency metadata or CVEs. It only tunes local context gathering.

### unsafe_review

For focused review of `unsafe`, FFI-like code, raw pointer logic, transmute, atomics, and concurrency primitives.

Default categories:

```text
unsafe, concurrency, filesystem, process
```

Defaults:

```text
radius = 160
max_risk_markers = 120
include_call_hierarchy = true when line+column exists
```

## Non-Goals

Do not add dependency/CVE lookup.

Do not add recursive call graph expansion.

Do not add taint analysis.

Do not add external scanner execution.

Do not change risk marker pattern semantics except through preset category filters.

Do not mutate files.

Do not change existing explicit `security_categories`, `radius`, `max_risk_markers`, or `include_call_hierarchy` behavior.

## Phase 1 — Add Input Field and Schema

Extend `LspInput`:

```rust
#[serde(default)]
security_preset: Option<String>,
```

Update schema:

```json
"security_preset": {
  "type": "string",
  "enum": ["rust_server", "rust_cli", "web_backend", "dependency_review", "unsafe_review"],
  "description": "Optional securityContext preset that sets default risk categories, radius, marker limits, and call-hierarchy behavior. Explicit inputs override preset defaults."
}
```

Acceptance criteria:

- schema exposes the field;
- schema snapshot updated;
- invalid presets are rejected clearly.

## Phase 2 — Add Preset Type

Add in `src/tool/lsp_security.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecurityPreset {
    RustServer,
    RustCli,
    WebBackend,
    DependencyReview,
    UnsafeReview,
}
```

Parser:

```rust
impl SecurityPreset {
    pub(crate) fn parse(input: Option<&str>) -> Result<Option<Self>, ToolErrorLike> { ... }
}
```

Because `lsp_security.rs` should ideally not depend directly on `ToolError`, use either:

```rust
pub(crate) fn parse_security_preset(input: Option<&str>) -> Result<Option<SecurityPreset>, String>
```

and map string errors to `ToolError::Execution` in `lsp.rs`.

Acceptance criteria:

- valid preset strings parse;
- invalid preset gives clear error listing supported values;
- tests cover valid/invalid parse.

## Phase 3 — Add Preset Defaults Struct

In `src/tool/lsp_security.rs`:

```rust
#[derive(Debug, Clone)]
pub(crate) struct SecurityPresetDefaults {
    pub categories: Vec<String>,
    pub radius: u32,
    pub max_risk_markers: usize,
    pub include_call_hierarchy: bool,
    pub note: &'static str,
}
```

Helper:

```rust
pub(crate) fn preset_defaults(preset: SecurityPreset) -> SecurityPresetDefaults { ... }
```

Use the values listed above.

Acceptance criteria:

- each preset maps to the intended categories and defaults;
- tests verify each preset default shape.

## Phase 4 — Resolve Effective Security Context Settings

Add helper in `src/tool/lsp.rs` or `src/tool/lsp_security.rs`:

```rust
struct EffectiveSecurityContextSettings {
    categories: Option<Vec<String>>,
    radius: u32,
    max_risk_markers: usize,
    include_call_hierarchy: bool,
    preset_note: Option<String>,
}
```

Resolution order:

1. Start from built-in default behavior:

```text
categories = None // all categories
radius = DEFAULT_SECURITY_CONTEXT_RADIUS
max_risk_markers = DEFAULT_MAX_RISK_MARKERS
include_call_hierarchy = has_position
```

2. If `security_preset` is provided, apply preset defaults.
3. Override with explicit user fields:

```text
security_categories overrides preset categories
radius overrides preset radius
max_risk_markers overrides preset max_risk_markers
include_call_hierarchy overrides preset include_call_hierarchy
```

4. Clamp final values:

```text
radius <= MAX_SECURITY_CONTEXT_RADIUS
max_risk_markers <= MAX_RISK_MARKERS
```

Recommended helper signature:

```rust
fn resolve_security_context_settings(
    parsed: &LspInput,
    has_position: bool,
) -> Result<EffectiveSecurityContextSettings, ToolError>
```

Acceptance criteria:

- explicit inputs always win over preset defaults;
- default behavior remains unchanged when no preset is provided;
- values remain clamped;
- tests cover all override cases.

## Phase 5 — Wire into `securityContext`

Replace local settings resolution:

```rust
let radius = parsed.radius.unwrap_or(DEFAULT_SECURITY_CONTEXT_RADIUS).min(MAX_SECURITY_CONTEXT_RADIUS);
let max_risk_markers = parsed.max_risk_markers.unwrap_or(DEFAULT_MAX_RISK_MARKERS).min(MAX_RISK_MARKERS);
let include_call_hierarchy = parsed.include_call_hierarchy.unwrap_or(has_position);
```

with:

```rust
let settings = self.resolve_security_context_settings(&parsed, has_position)?;
let radius = settings.radius;
let max_risk_markers = settings.max_risk_markers;
let include_call_hierarchy = settings.include_call_hierarchy;
```

Use:

```rust
scan_risk_markers(&excerpt, &settings.categories, max_risk_markers)
```

Add preset note to `notes`:

```rust
if let Some(note) = settings.preset_note {
    notes.push(note);
}
```

Acceptance criteria:

- no preset preserves current behavior;
- preset changes category/radius/marker/call-hierarchy defaults;
- explicit fields override preset defaults;
- output notes indicate selected preset.

## Phase 6 — Consider Output Metadata

Do not change packet schema unless simple.

Option A: use notes only.

```text
notes: ["security preset rust_server applied: tuned for Rust network service review"]
```

Option B: add field to `SecurityContextPacket`:

```rust
preset: Option<String>,
```

Preferred for this pass: add `preset: Option<String>` only if schema churn is acceptable. Otherwise notes are enough.

Recommendation: add `preset: Option<String>` because it is useful machine-readable metadata and low-risk.

If added:

```rust
preset: settings.preset_name,
```

Acceptance criteria:

- preset selection is visible in output either via `preset` or notes;
- tests pin whichever form is chosen.

## Phase 7 — Tests

Add unit tests in `src/tool/lsp_security.rs`:

```text
security_preset_parse_accepts_all_known_presets
security_preset_parse_rejects_unknown
rust_server_preset_defaults_match_expected
rust_cli_preset_defaults_match_expected
web_backend_preset_defaults_match_expected
dependency_review_preset_disables_call_hierarchy_by_default
unsafe_review_preset_focuses_unsafe_and_concurrency
```

Add `LspTool` helper tests:

```text
security_context_no_preset_preserves_defaults
security_context_preset_sets_categories
security_context_explicit_categories_override_preset
security_context_explicit_radius_overrides_preset_and_clamps
security_context_explicit_max_markers_overrides_preset_and_clamps
security_context_explicit_include_call_hierarchy_overrides_preset
security_context_invalid_preset_rejected
```

Add integration-ish operation tests with temp files:

```text
securityContext_rust_cli_filters_expected_categories
securityContext_dependency_review_omits_call_hierarchy_without_explicit_flag
securityContext_preset_visible_in_output
```

Keep tests hermetic. Do not require a live LSP server.

Acceptance criteria:

- parse/default/override behavior is thoroughly covered;
- operation-level tests prove presets affect output;
- no live LSP required.

## Phase 8 — Documentation

Update:

```text
architecture/lsp.md
architecture/tool.md
.opencode/skills/lsp/SKILL.md
AGENTS.md if relevant
```

Add section:

```markdown
### Security context presets

`securityContext` supports optional presets through `security_preset`. Presets tune default risk categories, excerpt radius, marker count, and call-hierarchy inclusion. Explicit input fields override preset defaults.
```

Document table:

```markdown
| Preset | Use case | Categories | Radius | Call hierarchy |
|--------|----------|------------|--------|----------------|
| rust_server | Rust services/APIs/daemons | auth, network, serialization, filesystem, process, secrets, sql, path_traversal, crypto, unsafe, concurrency | 120 | true when positioned |
| rust_cli | CLI/local automation | process, filesystem, secrets, path_traversal, serialization, crypto, unsafe, concurrency | 100 | true when positioned |
| web_backend | Web handlers/auth/database | auth, network, serialization, sql, secrets, filesystem, path_traversal, crypto | 120 | true when positioned |
| dependency_review | manifests/build/dependency-sensitive files | secrets, filesystem, process, network, serialization, crypto | 80 | false by default |
| unsafe_review | unsafe/FFI/concurrency review | unsafe, concurrency, filesystem, process | 160 | true when positioned |
```

Mention:

```text
Presets are retrieval defaults, not vulnerability policies. They do not change the read-only contract or add external scanners.
```

Acceptance criteria:

- docs list presets and overrides;
- docs state explicit fields override preset defaults;
- docs repeat no-vulnerability-verdict/no-mutation contract.

## Phase 9 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test -p codegg security_preset
cargo test -p codegg security_context_preset
cargo test --test lsp securityContext
cargo test -p codegg lsp_parameters_schema_snapshot
rg "security_preset|SecurityPreset|SecurityPresetDefaults|resolve_security_context_settings" src/tool tests architecture .opencode AGENTS.md
rg "external scanner|vulnerability scanner|vulnerability verdict|read-only" architecture/lsp.md architecture/tool.md .opencode/skills/lsp/SKILL.md AGENTS.md
```

Manual smoke:

```text
1. Run securityContext with no preset and confirm existing behavior.
2. Run with rust_cli on a CLI/process-heavy file and confirm process/filesystem/secrets categories dominate.
3. Run with dependency_review and line+column; confirm call_hierarchy is omitted unless explicitly requested.
4. Run with web_backend and explicit security_categories=["auth"]; confirm explicit category override wins.
5. Run with unsafe_review and radius override; confirm explicit radius wins but clamps at max.
```

## Done Criteria

This pass is complete when:

- `security_preset` is schema-exposed;
- all five initial presets parse and apply defaults;
- explicit fields override preset defaults;
- invalid presets are rejected clearly;
- selected preset is visible in output metadata or notes;
- tests cover parse/default/override/operation behavior;
- docs explain presets and no-mutation/no-verdict boundaries.

## Next Pass After This

After presets land, the next meaningful path is bounded recursive call expansion for `securityContext`, gated by explicit input fields such as:

```text
call_depth = 0 | 1 | 2
max_call_nodes = 32
```

That should remain read-only, shallow by default, and strictly capped.
