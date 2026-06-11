# LSP Security Context Cleanup Plan

## Purpose

Tighten the first `securityContext` implementation before adding presets, recursive call expansion, dependency metadata, or dedicated security-agent workflows.

The current pass landed the main feature:

- `securityContext` operation in the `lsp` tool schema.
- `SecurityContextPacket`, `SecurityRiskMarker`, and `SecurityContextLimits` DTOs.
- Deterministic risk marker scanning.
- Security-relevant diagnostics and symbol filtering.
- Optional definitions/references, call hierarchy, and overlay diagnostics.
- No-disk-write patch behavior.
- Basic schema, validation, no-write, marker, result-count, and category-filter tests.

This cleanup pass should focus on correctness, maintainability, and documentation. Do not expand the feature surface.

## Current Issues

1. `risk_markers_truncated` is based on the capped marker count (`risk_markers_len >= max_risk_markers`) and is therefore imprecise for exact cap-sized results.
2. `symbols_truncated` is also based on capped result count (`security_sym_count >= MAX_SECURITY_SYMBOLS`) and is imprecise.
3. Security diagnostics are capped before filtering, so relevant diagnostics after the first raw cap may be dropped.
4. The risk marker scanner and pattern table live inside `src/tool/lsp.rs`, which is already too large.
5. `scan_risk_markers` currently returns only `Vec<SecurityRiskMarker>`, so it cannot accurately report whether markers were dropped.
6. Docs mention `securityContext` in key responsibilities but need a fuller contract section: context packet, not vulnerability scanner; category list; no mutation; unsupported LSP behavior.
7. Tests cover main behavior but not precise truncation accounting for risk markers/symbols/diagnostics.

## Non-Goals

Do not add new security marker categories unless required to preserve existing behavior.

Do not add recursive call hierarchy.

Do not add dependency or CVE metadata.

Do not run external scanners.

Do not add taint analysis.

Do not add security presets.

Do not mutate files.

Do not change the public output schema except to make existing limit fields more accurate.

## Phase 1 — Extract Security Context Helpers

Move security-specific helper types and scanner logic out of `src/tool/lsp.rs` into a dedicated module:

```text
src/tool/lsp_security.rs
```

Move or define there:

```rust
pub(crate) struct RiskPattern { ... }
pub(crate) struct RiskScanResult { ... }
pub(crate) fn scan_risk_markers(...) -> RiskScanResult
pub(crate) fn is_security_relevant_symbol(...)
pub(crate) fn is_security_relevant_diagnostic(...)
pub(crate) fn security_terms() -> &'static [&'static str]
```

Keep DTOs that are serialized by the `lsp` tool in `src/tool/lsp.rs` unless moving them does not create visibility churn. The first cleanup priority is moving pattern/scanner logic.

Recommended scan result:

```rust
pub(crate) struct RiskScanResult {
    pub markers: Vec<SecurityRiskMarker>,
    pub truncated: bool,
}
```

If `SecurityRiskMarker` remains private to `lsp.rs`, either:

1. move it into `lsp_security.rs` and `pub(crate)` it; or
2. keep scanner in `lsp.rs` but move only pattern table and matching utilities.

Preferred: move `SecurityRiskMarker` and related pure helpers into `lsp_security.rs`, re-export internally with:

```rust
use crate::tool::lsp_security::{scan_risk_markers, SecurityRiskMarker};
```

Acceptance criteria:

- `src/tool/lsp.rs` shrinks materially;
- pattern table is isolated;
- scanner is unit-testable without invoking the tool;
- no public API is exposed outside the crate unnecessarily.

## Phase 2 — Precise Risk Marker Truncation

Current logic truncates inside `scan_risk_markers` and later computes:

```rust
risk_markers_truncated: risk_markers_len >= max_risk_markers
```

Change scanner logic to track raw marker count or whether a marker was dropped.

Option A: collect all then cap:

```rust
let truncated = markers.len() > max_markers;
markers.truncate(max_markers);
RiskScanResult { markers, truncated }
```

Option B: bounded collection with overflow flag:

```rust
if markers.len() < max_markers {
    markers.push(marker);
} else {
    truncated = true;
}
```

Option A is simpler and acceptable because the scanner only scans a bounded excerpt.

Update `SecurityContextLimits` population:

```rust
let risk_scan = scan_risk_markers(...);
let risk_markers_truncated = risk_scan.truncated;
let risk_markers = risk_scan.markers;
```

Acceptance criteria:

- exactly `max_risk_markers` markers means `risk_markers_truncated=false`;
- more than `max_risk_markers` markers means `risk_markers_truncated=true`;
- tests cover exact cap and over cap.

## Phase 3 — Filter Diagnostics Before Capping

Current behavior:

```rust
let all_diags = diagnostics.iter().take(MAX_SECURITY_DIAGNOSTICS).collect();
let security_diags = all_diags.iter().filter(is_security_relevant_diagnostic).collect();
```

This can drop a relevant diagnostic after the first raw cap.

Change to:

```rust
let raw_diags: Vec<DiagnosticSummary> = diag_output.diagnostics.iter().map(...).collect();
let relevant: Vec<_> = raw_diags
    .into_iter()
    .filter(|d| is_security_relevant_diagnostic(d, &risk_markers))
    .collect();
let diagnostics_truncated = relevant.len() > MAX_SECURITY_DIAGNOSTICS;
let security_diags = relevant.into_iter().take(MAX_SECURITY_DIAGNOSTICS).collect();
```

If allocation is a concern later, use streaming count and cap. For this pass, clarity matters more.

Acceptance criteria:

- security relevance filtering happens before cap;
- truncation flag reflects filtered relevant diagnostics, not all raw diagnostics;
- tests cover a synthetic list where a relevant diagnostic after many irrelevant ones survives.

## Phase 4 — Precise Security Symbol Truncation

Current behavior:

```rust
let security_syms = all_syms.iter().filter(...).take(MAX_SECURITY_SYMBOLS).collect();
let symbols_truncated = security_sym_count >= MAX_SECURITY_SYMBOLS;
```

Change to raw filtered length:

```rust
let relevant_syms: Vec<SymbolSummary> = all_syms
    .iter()
    .filter(|s| is_security_relevant_symbol(s, &risk_markers, parsed.line))
    .cloned()
    .collect();
let symbols_truncated = relevant_syms.len() > MAX_SECURITY_SYMBOLS;
let security_syms = relevant_syms.into_iter().take(MAX_SECURITY_SYMBOLS).collect();
```

Acceptance criteria:

- exactly `MAX_SECURITY_SYMBOLS` relevant symbols does not mark truncated;
- more than cap marks truncated;
- tests cover exact cap and over cap via pure helper or local synthetic vectors.

## Phase 5 — Security Context Limit Helper

Consider a small pure helper to reduce repeated cap logic:

```rust
pub(crate) fn cap_vec<T>(items: Vec<T>, max: usize) -> (Vec<T>, bool) {
    let truncated = items.len() > max;
    (items.into_iter().take(max).collect(), truncated)
}
```

This can replace the existing dead-code `take_capped` helper or make it public to the module.

Acceptance criteria:

- no unused helper remains with `#[allow(dead_code)]` unless justified;
- cap behavior is tested once and reused;
- hierarchy and security contexts can use the same helper if convenient.

## Phase 6 — Improve Notes and Error Visibility

Current packet omits diagnostic/symbol collection errors from the security packet; local variables are prefixed `_current_diag_err` and `_current_sym_err`.

Do not change schema unless necessary. Instead, add nonfatal notes:

```rust
if let Some(err) = current_diag_err {
    notes.push(format!("diagnostics unavailable: {err}"));
}
if let Some(err) = current_sym_err {
    notes.push(format!("document symbols unavailable: {err}"));
}
```

Similarly, if definitions/references fail, either keep silent or add notes:

```rust
notes.push(format!("definitions unavailable: {e}"));
notes.push(format!("references unavailable: {e}"));
```

Preferred: add notes for all nonfatal LSP subrequest failures. `securityContext` is a review packet; knowing missing sections matters.

Acceptance criteria:

- unavailable diagnostics/symbols/definitions/references are visible in `notes`;
- subrequest failures do not fail the whole packet;
- tests can verify notes by forcing missing/invalid LSP is difficult; pure helper tests are optional.

## Phase 7 — Documentation Completion

Update:

```text
architecture/lsp.md
architecture/tool.md
.opencode/skills/lsp/SKILL.md
AGENTS.md if relevant
```

Add a full `securityContext` section:

```markdown
### Security context packets

`securityContext` is a read-only context-gathering operation for security review. It is not a vulnerability scanner and does not produce vulnerability verdicts.

It combines:
- bounded source excerpt;
- deterministic risk markers;
- security-relevant diagnostics and symbols;
- definitions and references when a target position is supplied;
- shallow call hierarchy when a target position is supplied;
- optional overlay diagnostics for proposed full content or a single-file patch.

It never writes proposed content to disk. Patch/content input is applied only in memory through the existing semantic overlay path.
```

Document supported categories:

```text
auth, crypto, filesystem, network, process, unsafe, serialization, sql, secrets, path_traversal, concurrency
```

Document limits:

```text
risk markers: default 80, max 200
excerpt radius: default 80, max 200
security diagnostics: max 80
security symbols: max 80
references: max 80
```

Add/refine hierarchy section:

```markdown
`callHierarchy` maps incoming to callers and outgoing to callees. `typeHierarchy` maps incoming to supertypes and outgoing to subtypes. Both are shallow, bounded, non-recursive, and read-only. Unsupported servers may return empty sections or error fields.
```

Acceptance criteria:

- docs explicitly distinguish context from verdict;
- docs list categories and limits;
- docs mention no mutation;
- docs document hierarchy behavior beyond the key-responsibility bullet.

## Phase 8 — Tests

Add or update tests in `tests/lsp.rs` and/or module tests for `lsp_security.rs`.

Pure scanner tests:

```text
security_risk_scanner_exact_cap_not_truncated
security_risk_scanner_over_cap_truncated
security_risk_scanner_filters_categories
security_risk_scanner_preserves_line_numbers
security_risk_scanner_caps_matched_text
```

Diagnostic filtering tests:

```text
security_diagnostics_filter_before_cap_keeps_late_relevant_warning
security_diagnostics_exact_cap_not_truncated
security_diagnostics_over_cap_truncated
```

Symbol filtering tests:

```text
security_symbols_exact_cap_not_truncated
security_symbols_over_cap_truncated
security_symbols_target_line_included
security_symbols_keyword_included
security_symbols_near_marker_included
```

Tool behavior tests:

```text
securityContext_limits_risk_markers_precise
securityContext_limits_symbols_precise
securityContext_notes_include_no_position_message
securityContext_patch_does_not_write_disk // keep existing
```

Acceptance criteria:

- new tests are hermetic;
- no test requires a live LSP server;
- exact-cap vs over-cap behavior is covered.

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
cargo test --test lsp securityContext
cargo test --test lsp security_context
cargo test --test lsp security_risk
cargo test -p codegg lsp_security
rg "SecurityContextPacket|SecurityRiskMarker|RiskScanResult|scan_risk_markers|cap_vec" src/tool tests
rg "risk_markers_truncated|symbols_truncated|diagnostics_truncated" src/tool/lsp.rs src/tool/lsp_security.rs tests/lsp.rs
rg "securityContext" architecture/lsp.md architecture/tool.md .opencode/skills/lsp/SKILL.md AGENTS.md
rg "std::fs::write|tokio::fs::write|workspace/applyEdit|executeCommand" src/tool/lsp.rs src/tool/lsp_security.rs crates/egglsp/src/operations.rs
```

## Done Criteria

This cleanup pass is complete when:

- security risk marker scanning is extracted or otherwise modularized;
- marker truncation is precise;
- symbol truncation is precise;
- diagnostics are filtered before capping;
- nonfatal LSP collection failures are visible in notes;
- docs fully describe `securityContext` as a bounded context packet, not a scanner/verdict;
- tests cover exact-cap and over-cap behavior for markers/symbols/diagnostics;
- no mutation or command execution path is introduced.

## Next Pass After This

After cleanup, proceed to configurable security presets:

```text
security_preset = rust_server | rust_cli | web_backend | dependency_review | unsafe_review
```

Presets should tune marker categories, default radius, hierarchy inclusion, and symbol/diagnostic prioritization without changing the no-mutation contract.
