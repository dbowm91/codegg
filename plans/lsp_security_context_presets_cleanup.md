# LSP Security Context Presets Cleanup and Test Hardening Plan

## Purpose

Tighten the configurable `securityContext` presets implementation before moving to bounded recursive call expansion.

The presets pass is functionally landed:

- `security_preset` is schema-exposed.
- `SecurityPreset`, `parse_security_preset`, `SecurityPresetDefaults`, and `preset_defaults` exist in `src/tool/lsp_security.rs`.
- `securityContext` applies preset defaults and explicit overrides.
- Selected preset is surfaced in output and notes.
- Basic parser/default/schema/operation tests exist.

This pass should focus on maintainability and test coverage only.

## Current Issues

1. `resolve_security_context_settings` returns a raw tuple:

```rust
Result<(Option<Vec<String>>, u32, usize, bool, Option<String>), ToolError>
```

and uses `#[allow(clippy::type_complexity)]`.

2. Override behavior is implemented but not fully covered:

- explicit radius override;
- radius clamping;
- explicit max marker override;
- max marker clamping;
- explicit include-call-hierarchy override;
- dependency_review default call-hierarchy behavior at operation level.

3. Preset defaults are tested, but effective settings resolution is not directly tested as a first-class contract.

4. The selected preset output is tested, but preset override interactions are only partially tested through category filtering.

## Non-Goals

Do not add new presets.

Do not add recursive call expansion.

Do not change scanner categories.

Do not change risk marker matching behavior.

Do not change output schema except internal cleanup if necessary.

Do not change read-only/no-verdict behavior.

Do not run external scanners.

## Phase 1 — Replace Tuple with Settings Struct

Add a small internal struct near `LspInput` or in `lsp_security.rs` if visibility stays clean:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct EffectiveSecurityContextSettings {
    categories: Option<Vec<String>>,
    radius: u32,
    max_risk_markers: usize,
    include_call_hierarchy: bool,
    preset_note: Option<String>,
    preset_name: Option<String>,
}
```

If placed in `src/tool/lsp_security.rs`, make fields `pub(crate)` and keep type crate-private:

```rust
pub(crate) struct EffectiveSecurityContextSettings { ... }
```

Change:

```rust
fn resolve_security_context_settings(...) -> Result<(Option<Vec<String>>, u32, usize, bool, Option<String>), ToolError>
```

to:

```rust
fn resolve_security_context_settings(
    parsed: &LspInput,
    has_position: bool,
) -> Result<EffectiveSecurityContextSettings, ToolError>
```

Remove:

```rust
#[allow(clippy::type_complexity)]
```

Update `securityContext` call site:

```rust
let settings = Self::resolve_security_context_settings(&parsed, has_position)?;
let (excerpt, excerpt_truncated) = if has_position {
    Self::build_source_excerpt(&file, parsed.line, settings.radius)?
} else {
    Self::build_source_excerpt(&file, None, settings.radius)?
};
let risk_scan = scan_risk_markers(&excerpt, &settings.categories, settings.max_risk_markers);
...
let call_hierarchy = if settings.include_call_hierarchy && has_position { ... }
...
if let Some(note) = settings.preset_note { notes.push(note); }
...
preset: settings.preset_name,
```

Acceptance criteria:

- no raw tuple for effective security settings;
- no `type_complexity` allow needed;
- call site readability improves;
- output behavior remains unchanged.

## Phase 2 — Make Resolution Rules Explicit

Keep resolution order exactly as currently intended:

1. Built-in defaults:

```text
categories = None
radius = DEFAULT_SECURITY_CONTEXT_RADIUS
max_risk_markers = DEFAULT_MAX_RISK_MARKERS
include_call_hierarchy = has_position
preset_note = None
preset_name = None
```

2. If `security_preset` exists, apply preset defaults:

```text
categories = Some(defaults.categories)
radius = defaults.radius
max_risk_markers = defaults.max_risk_markers
include_call_hierarchy = defaults.include_call_hierarchy
preset_note = Some(defaults.note)
preset_name = Some(input preset string)
```

3. Explicit fields override:

```text
security_categories
radius
max_risk_markers
include_call_hierarchy
```

4. Clamp:

```text
radius <= MAX_SECURITY_CONTEXT_RADIUS
max_risk_markers <= MAX_RISK_MARKERS
```

Acceptance criteria:

- no behavior drift;
- invalid preset still returns a clear `ToolError::Execution`;
- explicit overrides still win over preset defaults.

## Phase 3 — Add Direct Effective Settings Tests

Add unit tests around `resolve_security_context_settings` in `src/tool/lsp.rs` tests.

Recommended tests:

```text
security_context_settings_no_preset_preserves_defaults_without_position
security_context_settings_no_preset_preserves_defaults_with_position
security_context_settings_rust_server_sets_defaults
security_context_settings_dependency_review_disables_call_hierarchy
security_context_settings_explicit_categories_override_preset
security_context_settings_explicit_radius_overrides_preset
security_context_settings_radius_clamps_to_max
security_context_settings_explicit_max_markers_overrides_preset
security_context_settings_max_markers_clamps_to_max
security_context_settings_explicit_include_call_hierarchy_false_overrides_preset
security_context_settings_explicit_include_call_hierarchy_true_overrides_dependency_review
security_context_settings_invalid_preset_rejected
```

These tests should construct `LspInput` directly. Since `LspInput` is private in the same module tests, this should be straightforward.

Use a helper:

```rust
fn security_context_input() -> LspInput { ... }
```

Acceptance criteria:

- override/clamp behavior is tested without invoking LSP;
- tests are hermetic and fast;
- expected values are explicit.

## Phase 4 — Strengthen Operation-Level Preset Tests

Keep existing operation tests, but add two specific checks that prove settings affect behavior externally:

### dependency_review omits call hierarchy unless explicitly requested

```rust
#[tokio::test]
async fn security_context_dependency_review_omits_call_hierarchy_without_explicit_flag() { ... }
```

Expected:

```rust
assert!(v["results"]["call_hierarchy"].is_null());
assert_eq!(v["results"]["preset"], "dependency_review");
```

This should not require a live LSP server because the section should be omitted before any hierarchy request.

### dependency_review include_call_hierarchy=true attempts section when positioned

This may produce an error-bearing hierarchy summary if no server supports it, so only assert that the section is present or that validation is not rejected. If this risks live LSP dependency, skip and rely on settings tests.

Recommended safer test:

```text
security_context_dependency_review_explicit_include_call_hierarchy_setting_resolves_true
```

via settings helper, not full operation.

Acceptance criteria:

- operation-level test proves preset visible and call hierarchy default false for dependency review;
- no live LSP dependency is introduced.

## Phase 5 — Ensure Preset Name Is Canonical

Currently output likely uses `parsed.security_preset.clone()`.

Keep that if input is constrained to canonical strings by parser. Since invalid preset is rejected first, this is acceptable.

Alternatively, add:

```rust
impl SecurityPreset {
    pub(crate) fn as_str(self) -> &'static str { ... }
}
```

and use canonical output from parsed enum.

Preferred cleanup:

```rust
pub(crate) fn security_preset_name(preset: SecurityPreset) -> &'static str
```

Then output canonical names even if parser later accepts aliases.

Acceptance criteria:

- output preset is one of the schema enum values;
- tests assert canonical value.

## Phase 6 — Docs Touch-Up

The docs are already good. Only update if the implementation changes output field semantics.

Check:

```text
architecture/lsp.md
architecture/tool.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Ensure the docs say:

```text
Explicit fields override preset defaults.
```

and:

```text
Presets are retrieval defaults, not vulnerability policies.
```

Acceptance criteria:

- no docs drift from implementation;
- no new docs implying scanner/verdict behavior.

## Phase 7 — Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test -p codegg security_context_settings
cargo test -p codegg security_preset
cargo test -p codegg security_context_dependency_review
cargo test -p codegg lsp_parameters_schema_snapshot
rg "EffectiveSecurityContextSettings|resolve_security_context_settings|type_complexity|security_preset" src/tool/lsp.rs src/tool/lsp_security.rs tests/lsp.rs architecture/lsp.md architecture/tool.md .opencode/skills/lsp/SKILL.md AGENTS.md
```

Expected search result:

```text
no #[allow(clippy::type_complexity)] on resolve_security_context_settings
```

## Done Criteria

This cleanup pass is complete when:

- effective security settings use a named struct, not a raw tuple;
- `#[allow(clippy::type_complexity)]` is removed;
- explicit override and clamp behavior is directly tested;
- dependency_review call-hierarchy default is tested;
- preset output remains canonical and machine-readable;
- docs remain accurate;
- no feature expansion or behavior drift is introduced.

## Next Pass After This

Proceed to bounded recursive call expansion for `securityContext`, gated by explicit inputs such as:

```text
call_depth = 0 | 1 | 2
max_call_nodes = 32
```

That future pass should remain read-only, default to no recursion, and enforce strict caps.
