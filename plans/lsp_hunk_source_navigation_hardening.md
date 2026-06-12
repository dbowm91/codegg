# LSP Hunk Source Navigation Hardening Plan

## Purpose

The first `hunkSourceContext` implementation at `09b0207e660f7f42de8e0feb974b663f84238784` landed the core hunk/source navigation stack: shared DTOs, unified diff parser, range primitives, pure navigator, async collector, tool operation, and docs.

This follow-up pass should harden correctness before hunk evidence is used by review, edit-planning, or security workflows.

The focus is not new features. The focus is reliable hunk mapping semantics.

## Current State

Implemented pieces:

- `crates/egglsp/src/hunk_context.rs`
  - `HunkLineRange`
  - `HunkDescriptor`
  - `HunkSourceNavigationRequest`
  - `HunkEvidence`
  - `HunkSourceNavigationLimits`
  - `HunkSourceNavigationResponse`
- `src/lsp/hunk_nav_parser.rs`
  - unified diff parser
  - multi-file parsing at parser level
- `src/lsp/hunk_nav_ranges.rs`
  - range overlap/containment/distance/expansion helpers
  - symbol/diagnostic/location matching helpers
- `src/lsp/hunk_nav.rs`
  - pure `HunkSourceNavigator` consuming `SemanticContextResponse`
- `src/lsp/hunk_nav_collector.rs`
  - async collector coordinating diff parsing and semantic collection
- `src/tool/lsp.rs`
  - model-facing read-only `hunkSourceContext` operation

Known issues to patch:

1. Diagnostic line indexing may be off by one. `FileDiagnostic.line` is used as if 1-indexed in hunk matching, while existing tool adapters convert diagnostics to presentation by adding `+1`.
2. Malformed hunk-looking headers can be silently ignored and become `Ok([])`/`no hunks` instead of a structured parse error.
3. Truncation detection is approximate and can mark exact-cap results as truncated.
4. `HunkSourceNavigationCollector` checks hunk truncation after truncating, so exact-cap hunk counts are marked truncated.
5. `hunkSourceContext` accepts one `file_path` but parser can emit hunks for multiple files; these hunks are not filtered/rejected before using one file's semantic context.
6. Docs claim output includes the full base `SemanticContextResponse`, but `HunkSourceNavigationResponse` currently does not include it.
7. Semantic collection is centered on the first hunk only. That is acceptable for phase one if documented, but current docs should not overclaim per-hunk targeted enrichment.
8. Tool-facing pre-parsed hunks are not exposed even though the DTO supports them.

## Non-Goals

Do not add security hunk enrichment in this pass.

Do not add automatic git-diff discovery in this pass.

Do not add patch application or mutating behavior.

Do not move overlay translation or source-action preview ownership.

Do not require live LSP servers for the new tests.

Do not implement per-hunk LSP collection yet unless needed for a minimal correctness fix.

## Phase 1 — Fix Diagnostic Line Indexing

Problem:

`hunk_nav_ranges::diagnostic_to_range()` currently maps:

```rust
HunkLineRange {
    start_line: diag.line,
    end_line: diag.line,
}
```

Existing presentation code maps `FileDiagnostic.line` to user-visible lines using `line: d.line + 1`, implying `FileDiagnostic.line` is 0-indexed internally.

Implementation:

- Confirm the canonical convention for `FileDiagnostic.line` in `crates/egglsp` / diagnostics code.
- If `FileDiagnostic.line` is 0-indexed internally, update `diagnostic_to_range()` to:

```rust
HunkLineRange {
    start_line: diag.line + 1,
    end_line: diag.line + 1,
}
```

- Add a comment explaining the conversion from diagnostic-native line to hunk DTO 1-indexed line.
- Ensure no double-conversion happens elsewhere in hunk evidence output.

Acceptance criteria:

- Diagnostic line matching uses the same 1-indexed convention as hunk descriptors.
- A diagnostic with internal `line = 9` matches a hunk range `10..10`.
- Existing public diagnostic presentation remains unchanged.

## Phase 2 — Return Structured Errors for Malformed Hunk Headers

Problem:

The parser currently ignores malformed hunk-looking lines such as `@@ bad header`, returning `Ok([])`. This hides the parse problem behind a later `no hunks` error.

Implementation:

- Adjust parser logic so lines beginning with `@@` but not matching a valid hunk header return `ParseHunkError::InvalidHunkHeader(line.to_string())`.
- Keep non-hunk text outside diffs ignored only if it does not look like a hunk header.
- Consider accepting both `@@ -a,b +c,d @@` and hunk headers with trailing context.
- Keep `EmptyInput` for truly empty/whitespace-only diff input.

Acceptance criteria:

- `@@ bad header` returns `InvalidHunkHeader`.
- Valid hunk headers with trailing context still parse.
- Non-diff prose with no hunk header produces `Ok([])` only if that is the desired public behavior; otherwise return a specific `NoHunks` parse error. Prefer a distinct error if it improves tool feedback.

## Phase 3 — Fix Truncation Detection With Pre-Cap Counts

Problem:

The navigator caps results before it can determine whether anything was actually dropped. Patterns like `len() >= max` over-report truncation when exactly `max` items exist.

Implementation:

- In `HunkSourceNavigator::build_evidence()`, compute raw counts before `take()`/`truncate()`.
- Set truncation flags only when `raw_count > max`.
- Apply this to:
  - related symbols;
  - intersecting diagnostics;
  - nearby diagnostics if it has its own cap;
  - references;
  - any future per-hunk collections.
- Consider adding per-hunk `section_truncations` entries for hunk-local truncation instead of only global booleans.

Suggested pattern:

```rust
let raw_related = find_related_symbols_uncapped(...);
let related_truncated = raw_related.len() > self.max_symbols_per_hunk;
let related_symbols = raw_related.into_iter().take(self.max_symbols_per_hunk).cloned().collect();
```

If existing helpers cap internally, either:

- add uncapped helper variants; or
- make helpers return `(Vec<T>, bool)`.

Acceptance criteria:

- Exactly `max` related symbols does not mark truncated.
- `max + 1` related symbols marks truncated.
- Exactly `max` diagnostics does not mark truncated.
- `max + 1` diagnostics marks truncated.
- Exactly `max` references does not mark truncated.
- `max + 1` references marks truncated.

## Phase 4 — Fix Hunk Count Truncation

Problem:

`HunkSourceNavigationCollector` truncates the hunk list, then checks `hunks.len() >= request.max_hunks`. This marks exact-cap inputs as truncated even when no hunk was dropped.

Implementation:

- Record `raw_hunk_count = hunks.len()` before truncating.
- Set `limits.hunks_truncated = raw_hunk_count > request.max_hunks`.
- Then truncate.
- Handle `max_hunks == 0` explicitly. Prefer rejecting with a clear error or coercing to `1`; do not silently return no evidence unless documented.

Acceptance criteria:

- `raw_hunk_count == max_hunks` does not mark truncated.
- `raw_hunk_count > max_hunks` marks truncated.
- `max_hunks == 0` has deterministic documented behavior.

## Phase 5 — Handle Multi-File Patches Correctly

Problem:

The parser can emit hunks for multiple files, but the model-facing `hunkSourceContext` operation accepts a single `file_path` and collects one `SemanticContextResponse`. Passing all parsed hunks through can attach semantic evidence from one file to hunks from another file.

Choose one behavior for this pass.

Preferred for safety/simplicity:

- Reject multi-file patches in `hunkSourceContext` unless all parsed hunks match the supplied `file_path` after normalization.
- Return a clear error such as:

```text
hunkSourceContext currently supports one file per request; patch contains hunks for: src/a.rs, src/b.rs
```

Alternative:

- Filter parsed hunks to `file_path` and add a note listing skipped files.
- This is less surprising if the user intentionally supplies a multi-file diff plus `file_path`.

Recommendation:

- For first hardening pass, reject multi-file patches to avoid silent evidence mismatch.
- Add multi-file support later with per-file semantic collection.

Implementation details:

- Normalize `file_path` and hunk file paths consistently:
  - strip `a/` and `b/` prefixes;
  - compare relative-to-root when possible;
  - avoid requiring absolute path equality against diff-relative paths without normalization.
- If no hunk matches `file_path`, return a clear error.

Acceptance criteria:

- Single-file patch for `file_path` succeeds.
- Multi-file patch for `file_path` errors clearly or filters with explicit notes, depending on chosen behavior.
- Hunks from another file are never mapped against the wrong file's semantic response.

## Phase 6 — Reconcile Response DTO and Documentation

Problem:

Docs say `hunkSourceContext` output includes `semantic — full SemanticContextResponse`, but `HunkSourceNavigationResponse` currently omits the full semantic response.

Choose one design.

Preferred:

- Keep output smaller and do not include full `SemanticContextResponse` by default.
- Update docs to say hunk evidence is derived from a semantic response and preserves selected semantic metadata.
- If a full semantic response is needed later, add an explicit `include_semantic_response` flag.

Alternative:

- Add `semantic: Option<SemanticContextResponse>` to `HunkSourceNavigationResponse`.
- Default it to `None`; only populate if explicitly requested.

Recommendation:

- Update docs only for now. Do not include full semantic response by default.

Acceptance criteria:

- Docs match actual output shape.
- API comments in DTOs do not claim full semantic response is included.
- `AGENTS.md` states the boundary accurately.

## Phase 7 — Document First-Hunk-Centered Semantic Collection

Problem:

The collector sets semantic request position to the first hunk's new start line. Definitions/references/hierarchy are therefore centered on the first hunk and reused for all hunks. This is acceptable for a first pass, but it should not be hidden.

Implementation:

- Add a note in docs and possibly response notes when multiple hunks are present and definitions/references/hierarchy are requested.
- Note that phase one collects one semantic response per file, centered on the first hunk.
- Add future TODO for per-hunk or per-symbol targeted semantic enrichment.

Acceptance criteria:

- Multi-hunk output does not imply each hunk had targeted definition/reference LSP requests.
- Docs accurately describe one semantic collection per file.

## Phase 8 — Align Tool Schema With DTO Support

Problem:

`HunkSourceNavigationRequest` supports pre-parsed `hunks`, but `LspTool` currently constructs `hunks: vec![]` and exposes only `patch`/`max_hunks` for `hunkSourceContext`.

Choose one behavior.

Preferred:

- Do not expose pre-parsed hunks yet.
- Document `hunks` as internal DTO support, not model-facing input.
- Ensure public schema/docs only advertise `patch`.

Alternative:

- Add a `hunks` field to `LspInput` and the tool schema.
- Validate mutual exclusion between `patch` and `hunks`.
- Parse JSON hunk descriptors safely.

Recommendation:

- Keep model-facing input patch-only for now.
- Add pre-parsed hunk input later only if there is a real caller.

Acceptance criteria:

- Public docs and schema agree.
- DTO support does not imply model-facing support.

## Phase 9 — Add Regression Tests

Add focused tests without live LSP servers.

Required parser tests:

- `malformed_hunk_header_returns_invalid_hunk_header`
- `valid_hunk_header_with_context_still_parses`
- `non_diff_without_hunks_returns_clear_error_or_empty_per_policy`

Required range/indexing tests:

- `diagnostic_line_zero_based_converts_to_hunk_one_based`
- `diagnostic_line_matches_expected_hunk_after_conversion`

Required truncation tests:

- `exact_max_related_symbols_not_truncated`
- `over_max_related_symbols_truncated`
- `exact_max_diagnostics_not_truncated`
- `over_max_diagnostics_truncated`
- `exact_max_references_not_truncated`
- `over_max_references_truncated`
- `exact_max_hunks_not_truncated`
- `over_max_hunks_truncated`

Required multi-file tests:

- `single_file_patch_matching_file_path_succeeds`
- `multi_file_patch_rejected_or_filtered_per_policy`
- `hunks_for_other_file_not_mapped_to_target_semantic_response`

Required docs/API consistency tests where practical:

- Serialize `HunkSourceNavigationResponse` and assert there is no `semantic` field if docs choose the compact-output path.

Acceptance criteria:

- Tests are pure or use fake/static semantic responses.
- No live language server required.
- Tests fail against the known issues above.

## Phase 10 — Verification

Run:

```bash
cargo fmt --all
cargo test -p egglsp
cargo test --lib lsp
```

Then, if feasible:

```bash
cargo test --all --workspace
cargo clippy --all-targets --all-features -- -D warnings
```

If full workspace tests or clippy are skipped, record why in the implementation summary.

## Review Checklist

Before this hardening pass is complete:

- Diagnostic-to-hunk line mapping is correct and documented.
- Malformed hunk headers produce actionable parse errors.
- Truncation flags are based on raw pre-cap counts.
- Exact-cap counts are not reported as truncated.
- Multi-file patches are rejected or explicitly filtered; no wrong-file hunk evidence is produced.
- Docs match the actual `HunkSourceNavigationResponse` shape.
- First-hunk-centered semantic collection is documented or noted.
- Tool-facing schema and docs agree on whether pre-parsed hunks are supported.
- No new live-LSP dependency is introduced in tests.

## Expected Follow-Up

After this hardening pass, the next useful phase is integration into review/edit-planning flows:

- use `hunkSourceContext` as the default context operation for diff review;
- add optional hunk-aware security enrichment;
- optionally add multi-file hunk collection with one semantic response per file;
- later add per-hunk targeted definition/reference enrichment if the first-hunk-centered model proves insufficient.
