# Eggsearch/Eggsact Integration Hardening Pass

## Purpose

This plan is a corrective hardening pass after the initial implementation of the eggsearch and eggsact integration roadmap. The implementation has the right broad architecture: eggsearch is now the external MCP evidence/search backend, eggsact is now an in-process deterministic/preflight substrate, and Codegg keeps the model-facing wrapper/policy boundary.

The remaining work is not feature expansion. It is correctness, safety, release-readiness, and behavior-polish work. The goal is to make the current implementation predictable under disabled configuration, robust around Unicode/output handling, less dependent on string parsing, and easier to validate in CI.

## Current observed shape

The repo now includes:

- Native Codegg wrappers for expanded eggsearch tools: `repo_search`, `repo_fetch`, `repo_map`, `security_search`, `research_search`, `batch_fetch`, and `evidence_bundle`.
- Expanded `search_backend::eggsearch` adapter functions with timeout, output cap, truncation state, and trust framing paths.
- Direct `eggsact` dependency and in-process `EggsactRuntime` adapter.
- Generic `EggsactTool` wrapper and a small default deterministic tool palette.
- Harness-side `PreflightService` with block/warn/annotate findings and `off`/`observe`/`warn`/`block_on_definite` modes.
- New config sections for `[deterministic_tools]` and `[preflight]`.
- Centralized integrated config resolution.
- Expanded tests and docs.

The next pass should assume that this architecture is worth keeping and should focus on specific weaknesses.

## High-priority hardening items

### 1. Make disabled evidence/search config affect model-visible wrappers

#### Problem

The expanded eggsearch wrapper tools appear to be registered unconditionally in `ToolRegistry::with_options`:

- `repo_search`
- `repo_fetch`
- `security_search`
- `research_search`
- `repo_map`
- `batch_fetch`
- `evidence_bundle`

If `[search].backend = "disabled"`, dispatch will likely fail with a disabled/unavailable error, but the tools may still be present in model-facing definitions. That creates unnecessary tool palette noise and contradicts the stronger roadmap acceptance criteria for disabled backends.

#### Required behavior

When evidence/search is disabled:

- `websearch` and `webfetch` should preserve their existing disabled behavior.
- Expanded evidence tools should either be omitted from model-facing definitions or replaced by hidden disabled placeholders.
- `/tool-backends` and `doctor search` should report that evidence/search tools are disabled.
- Raw eggsearch MCP tools must remain hidden by default.

Recommended behavior:

- If `[search].backend = "disabled"`, do not expose expanded evidence wrappers in `definitions()`.
- If a user invokes one by name through tests/debug paths, return a clear disabled error.
- If `[search].backend = "builtin"`, keep only `websearch`/`webfetch` builtin behavior. Expanded repo/security/research/evidence wrappers should be disabled/unavailable unless a builtin equivalent exists.
- If `[search].backend = "eggsearch"`, register the expanded wrappers normally.

#### Implementation guidance

Introduce an evidence registration helper in `ToolRegistry::with_options` or a small function near the new wrappers:

```rust
fn register_evidence_tools(registry: &mut ToolRegistry, evidence_cfg: &EvidenceBackendRuntimeConfig) {
    match evidence_cfg.backend_or_enabled_state() {
        EvidenceBackendState::Eggsearch => { /* register wrappers */ }
        EvidenceBackendState::Builtin => { /* only native websearch/webfetch; expanded wrappers disabled/hidden */ }
        EvidenceBackendState::Disabled => { /* disabled placeholders or no model-visible registration */ }
    }
}
```

The runtime config may need to carry the effective `SearchBackendConfig`, not only an `enabled` boolean.

#### Tests

Add tests covering:

- `[search].backend = "disabled"` omits expanded evidence tools from model definitions.
- `[search].backend = "builtin"` omits expanded evidence tools unless intentionally supported.
- `[search].backend = "eggsearch"` includes expanded evidence wrappers.
- Direct invocation of disabled expanded wrappers returns a clear disabled error if placeholders are retained.
- `/tool-backends` reports disabled evidence tools accurately.

### 2. Make eggsact output truncation UTF-8 safe and semantically correct

#### Problem

`EggsactRuntime::format_response` currently slices with `&output[..max_chars]`. This can panic if `max_chars` lands inside a multibyte UTF-8 code point. The truncation flag is also computed with `output.len() >= max_output_chars`, which marks exactly-at-limit output as truncated even when it was not truncated.

#### Required behavior

- Never panic while truncating arbitrary Unicode text.
- Interpret `max_output_chars` as characters or rename/implement it explicitly as bytes with boundary-safe truncation.
- Report `truncated = true` only when truncation actually occurred.
- Avoid producing output longer than the configured cap by accident when appending the truncation marker.

#### Implementation guidance

Add a small shared helper, either local to `src/eggsact/adapter.rs` or in a utility module:

```rust
pub struct TruncatedText {
    pub text: String,
    pub truncated: bool,
}

pub fn truncate_utf8_safe(input: &str, max_chars: usize, marker: &str) -> TruncatedText { ... }
```

Recommended semantics:

- Treat `max_chars` as Unicode scalar count for user-facing config named `max_output_chars`.
- If `input.chars().count() <= max_chars`, return unchanged with `truncated = false`.
- If truncating, reserve space for a concise marker if possible.
- Never slice by byte index unless using `char_indices()`.

Return truncation metadata from `format_response` rather than recomputing it from string length:

```rust
struct FormattedEggsactResponse {
    output: String,
    truncated: bool,
}
```

#### Tests

Add tests for:

- Truncating multibyte Unicode, emoji, and combining marks does not panic.
- Exactly-at-limit output is not marked truncated.
- One-character-over-limit output is marked truncated.
- Very small caps behave deterministically.
- The final output is valid UTF-8.

### 3. Stop parsing eggsact formatted text for preflight decisions

#### Problem

`PreflightService` currently infers decisions by parsing `EggsactCallResult.output` for strings such as `match_count:`, `matches:`, `risk: high`, `verdict: block`, and `confusable`. This is fragile because `output` is a Codegg-formatted presentation string, not the source-of-truth response structure.

#### Required behavior

Preflight decisions should be based on structured eggsact response fields wherever possible:

- `response.ok`
- `response.machine_code`
- `response.result`
- `response.findings`
- `response.warnings`
- explicit verdict/risk fields if eggsact returns them

String parsing should remain only as a final backward-compatible fallback and should be isolated in clearly named fallback helpers.

#### Implementation guidance

Extend `EggsactCallResult` to carry structured fields from eggsact `ToolResponse`:

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

Then refactor preflight parsers:

- `check_text_replace`: read match count/ambiguity from structured `result` if available.
- `check_json_valid` and `check_toml_valid`: use `success`, `machine_code`, and `error` fields before formatted output.
- `check_command`: read structured verdict/risk/findings before falling back to text search.
- `check_text_security`: read structured verdict/finding severity before string search.

If eggsact does not currently expose enough structured information, prefer improving eggsact upstream rather than adding more brittle Codegg parsing.

#### Tests

Add tests with synthetic `EggsactCallResult` values containing structured JSON fields:

- match count 0 -> block/warn according to mode.
- match count >1 and ambiguity true -> block/warn according to mode.
- command verdict `block` -> block/warn according to mode.
- command verdict `warn` -> warning.
- text security verdict `block` remains warn by default for Unicode safety.
- no structured fields falls back to legacy string parsing.

### 4. Canonicalize and validate deterministic/preflight config

#### Problem

The new config schema has `[deterministic_tools]` and `[preflight]`, but validation appears incomplete. The runtime resolver logs a warning for unknown eggsact profiles but returns the original unknown profile string; `EggsactRuntime::new` later silently falls back to `Profile::Default`.

This creates two issues:

- The warning says fallback happened in the resolver even though the resolver did not canonicalize the value.
- Bad config can silently alter behavior instead of producing a clear validation error or normalized runtime config.

#### Required behavior

- Validate or canonicalize `deterministic_tools.backend`.
- Validate or canonicalize eggsact profile names.
- Validate `model_audience` and `harness_audience`.
- Validate `max_output_chars` range.
- Validate preflight mode, max-related fields, and booleans through typed schema where possible.
- Decide explicitly whether unknown eggsact profiles are errors or warnings.

Recommended policy:

- For production user config, unknown built-in eggsact profile names should be config validation errors unless an explicit `allow_custom_profile = true` is added.
- For runtime resolution, never return an unknown profile while claiming fallback.
- If falling back, set `profile = "default"` or another safe known value in the runtime config.

#### Implementation guidance

Add validation in `Config::validate`:

```rust
if let Some(ref dt) = self.deterministic_tools {
    dt.validate().map_err(...)
}
if let Some(ref pf) = self.preflight {
    pf.validate().map_err(...)
}
```

Implement:

```rust
impl DeterministicToolsConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> { ... }
}

impl PreflightConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> { ... }
}
```

Validation constraints:

- `backend`: `native` or `disabled` initially. Reserve `mcp` only if implemented.
- `profile`: one of eggsact built-ins currently used by Codegg, or custom only behind an explicit field.
- `model_audience`: `model` only for model-facing calls unless another value is intentionally supported.
- `harness_audience`: `harness` or `model`, but default to `harness`.
- `max_output_chars`: nonzero, with upper bound such as `1_000_000` or lower if desired.

#### Tests

Add config tests for:

- Valid deterministic/preflight config parses and validates.
- Unknown backend fails validation.
- Unknown profile fails validation or canonicalizes according to chosen policy.
- Invalid audience fails validation.
- `max_output_chars = 0` fails validation.
- Resolver returns canonical profile after fallback if warning policy is retained.

### 5. Reconcile documentation examples with implemented config

#### Problem

The docs were expanded quickly. Config examples must exactly match implemented schema and behavior. Any aspirational field names that do not parse should be removed or marked as future work.

#### Required behavior

- All README and architecture config examples should parse against `codegg-config` schema.
- Docs should state whether `eggsact` is a git dependency or a crates.io dependency for the current release.
- Docs should distinguish `security` local deterministic review from `security_search` external advisory/evidence search.
- Docs should state that expanded eggsearch wrappers are unavailable when search backend is disabled or builtin.

#### Implementation guidance

Add documentation tests or config fixture tests for snippets from:

- `README.md`
- `architecture/search_backend.md`
- `architecture/deterministic_tools.md`
- `architecture/preflight.md`
- `architecture/config.md`

If full doc-test extraction is too heavy, create `tests/config_examples.rs` with representative TOML snippets copied from docs.

#### Tests

Add tests for:

- Minimal eggsearch config snippet parses.
- Advanced eggsearch config snippet parses.
- Deterministic tools snippet parses.
- Preflight snippet parses.
- Disabled search snippet produces expected runtime config.

### 6. Revisit default deterministic profile and tool palette

#### Problem

The roadmap initially suggested `codegg_core_min` or a similarly narrow profile for the first model-facing default. The implementation defaults to `codegg_core`. This may be fine, but it should be an intentional decision, not drift.

#### Required behavior

- Confirm that every always-visible eggsact tool is available under the default profile.
- Confirm that deferred tools are available under the default profile or handle unavailable tools gracefully.
- Confirm that default profile does not expose tools beyond the explicit Codegg wrapper list.
- Document why `codegg_core` rather than `codegg_core_min` is the default, or change it.

#### Implementation guidance

Add a test that constructs the default `EggsactRuntime` and verifies all registered deterministic wrappers are callable or intentionally unavailable. If some deferred tools require `full` or another profile, either:

- move them behind config, or
- use a broader profile explicitly and document why, or
- skip registering unavailable wrappers.

#### Tests

Add tests for:

- All always-visible deterministic wrappers are available in default profile.
- Deferred wrappers registered by default are available in default profile.
- If config profile changes to `codegg_core_min`, the registry only exposes compatible tools.

### 7. Improve missing eggsearch tool compatibility checks

#### Problem

The adapter has missing-tool error work, but expanded wrappers should also have a predictable compatibility diagnostic before a model tries to call a missing wrapper against an older eggsearch binary.

#### Required behavior

- `doctor search` should classify eggsearch tool coverage as complete/partial/missing.
- `/tool-backends` should expose the same compatibility status.
- Wrapper calls should still fail with actionable errors if an advertised tool disappears or the doctor check was not run.

#### Implementation guidance

Define a required/recommended tool set:

```rust
const EGGSEARCH_REQUIRED_TOOLS: &[&str] = &["web_search", "web_fetch", "provider_status"];
const EGGSEARCH_RECOMMENDED_TOOLS: &[&str] = &["batch_fetch", "repo_search", "repo_fetch", "repo_map", "security_search", "research_search", "build_evidence_bundle"];
```

Report:

- required missing: incompatible.
- recommended missing: partial support.
- all present: complete.

Do not block startup on missing recommended tools.

#### Tests

Add tests for:

- Complete advertised tool set -> complete.
- Missing required tool -> incompatible.
- Missing recommended tool only -> partial.
- Doctor summary contains missing tool names.

### 8. Make preflight failure policy explicit

#### Problem

Many preflight errors currently fail open by returning `Allow { findings: vec![] }`. That is appropriate for warn/observe mode but may be too permissive for `block_on_definite` when the failure is in the validator itself for an operation that is about to mutate files.

#### Required behavior

Define and implement failure policy:

- In `observe` and `warn`, validator runtime errors should fail open with debug logs.
- In `block_on_definite`, validator runtime errors for critical checks should either warn loudly or block depending on a config option.
- Unicode/security hygiene checks should likely fail open by default.
- Exact patch replacement checks and config parse checks may need fail-closed behavior when the preflight service itself cannot run.

#### Implementation guidance

Add `preflight.failure_policy` if necessary:

```toml
[preflight]
failure_policy = "warn"  # "allow" | "warn" | "block_critical"
```

If avoiding schema expansion, at least centralize error handling in helper functions:

```rust
fn decision_for_preflight_runtime_error(&self, check_kind: PreflightCheckKind, error: &ToolError) -> PreflightDecision
```

#### Tests

Add tests for:

- Validator runtime error in warn mode -> warning or allow according to documented policy.
- Validator runtime error in block_on_definite for patch/config critical check -> expected decision.
- Runtime error in unicode check does not block by default.

### 9. Tighten dependency and release reproducibility

#### Problem

Codegg currently uses `eggsact = { git = "https://github.com/eggstack/eggsact" }`. That is acceptable during rapid development but less ideal for public release.

#### Required behavior

Before public release:

- Prefer a crates.io-published `eggsact` version containing the required API.
- If git dependency remains, pin an explicit `rev` and document why.
- Ensure `Cargo.lock` records the exact revision.
- Check whether the git dependency affects downstream installation workflows.

#### Implementation guidance

Options:

1. Publish required eggsact version and switch to:

```toml
eggsact = "1.1.x"
```

2. Temporarily pin git revision:

```toml
eggsact = { git = "https://github.com/eggstack/eggsact", rev = "..." }
```

3. If API is still unstable, feature-gate eggsact integration until release stabilization.

#### Tests/checks

Run:

```bash
cargo metadata --locked
cargo build --locked
cargo test --locked
```

Confirm clean checkout reproducibility.

### 10. Establish explicit validation gate for this integration

#### Problem

The commit history claims substantial tests, but the GitHub connector did not show workflow runs or commit statuses for the current head. The repo needs a local and CI validation record for this integration before calling it release-ready.

#### Required behavior

Run and record:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

If `--all-features` is too broad due to live tests or optional image/plugin dependencies, document the exact release gate and why.

Also run targeted tests:

```bash
cargo test --test fake_eggsearch_mcp
cargo test --test eggsact_adapter
cargo test --test eggsact_deterministic_tools
cargo test --test preflight_integration
cargo test search_backend
```

#### Acceptance criteria

- Validation command output is clean or warnings are documented with follow-up items.
- No default test requires live network access.
- No test relies on a locally installed eggsearch binary unless marked ignored/feature-gated.
- No test relies on sibling local checkout of eggsact.

## Suggested implementation order

1. Fix UTF-8-safe truncation and structured eggsact result retention.
2. Refactor preflight decisions to consume structured eggsact fields.
3. Add deterministic/preflight config validation and canonicalization.
4. Fix disabled evidence/search model visibility.
5. Add compatibility reporting for eggsearch tool coverage.
6. Reconcile docs and config examples with implemented schema.
7. Tighten dependency pinning/versioning.
8. Run full validation gate and patch failures.

This order reduces risk because it first stabilizes low-level output/result handling, then fixes policy surfaces, then validates release posture.

## Files likely to touch

- `src/eggsact/adapter.rs`
- `src/preflight/service.rs`
- `src/tool/mod.rs`
- `src/tool/integrated_config.rs`
- `src/search_backend/bootstrap.rs`
- `src/search_backend/eggsearch.rs`
- `src/search_backend/framing.rs`
- `src/search_backend/state.rs`
- `crates/codegg-config/src/schema.rs`
- `crates/codegg-config/src/paths.rs`
- `src/main.rs`
- `src/tui/commands/diagnostics.rs`
- `README.md`
- `architecture/search_backend.md`
- `architecture/deterministic_tools.md`
- `architecture/preflight.md`
- `architecture/config.md`
- `tests/eggsact_adapter.rs`
- `tests/eggsact_deterministic_tools.rs`
- `tests/preflight_integration.rs`
- `tests/fake_eggsearch_mcp.rs`
- `tests/search_backend_eggsearch.rs`
- new `tests/config_examples.rs` if doc/config snippet validation is added

## Final acceptance criteria

This hardening pass is complete when:

- Disabled search/evidence config removes or hides expanded evidence wrappers from model definitions.
- Eggsact output truncation is UTF-8 safe and truncation metadata is correct.
- Preflight decisions use structured eggsact response data first, with string parsing only as a fallback.
- Deterministic/preflight config has validation and canonical runtime resolution.
- Doctor and `/tool-backends` report eggsearch compatibility coverage.
- Documentation examples parse and match implemented behavior.
- Eggsact dependency is crates.io-pinned or explicitly git-rev-pinned with rationale.
- The full agreed validation gate is clean on a fresh checkout.
