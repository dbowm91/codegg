# Eggsearch/Eggsact Validation and Polish Pass

## Purpose

This plan is the final validation and polish pass after the eggsearch/eggsact integration roadmap and hardening pass. The implementation is now broadly in good shape: expanded eggsearch wrappers are present, eggsact is integrated in-process, deterministic tools are registered through Codegg wrappers, harness-side preflight exists, evidence wrapper registration is gated, UTF-8 truncation is safer, structured eggsact response fields are retained, and config validation has improved.

This pass should avoid new feature expansion. The goal is to verify release semantics, remove ambiguity in edge cases, close dependency reproducibility gaps, and produce a clean validation record.

## Current status to assume

The latest hardening commit addressed the major issues from the previous review:

- Expanded evidence wrappers are now gated by evidence backend enablement.
- Eggsact output formatting uses UTF-8-safe truncation and explicit truncation metadata.
- `EggsactCallResult` carries structured `result`, `findings`, `warnings`, `error_type`, and `error` fields.
- Preflight parsing uses structured eggsact fields first for replacement, command, and text-security decisions.
- Deterministic tools config validation checks backend, profile, audience, and output cap ranges.
- Bootstrap diagnostics classify eggsearch coverage as complete, partial, or incompatible.

The remaining work is mostly verification and policy polish.

## Primary validation questions

Answer these with code/tests rather than prose alone:

1. Does `[search].backend = "builtin"` hide expanded eggsearch-only evidence wrappers?
2. Does `[search].backend = "disabled"` hide or disable all search/evidence tools consistently?
3. Does the final output from `truncate_utf8_safe` respect the configured cap semantics?
4. Can all docs config snippets parse against `codegg-config`?
5. Is the eggsact dependency reproducible for a clean checkout and release flow?
6. Are all default tests local and deterministic, with no live network or installed-eggsearch requirement?
7. Is the full validation gate clean on a fresh checkout?

## Task 1: Verify and polish evidence backend visibility semantics

### Problem

The hardening pass gates expanded evidence wrapper registration on `evidence_config.enabled`. That fixes disabled search in the obvious case, but the runtime distinction between `eggsearch`, `builtin`, and `disabled` still needs explicit verification.

The intended semantics are:

- `backend = "eggsearch"`: expose expanded evidence wrappers and route through eggsearch.
- `backend = "builtin"`: keep only tools with real builtin support. Expanded eggsearch-only tools should not be model-visible unless a builtin equivalent exists.
- `backend = "disabled"`: hide expanded evidence wrappers and disable/hide web search/fetch according to existing policy.

### Implementation guidance

Inspect `src/tool/integrated_config.rs` and ensure `EvidenceBackendRuntimeConfig` carries more than `enabled`; it should retain the effective backend enum or a string label:

```rust
pub enum EvidenceBackendMode {
    Eggsearch,
    Builtin,
    Disabled,
}
```

or:

```rust
pub backend: SearchBackendConfig,
```

Then update registration logic in `ToolRegistry::with_options` so expanded wrappers are registered only for `Eggsearch`.

Do not infer `builtin` as merely enabled. Builtin means legacy builtin web search/fetch only unless Codegg has native implementations for repo/security/research/evidence wrappers.

### Required tests

Add or extend tests to assert model-visible tool names for each backend:

- default/no search config: expected default behavior, likely eggsearch wrappers visible.
- `[search].backend = "eggsearch"`: expanded evidence wrappers visible.
- `[search].backend = "builtin"`: expanded evidence wrappers hidden.
- `[search].backend = "disabled"`: expanded evidence wrappers hidden.

Test names should be direct, for example:

```rust
expanded_evidence_tools_visible_only_for_eggsearch_backend
search_builtin_hides_eggsearch_only_wrappers
search_disabled_hides_expanded_evidence_wrappers
```

### Acceptance criteria

- Builtin mode does not expose eggsearch-only wrappers.
- Disabled mode does not expose expanded wrappers.
- Diagnostics still report the configured backend accurately.

## Task 2: Decide and enforce truncation cap semantics

### Problem

`truncate_utf8_safe` is now Unicode-safe, but its comment states that the marker may overflow the configured limit when the limit is very small. This is safe but ambiguous. Config named `max_output_chars` normally implies a hard maximum.

### Implementation guidance

Choose one policy and implement it consistently.

Preferred policy:

- `max_output_chars` is a hard character cap for final model-visible output.
- If a marker is configured and fits, reserve space for it.
- If the marker does not fit, return only as many marker characters as fit or return a raw truncated prefix without a marker.
- Never return more than `max_output_chars` characters unless `max_output_chars == 0`, which validation should reject.

Example helper behavior:

```rust
truncate_utf8_safe("hello world", 5, "...") == "he..."
truncate_utf8_safe("hello world", 2, "...") == ".." or "he" depending policy
truncate_utf8_safe("🌍🌎🌏", 2, "…") == "🌍…"
```

Document the chosen behavior in the helper docstring and tests.

### Required tests

- final output char count never exceeds cap.
- cap of 1 with marker does not exceed 1 char.
- cap of 2 with marker does not exceed 2 chars.
- emoji/multibyte truncation remains valid UTF-8.
- at-limit output is not marked truncated.
- over-limit output is marked truncated.

### Acceptance criteria

- Helper is Unicode-safe and hard-cap correct.
- Tests encode the cap semantics clearly.

## Task 3: Finish structured preflight parsing for JSON/TOML/config checks

### Problem

Replacement, command, and text-security preflight parsing now use structured eggsact fields first. JSON/TOML/config checks still mostly format `result.output` for error messages. That is okay, but final polish should use structured fields where available.

### Implementation guidance

Add helper functions for structured error extraction:

```rust
fn structured_error_message(result: &EggsactCallResult, fallback_prefix: &str) -> String
fn structured_location(result: &EggsactCallResult) -> Option<PreflightLocation>
```

Use, in order:

1. `result.error`
2. `result.error_type`
3. `result.result.error`, `result.result.message`, `result.result.line`, `result.result.column`
4. first structured finding with message/severity/location
5. formatted output fallback

Apply to:

- `check_json_valid`
- `check_toml_valid`
- `check_config`

### Required tests

Synthetic `EggsactCallResult` tests for:

- JSON parse error with `error` string.
- TOML parse error with structured `line`/`column` location.
- config preflight with structured finding.
- fallback behavior when no structured fields exist.

### Acceptance criteria

- JSON/TOML/config preflight findings prefer structured error details.
- Locations are populated when eggsact provides them.
- Existing fallback output still works.

## Task 4: Validate documentation snippets against config schema

### Problem

Docs were updated quickly. Release readiness requires config snippets in README and architecture docs to parse and validate.

### Implementation guidance

Create `tests/config_examples.rs` with representative snippets copied from docs or derived from them.

Cover:

- minimal eggsearch config.
- advanced eggsearch config.
- builtin search config.
- disabled search config.
- deterministic tools config.
- preflight config.
- combined config with search + deterministic + preflight.

If extracting snippets automatically is too much, manually define canonical snippets in the test and ensure docs match them.

### Required test pattern

```rust
fn parse_and_validate_toml(snippet: &str) {
    let cfg: Config = toml::from_str(snippet).expect("config parses");
    cfg.validate().expect("config validates");
}
```

If Codegg config uses JSON/JSONC in some paths, add equivalent JSON parse tests where relevant.

### Acceptance criteria

- Config examples parse and validate.
- Docs do not contain unsupported fields.
- Builtin/disabled examples document evidence wrapper behavior clearly.

## Task 5: Resolve eggsact dependency reproducibility

### Problem

The integration currently uses a git dependency for eggsact. That is workable for co-development but weaker for public release and clean install reproducibility.

### Preferred release behavior

Use a crates.io version:

```toml
eggsact = "1.1.x"
```

if the required API is published.

### Acceptable interim behavior

If crates.io is not ready, pin an explicit git revision:

```toml
eggsact = { git = "https://github.com/eggstack/eggsact", rev = "<sha>" }
```

and document why this is temporary.

### Implementation guidance

- Check whether the required eggsact API is available in the published version intended for release.
- If yes, switch to crates.io dependency and update `Cargo.lock`.
- If no, pin `rev` and add a release blocker note in docs or a plan comment.
- Run `cargo metadata --locked` after the change.

### Acceptance criteria

- Clean checkout can resolve dependencies reproducibly.
- Release docs clearly describe dependency state.
- No unpinned moving git dependency remains before public release unless intentionally accepted.

## Task 6: Validate eggsearch compatibility diagnostics end-to-end

### Problem

Bootstrap now classifies tool coverage as complete/partial/incompatible, but this needs end-to-end validation through doctor output and fake MCP tests.

### Implementation guidance

Use fake eggsearch MCP tests to simulate:

- full tool set.
- required-only tool set.
- missing recommended tool set.
- missing required tool set.
- no tools.

Assert doctor/bootstrap summary lines include:

- `Tool coverage: complete`
- `Tool coverage: partial`
- `Missing recommended: ...`
- `Tool coverage: incompatible`
- `Missing required: ...`

### Acceptance criteria

- Doctor output is actionable for old eggsearch binaries.
- Partial support does not block startup.
- Missing required tools are clearly reported as incompatible.

## Task 7: Confirm default deterministic palette availability

### Problem

The default deterministic profile is `codegg_core`. The intended default is acceptable if every registered visible and deferred wrapper is actually available under that profile. This should be verified explicitly.

### Implementation guidance

Add a test that builds the default registry and/or default eggsact runtime and verifies:

- every always-visible deterministic wrapper has an underlying eggsact tool available.
- every deferred deterministic wrapper has an underlying eggsact tool available, or is not registered.
- changing profile to `codegg_core_min` produces expected reduced behavior or still supports the current palette.

If there is no public way to inspect wrapper metadata, add a small test-only helper or make deterministic wrapper metadata available in a non-invasive way.

### Acceptance criteria

- Default profile/tool palette is verified.
- If `codegg_core` is retained as default, docs explain why.
- No registered deterministic wrapper fails because the eggsact profile lacks the underlying tool.

## Task 8: Final release validation gate

### Required commands

Run on a clean checkout:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo metadata --locked
```

Run targeted tests explicitly:

```bash
cargo test --test fake_eggsearch_mcp
cargo test --test eggsact_adapter
cargo test --test eggsact_deterministic_tools
cargo test --test preflight_integration
cargo test search_backend
cargo test config_examples
```

If any command is not currently valid, document the reason and replace it with the correct command. Do not silently skip validation.

### Test policy

- Default tests must not require live network access.
- Default tests must not require an installed `eggsearch` binary.
- Default tests must not require a sibling local eggsact checkout.
- Live eggsearch/provider smoke tests should be feature-gated or ignored by default.

### Acceptance criteria

- Full gate passes or failures are documented with a follow-up plan.
- Validation results are included in the commit message or a short `plans/` follow-up note.
- No hidden live-service dependency exists in default tests.

## Task 9: Documentation polish

### Required doc updates

Update docs only where implementation requires clarification:

- README: dependency state for eggsact, install guidance for eggsearch, default backend behavior.
- `architecture/search_backend.md`: builtin vs eggsearch vs disabled semantics, expanded wrapper availability, compatibility diagnostics.
- `architecture/deterministic_tools.md`: default profile, visible vs deferred tool palette, structured output/truncation semantics.
- `architecture/preflight.md`: structured parsing, failure-open behavior, mode semantics.
- `architecture/config.md`: validated config snippets.
- `AGENTS.md`: contributor boundary reminders.

### Acceptance criteria

- Docs match code and tests.
- No aspirational config fields are documented as current behavior.
- Release caveats are explicit rather than implicit.

## Task 10: Optional small cleanup after validation

Only do these if the validation pass already touches nearby code:

- Rename internal variables from generic `enabled` to `backend_mode` where clarity matters.
- Add comments around intentional fail-open preflight runtime errors.
- Centralize repeated structured field extraction helpers in `preflight/service.rs`.
- Add a `SearchBackendConfig` display helper to avoid debug-lowercase formatting.
- Tighten wording around `trusted` vs `instruction-trusted` in evidence frames.

Avoid broad refactors in this pass.

## Final acceptance criteria

This pass is complete when:

- Eggsearch-only wrappers are visible only when eggsearch backend is active.
- UTF-8 truncation is hard-cap correct and tested.
- JSON/TOML/config preflight uses structured error fields where available.
- Docs config snippets parse and validate.
- Eggsact dependency is crates.io-pinned or git-rev-pinned with rationale.
- Eggsearch compatibility diagnostics are tested end-to-end.
- Default deterministic palette availability is verified.
- Full local validation gate is clean or any failures are documented with a precise follow-up.

## Non-goals

- Do not add new eggsearch tools.
- Do not add new eggsact tools.
- Do not redesign the tool registry.
- Do not remove legacy builtin websearch/webfetch fallback in this pass.
- Do not require live network tests in default CI.
